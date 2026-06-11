use std::path::Path;

use bevy::app::AppExit;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::characters::{CharacterKind, SelectedParty};
use crate::core::{GameState, Game_State, MainCamera};
use crate::world::SetLeaderRequest;
use crate::render3d::{iso_camera_offset, spawn_menu_stage_camera, PlaceholderVisual, CHAR_HEIGHT};
use crate::save::{AutoSaveSettings, SaveAction, SaveRequest, SaveSlot};
use crate::settings::{GraphicsSettings, GraphicsToggle, GRAPHICS_TOGGLES};
use crate::ui_style::{
    bottom_scrim, button_node, button_text, button_text_lg, button_visual, font_size, label_text,
    menu_scene_overlay, overlay_root, palette, panel, scene_glow, scene_vignette, spacing, top_scrim,
};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ResumeState>()
            .init_resource::<MainMenuPage>()
            .init_resource::<PauseMenuPage>()
            .add_systems(Startup, spawn_menu_scene)
            .add_systems(Update, manage_scene_cameras)
            .add_systems(Update, animate_menu_actors)
            .add_systems(Update, orbit_menu_camera)
            .add_systems(Update, sync_main_menu_page)
            .add_systems(Update, sync_pause_menu_page)
            .add_systems(Update, spawn_main_menu_ui)
            .add_systems(Update, spawn_pause_menu_ui)
            .add_systems(Update, spawn_game_over_ui)
            .add_systems(Update, spawn_victory_ui)
            .add_systems(Update, animate_title)
            .add_systems(Update, toggle_pause_state)
            .add_systems(Update, handle_menu_actions)
            .add_systems(Update, update_autosave_status_text)
            .add_systems(Update, update_graphics_toggle_text)
            .add_systems(Update, update_load_slot_status);
    }
}

/// Entity of the dedicated 3D camera that frames the main-menu stage. Stored so
/// the menu UI can target it explicitly while the gameplay world camera is
/// switched off (see [`manage_scene_cameras`]).
#[derive(Resource)]
struct MenuSceneCamera(Entity);

/// Tag for the menu stage's camera.
#[derive(Component)]
struct MenuViewCamera;

/// One of the roster characters posing on the menu stage. Drives the idle
/// animation in [`animate_menu_actors`]. Public so the outline-width system in
/// `render3d` can exclude these (they're framed by a different camera).
#[derive(Component)]
pub struct MenuActor {
    /// Resting position (capsule centre) on the stage.
    home: Vec3,
    /// Per-actor phase offset so the cast doesn't move in lockstep.
    phase: f32,
    /// How far this one sways, in radians.
    sway: f32,
}

/// The menu's 3D stage lives far from the gameplay world so neither camera ever
/// sees the other's geometry. Lights are global, so the cast is still lit by the
/// scene sun; the menu camera adds its own brighter ambient fill.
const MENU_STAGE_ORIGIN: Vec3 = Vec3::new(0.0, -60_000.0, 0.0);
/// Height of the dais the cast stands on (a low prop box, base on z = 0).
const MENU_DAIS_HEIGHT: f32 = 14.0;
/// Spacing between adjacent cast members along the line-up axis.
const MENU_ACTOR_SPACING: f32 = 46.0;
/// How far the centre of the line-up bulges toward the camera (a shallow arc).
const MENU_ARC_BULGE: f32 = 34.0;

/// Build the 3D title-screen stage once at startup: a dedicated toon camera, a
/// dais, and the seven-strong roster arranged in a shallow arc facing the
/// camera. Animated by [`animate_menu_actors`] / [`orbit_menu_camera`].
fn spawn_menu_scene(mut commands: Commands) {
    let origin = MENU_STAGE_ORIGIN;
    // Resting capsule-centre height: standing on top of the dais.
    let stand_z = MENU_DAIS_HEIGHT + CHAR_HEIGHT * 0.5;
    // Frame a touch above the dais so the cast sits in the lower-middle third.
    let focus = origin + Vec3::new(0.0, 0.0, CHAR_HEIGHT * 0.55);

    let cam = spawn_menu_stage_camera(&mut commands, focus);
    commands.entity(cam).insert((
        MenuViewCamera,
        // Order above the world camera (0); only one is active at a time anyway.
        Camera {
            order: 1,
            ..default()
        },
        Name::new("MenuStageCamera"),
    ));
    commands.insert_resource(MenuSceneCamera(cam));

    // Dais: a low, dark plinth for the cast to stand on. Rotated -45° about Z so
    // its long axis runs under the diagonal line-up (the `line` axis below).
    commands.spawn((
        PlaceholderVisual::prop(
            Color::srgb(0.10, 0.11, 0.17),
            Vec2::new(MENU_ACTOR_SPACING * 8.0, 132.0),
            MENU_DAIS_HEIGHT,
        ),
        Transform::from_translation(origin)
            .with_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_4)),
        Name::new("MenuDais"),
    ));

    // Screen-horizontal axis on the ground for the iso camera is roughly
    // (1,-1); the toward-camera axis is roughly (1,1). Spread the cast along the
    // former and bulge the middle along the latter for a shallow arc.
    let line = Vec3::new(1.0, -1.0, 0.0).normalize();
    let toward_cam = Vec3::new(1.0, 1.0, 0.0).normalize();

    let roster = CharacterKind::ALL;
    let last = (roster.len() - 1) as f32;
    let mid = last * 0.5;
    for (i, kind) in roster.into_iter().enumerate() {
        let k = i as f32 - mid; // centred index, e.g. -3..3
        let arc = (1.0 - (k / mid).powi(2)) * MENU_ARC_BULGE; // 0 at ends, max at centre
        let ground = origin + line * (k * MENU_ACTOR_SPACING) + toward_cam * arc;
        let home = ground + Vec3::Z * stand_z;
        commands.spawn((
            PlaceholderVisual::character(kind.color()).toon(),
            Transform::from_translation(home),
            kind,
            MenuActor {
                home,
                // Spread phases around the circle so the idle looks organic.
                phase: i as f32 * 0.9,
                sway: 0.10 + (i % 3) as f32 * 0.04,
            },
            Name::new(format!("MenuActor({})", kind.display_name())),
        ));
    }
}

/// Keep exactly one scene camera active: the 3D menu camera in `MainMenu`, the
/// gameplay world camera everywhere else. This is what makes "New Game" actually
/// leave the title screen for the world rather than peel back an overlay.
fn manage_scene_cameras(
    game_state: Res<GameState>,
    mut menu_cam: Query<&mut Camera, (With<MenuViewCamera>, Without<MainCamera>)>,
    mut world_cam: Query<&mut Camera, (With<MainCamera>, Without<MenuViewCamera>)>,
) {
    let in_menu = game_state.0 == Game_State::MainMenu;
    if let Ok(mut cam) = menu_cam.single_mut() {
        if cam.is_active != in_menu {
            cam.is_active = in_menu;
        }
    }
    if let Ok(mut cam) = world_cam.single_mut() {
        if cam.is_active == in_menu {
            cam.is_active = !in_menu;
        }
    }
}

/// Idle animation for the cast: a soft vertical bob, a gentle yaw sway, a slight
/// forward/back tilt, and a breathing scale — each offset by the actor's phase.
fn animate_menu_actors(time: Res<Time>, mut actors: Query<(&MenuActor, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (actor, mut tf) in &mut actors {
        let p = t * 1.4 + actor.phase;
        let bob = p.sin() * 3.0;
        let yaw = (p * 0.6).sin() * actor.sway;
        let tilt = (p * 0.5).cos() * 0.05;
        let breathe = 1.0 + (p * 1.1).sin() * 0.03;
        tf.translation = actor.home + Vec3::Z * bob;
        tf.rotation = Quat::from_rotation_z(yaw) * Quat::from_rotation_x(tilt);
        tf.scale = Vec3::new(1.0, 1.0, breathe);
    }
}

/// Slowly sway the menu camera around the stage for a living, cinematic title
/// screen. A gentle oscillation (±~20°) rather than a full spin.
fn orbit_menu_camera(time: Res<Time>, mut cam: Query<&mut Transform, With<MenuViewCamera>>) {
    let Ok(mut tf) = cam.single_mut() else {
        return;
    };
    let focus = MENU_STAGE_ORIGIN + Vec3::new(0.0, 0.0, CHAR_HEIGHT * 0.55);
    let yaw = (time.elapsed_secs() * 0.12).sin() * 0.35;
    let offset = Quat::from_rotation_z(yaw) * iso_camera_offset();
    *tf = Transform::from_translation(focus + offset).looking_at(focus, Vec3::Z);
}

#[derive(Resource)]
pub struct ResumeState(pub Game_State);

impl Default for ResumeState {
    fn default() -> Self {
        Self(Game_State::Exploring)
    }
}

#[derive(Resource, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuPage {
    Title,
    Load,
    Settings,
}

impl Default for MainMenuPage {
    fn default() -> Self {
        Self::Title
    }
}

#[derive(Resource, Clone, Copy, PartialEq, Eq)]
pub enum PauseMenuPage {
    Main,
    Settings,
    Party,
}

impl Default for PauseMenuPage {
    fn default() -> Self {
        Self::Main
    }
}

#[derive(Component)]
struct MainMenuRoot(MainMenuPage);

#[derive(Component)]
struct PauseMenuRoot(PauseMenuPage);

#[derive(Component, Clone, Copy)]
enum MenuButtonAction {
    StartGame,
    QuitGame,
    OpenLoadPage,
    OpenSettingsPage,
    BackToTitle,
    ResumeGame,
    ReturnToTitle,
    /// From the defeat screen: start a fresh run at the party-selection screen.
    RestartRun,
    PauseOpenSettings,
    PauseOpenParty,
    PauseBack,
    /// From the pause "Party" page: promote this character to leader.
    MakeLeader(CharacterKind),
    LoadSlot1,
    LoadSlot2,
    LoadSlot3,
    ToggleAutosave,
    ToggleGraphics(GraphicsToggle),
}

#[derive(Component)]
struct AutosaveStatusText;

#[derive(Component)]
struct GraphicsToggleText(GraphicsToggle);

#[derive(Component)]
struct LoadSlotStatusText(SaveSlot);

const HERO_BTN: f32 = 56.0;
const ROW_BTN: f32 = 44.0;
const TOGGLE_BTN: f32 = 40.0;
const BUTTON_COLUMN_WIDTH: f32 = 320.0;
const SUB_PANEL_WIDTH: f32 = 520.0;
const PAUSE_PANEL_WIDTH: f32 = 420.0;

fn sync_main_menu_page(
    game_state: Res<GameState>,
    mut page: ResMut<MainMenuPage>,
) {
    if game_state.is_changed() && game_state.0 == Game_State::MainMenu {
        *page = MainMenuPage::Title;
    }
}

fn sync_pause_menu_page(
    game_state: Res<GameState>,
    mut page: ResMut<PauseMenuPage>,
) {
    if game_state.is_changed() && game_state.0 == Game_State::Paused {
        *page = PauseMenuPage::Main;
    }
}

fn spawn_main_menu_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    page: Res<MainMenuPage>,
    menu_camera: Res<MenuSceneCamera>,
    existing: Query<(Entity, &MainMenuRoot)>,
    children: Query<&Children>,
) {
    if game_state.0 != Game_State::MainMenu {
        for (entity, _) in existing.iter() {
            despawn_recursive(&mut commands, entity, &children);
        }
        return;
    }

    if let Ok((entity, root)) = existing.single() {
        if root.0 == *page {
            return;
        }
        despawn_recursive(&mut commands, entity, &children);
    }

    // The menu UI is rendered by the dedicated 3D stage camera, so target it
    // explicitly — the gameplay world camera is disabled while we're here. The
    // root is transparent so the animated 3D cast shows through behind the UI.
    let root = commands
        .spawn((
            menu_scene_overlay(),
            UiTargetCamera(menu_camera.0),
            MainMenuRoot(*page),
        ))
        .id();

    // Backdrop layers, spawned first so foreground content renders on top: a
    // soft centre bloom + framing scrims for text legibility over the cast,
    // then a footer strip.
    commands.entity(root).with_children(|bg| {
        bg.spawn(scene_glow());
        bg.spawn(scene_vignette());
        bg.spawn(top_scrim());
        bg.spawn(bottom_scrim());
        spawn_scene_footer(bg);
    });

    match *page {
        MainMenuPage::Title => spawn_title_page(&mut commands, root),
        MainMenuPage::Load => spawn_load_page(&mut commands, root),
        MainMenuPage::Settings => {
            spawn_settings_page(&mut commands, root, /* is_pause */ false)
        }
    }
}

/// Bottom strip shown on every main-menu page: version on the left, a control
/// hint on the right.
fn spawn_scene_footer(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(spacing::LG),
            left: Val::Px(spacing::XL),
            right: Val::Px(spacing::XL),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(concat!("Seirei Kuni  v", env!("CARGO_PKG_VERSION"))),
                TextFont {
                    font_size: font_size::SMALL,
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));
            row.spawn((
                Text::new("Esc — back"),
                TextFont {
                    font_size: font_size::SMALL,
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));
        });
}

fn spawn_title_page(commands: &mut Commands, root: Entity) {
    commands.entity(root).with_children(|parent| {
        // Fill the screen and push the wordmark to the top and the buttons to
        // the bottom, leaving the animated 3D cast on show in between.
        parent
            .spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect {
                    top: Val::Px(64.0),
                    bottom: Val::Px(72.0),
                    ..UiRect::all(Val::Px(spacing::XL))
                },
                ..default()
            })
            .with_children(|col| {
                // --- Top: wordmark + tagline ---
                col.spawn(Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(spacing::SM),
                    ..default()
                })
                .with_children(|head| {
                    head.spawn((
                        Text::new("SEIREI KUNI"),
                        TextFont {
                            font_size: font_size::TITLE,
                            ..default()
                        },
                        TextColor(palette::BRAND),
                        TitlePulse,
                    ));
                    head.spawn((
                        Text::new("A journey through spirits and steel"),
                        TextFont {
                            font_size: font_size::BODY,
                            ..default()
                        },
                        TextColor(palette::TEXT_SECONDARY),
                    ));
                });

                // --- Bottom: buttons in one fixed-width, stretched column so
                // they line up regardless of label length. ---
                col.spawn(Node {
                    width: Val::Px(BUTTON_COLUMN_WIDTH),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    row_gap: Val::Px(spacing::MD),
                    ..default()
                })
                .with_children(|menu| {
                    spawn_hero_button(menu, "New Game", MenuButtonAction::StartGame);
                    spawn_hero_button(menu, "Load Game", MenuButtonAction::OpenLoadPage);
                    spawn_hero_button(menu, "Settings", MenuButtonAction::OpenSettingsPage);
                    spawn_hero_button(menu, "Quit", MenuButtonAction::QuitGame);
                });
            });
    });
}

/// Marks the title wordmark so [`animate_title`] can pulse its colour.
#[derive(Component)]
struct TitlePulse;

/// Gently breathe the title between the brand gold and a brighter tint so the
/// screen feels alive without being distracting.
fn animate_title(time: Res<Time>, mut titles: Query<&mut TextColor, With<TitlePulse>>) {
    // ~0.18 Hz sine, eased into [0, 1].
    let t = 0.5 + 0.5 * (time.elapsed_secs() * 1.1).sin();
    let color = palette::BRAND.mix(&palette::BRAND_BRIGHT, t);
    for mut text_color in &mut titles {
        text_color.0 = color;
    }
}

fn spawn_load_page(commands: &mut Commands, root: Entity) {
    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(SUB_PANEL_WIDTH)).with_children(|col| {
            col.spawn((
                Text::new("Load Game"),
                TextFont {
                    font_size: 36.0,
                    ..default()
                },
                TextColor(palette::TEXT_HEADING),
                Node {
                    margin: UiRect::bottom(Val::Px(spacing::SM)),
                    ..default()
                },
            ));
            col.spawn(label_text(
                "Pick a save slot to resume your journey.",
            ));

            for (slot, action) in [
                (SaveSlot::Slot1, MenuButtonAction::LoadSlot1),
                (SaveSlot::Slot2, MenuButtonAction::LoadSlot2),
                (SaveSlot::Slot3, MenuButtonAction::LoadSlot3),
            ] {
                spawn_load_slot_button(col, slot, action);
            }

            col.spawn(Node {
                height: Val::Px(spacing::SM),
                ..default()
            });

            spawn_hero_button(col, "Back", MenuButtonAction::BackToTitle);
        });
    });
}

fn spawn_settings_page(commands: &mut Commands, root: Entity, is_pause: bool) {
    let back_action = if is_pause {
        MenuButtonAction::PauseBack
    } else {
        MenuButtonAction::BackToTitle
    };

    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(SUB_PANEL_WIDTH)).with_children(|col| {
            col.spawn((
                Text::new("Settings"),
                TextFont {
                    font_size: 36.0,
                    ..default()
                },
                TextColor(palette::TEXT_HEADING),
                Node {
                    margin: UiRect::bottom(Val::Px(spacing::SM)),
                    ..default()
                },
            ));

            col.spawn(label_text("Saving"));
            col.spawn((
                Button::default(),
                button_node(ROW_BTN),
                button_visual(),
                MenuButtonAction::ToggleAutosave,
            ))
            .with_children(|btn| {
                btn.spawn((button_text("Autosave: ..."), AutosaveStatusText));
            });

            col.spawn((
                label_text("Performance"),
                Node {
                    margin: UiRect::top(Val::Px(spacing::SM)),
                    ..default()
                },
            ));

            for toggle in GRAPHICS_TOGGLES {
                col.spawn((
                    Button::default(),
                    button_node(TOGGLE_BTN),
                    button_visual(),
                    MenuButtonAction::ToggleGraphics(toggle),
                ))
                .with_children(|btn| {
                    btn.spawn((button_text("..."), GraphicsToggleText(toggle)));
                });
            }

            col.spawn(Node {
                height: Val::Px(spacing::SM),
                ..default()
            });

            spawn_hero_button(col, "Back", back_action);
        });
    });
}

fn spawn_pause_menu_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    page: Res<PauseMenuPage>,
    party: Res<SelectedParty>,
    main_menu_root: Query<Entity, With<MainMenuRoot>>,
    existing: Query<(Entity, &PauseMenuRoot)>,
    children: Query<&Children>,
) {
    if game_state.0 != Game_State::Paused || !main_menu_root.is_empty() {
        for (entity, _) in existing.iter() {
            despawn_recursive(&mut commands, entity, &children);
        }
        return;
    }

    if let Ok((entity, root)) = existing.single() {
        // The Party page also rebuilds when the roster reorders (e.g. after a
        // "Make Leader"), so the leader marker tracks reality.
        let stale = *page == PauseMenuPage::Party && party.is_changed();
        if root.0 == *page && !stale {
            return;
        }
        despawn_recursive(&mut commands, entity, &children);
    }

    let root = commands.spawn((overlay_root(), PauseMenuRoot(*page))).id();

    match *page {
        PauseMenuPage::Main => spawn_pause_main_page(&mut commands, root),
        PauseMenuPage::Settings => spawn_settings_page(&mut commands, root, /* is_pause */ true),
        PauseMenuPage::Party => spawn_pause_party_page(&mut commands, root, &party),
    }
}

fn spawn_pause_main_page(commands: &mut Commands, root: Entity) {
    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(PAUSE_PANEL_WIDTH)).with_children(|col| {
            col.spawn((
                Text::new("Paused"),
                TextFont {
                    font_size: 36.0,
                    ..default()
                },
                TextColor(palette::TEXT_HEADING),
                Node {
                    margin: UiRect::bottom(Val::Px(spacing::SM)),
                    ..default()
                },
            ));

            spawn_hero_button(col, "Resume", MenuButtonAction::ResumeGame);
            spawn_hero_button(col, "Party", MenuButtonAction::PauseOpenParty);
            spawn_hero_button(col, "Settings", MenuButtonAction::PauseOpenSettings);
            spawn_hero_button(col, "Return to Title", MenuButtonAction::ReturnToTitle);
            spawn_hero_button(col, "Quit", MenuButtonAction::QuitGame);
        });
    });
}

/// Pause "Party" page: lists the roster in order. Element 0 is the leader (the
/// overworld avatar); every other member gets a "Make Leader" button that
/// promotes them. The list is read straight from [`SelectedParty`] so it always
/// reflects the live order.
fn spawn_pause_party_page(commands: &mut Commands, root: Entity, party: &SelectedParty) {
    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(PAUSE_PANEL_WIDTH)).with_children(|col| {
            col.spawn((
                Text::new("Party"),
                TextFont {
                    font_size: 36.0,
                    ..default()
                },
                TextColor(palette::TEXT_HEADING),
                Node {
                    margin: UiRect::bottom(Val::Px(spacing::SM)),
                    ..default()
                },
            ));

            for (idx, kind) in party.0.iter().copied().enumerate() {
                if idx == 0 {
                    col.spawn(label_text(&format!("{}  (leader)", kind.display_name())));
                } else {
                    spawn_hero_button(
                        col,
                        &format!("Make {} leader", kind.display_name()),
                        MenuButtonAction::MakeLeader(kind),
                    );
                }
            }

            col.spawn(Node {
                height: Val::Px(spacing::SM),
                ..default()
            });
            spawn_hero_button(col, "Back", MenuButtonAction::PauseBack);
        });
    });
}

/// Root of the defeat overlay, shown while in [`Game_State::GameOver`].
#[derive(Component)]
struct GameOverRoot;

/// Root of the victory / closing overlay, shown in [`Game_State::Victory`].
#[derive(Component)]
struct VictoryRoot;

/// Full-screen defeat panel. Spawned once on entering `GameOver`, torn down on
/// leaving. Rendered by the (still-active) gameplay camera over the frozen
/// battlefield.
fn spawn_game_over_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    existing: Query<Entity, With<GameOverRoot>>,
    children: Query<&Children>,
) {
    if game_state.0 != Game_State::GameOver {
        for entity in existing.iter() {
            despawn_recursive(&mut commands, entity, &children);
        }
        return;
    }
    if !existing.is_empty() {
        return;
    }

    let root = commands.spawn((overlay_root(), GameOverRoot)).id();
    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(PAUSE_PANEL_WIDTH)).with_children(|col| {
            col.spawn((
                Text::new("The Party Has Fallen"),
                TextFont {
                    font_size: 40.0,
                    ..default()
                },
                TextColor(palette::BRAND),
                Node {
                    margin: UiRect::bottom(Val::Px(spacing::SM)),
                    ..default()
                },
            ));
            col.spawn(label_text(
                "Your leader's thread is cut, and the defilement spreads unchecked.",
            ));
            col.spawn(Node {
                height: Val::Px(spacing::SM),
                ..default()
            });
            spawn_hero_button(col, "Try Again", MenuButtonAction::RestartRun);
            spawn_hero_button(col, "Return to Title", MenuButtonAction::ReturnToTitle);
            spawn_hero_button(col, "Quit", MenuButtonAction::QuitGame);
        });
    });
}

/// Full-screen victory panel shown when the final boss falls. Acts as the run's
/// closing screen.
fn spawn_victory_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    existing: Query<Entity, With<VictoryRoot>>,
    children: Query<&Children>,
) {
    if game_state.0 != Game_State::Victory {
        for entity in existing.iter() {
            despawn_recursive(&mut commands, entity, &children);
        }
        return;
    }
    if !existing.is_empty() {
        return;
    }

    let root = commands.spawn((overlay_root(), VictoryRoot)).id();
    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(SUB_PANEL_WIDTH)).with_children(|col| {
            col.spawn((
                Text::new("The Land Is Cleansed"),
                TextFont {
                    font_size: 44.0,
                    ..default()
                },
                TextColor(palette::BRAND),
                Node {
                    margin: UiRect::bottom(Val::Px(spacing::SM)),
                    ..default()
                },
            ));
            col.spawn(label_text(
                "The Gashadokuro crumbles to dust. The shrine bells fall silent at last,",
            ));
            col.spawn(label_text(
                "and the kegare lifts from the eastern path. Your name passes into song.",
            ));
            col.spawn(Node {
                height: Val::Px(spacing::MD),
                ..default()
            });
            col.spawn((
                Text::new("— Seirei Kuni —"),
                TextFont {
                    font_size: font_size::BODY,
                    ..default()
                },
                TextColor(palette::TEXT_SECONDARY),
                Node {
                    margin: UiRect::bottom(Val::Px(spacing::MD)),
                    ..default()
                },
            ));
            spawn_hero_button(col, "Return to Title", MenuButtonAction::ReturnToTitle);
            spawn_hero_button(col, "Quit", MenuButtonAction::QuitGame);
        });
    });
}

fn spawn_hero_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: MenuButtonAction,
) {
    parent
        .spawn((
            Button::default(),
            button_node(HERO_BTN),
            button_visual(),
            action,
        ))
        .with_children(|btn| {
            btn.spawn(button_text_lg(label));
        });
}

fn spawn_load_slot_button(
    parent: &mut ChildSpawnerCommands,
    slot: SaveSlot,
    action: MenuButtonAction,
) {
    parent
        .spawn((
            Button::default(),
            Node {
                height: Val::Px(56.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(spacing::LG)),
                border: UiRect::all(Val::Px(1.5)),
                ..default()
            },
            button_visual(),
            action,
        ))
        .with_children(|btn| {
            btn.spawn(button_text_lg(slot_label(slot)));
            btn.spawn((
                Text::new("..."),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(palette::TEXT_SECONDARY),
                LoadSlotStatusText(slot),
            ));
        });
}

fn slot_label(slot: SaveSlot) -> &'static str {
    match slot {
        SaveSlot::Slot1 => "Slot 1",
        SaveSlot::Slot2 => "Slot 2",
        SaveSlot::Slot3 => "Slot 3",
        SaveSlot::Auto => "Auto",
    }
}

fn slot_status_text(slot: SaveSlot) -> &'static str {
    let path = save_slot_path(slot);
    if Path::new(&path).exists() {
        "Saved"
    } else {
        "Empty"
    }
}

fn save_slot_path(slot: SaveSlot) -> String {
    let file_name = match slot {
        SaveSlot::Auto => "auto.ron",
        SaveSlot::Slot1 => "slot_1.ron",
        SaveSlot::Slot2 => "slot_2.ron",
        SaveSlot::Slot3 => "slot_3.ron",
    };
    format!("saves/{}", file_name)
}

fn update_load_slot_status(mut labels: Query<(&mut Text, &mut TextColor, &LoadSlotStatusText)>) {
    for (mut text, mut color, marker) in &mut labels {
        let status = slot_status_text(marker.0);
        if text.0 != status {
            text.0 = status.to_string();
        }
        color.0 = if status == "Saved" {
            palette::ACCENT_SUCCESS
        } else {
            palette::TEXT_DIM
        };
    }
}

fn toggle_pause_state(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    mut resume_state: ResMut<ResumeState>,
    mut pause_page: ResMut<PauseMenuPage>,
    mut main_page: ResMut<MainMenuPage>,
) {
    if !input.just_pressed(KeyCode::Escape) {
        return;
    }

    match game_state.0 {
        Game_State::MainMenu => {
            // ESC backs out of sub-pages on the title screen.
            if *main_page != MainMenuPage::Title {
                *main_page = MainMenuPage::Title;
            }
        }
        Game_State::Paused => {
            if *pause_page != PauseMenuPage::Main {
                *pause_page = PauseMenuPage::Main;
            } else {
                game_state.0 = resume_state.0;
            }
        }
        // Terminal run states own the whole screen; Esc must not peel back into
        // a paused world that no longer exists.
        Game_State::GameOver | Game_State::Victory => {}
        // Esc closes a read-only overlay straight back to exploration rather
        // than stacking the pause menu on top of it.
        Game_State::CharacterSheet | Game_State::QuestLog => {
            game_state.0 = Game_State::Exploring;
        }
        other => {
            resume_state.0 = other;
            game_state.0 = Game_State::Paused;
        }
    }
}

fn handle_menu_actions(
    mut exit: MessageWriter<AppExit>,
    mut game_state: ResMut<GameState>,
    mut resume_state: ResMut<ResumeState>,
    mut autosave: ResMut<AutoSaveSettings>,
    mut graphics: ResMut<GraphicsSettings>,
    mut save_requests: ResMut<Messages<SaveRequest>>,
    mut main_page: ResMut<MainMenuPage>,
    mut pause_page: ResMut<PauseMenuPage>,
    mut mouse_input: ResMut<ButtonInput<MouseButton>>,
    mut key_input: ResMut<ButtonInput<KeyCode>>,
    mut leader_requests: MessageWriter<SetLeaderRequest>,
    mut interactions: Query<(&Interaction, &MenuButtonAction), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, action) in &mut interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }

        match action {
            MenuButtonAction::StartGame => {
                // New runs go through the party-selection screen first; the
                // roster the player picks there drives who is spawned.
                game_state.0 = Game_State::PartySelection;
                resume_state.0 = Game_State::Exploring;
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::QuitGame => {
                exit.write(AppExit::Success);
            }
            MenuButtonAction::OpenLoadPage => {
                *main_page = MainMenuPage::Load;
            }
            MenuButtonAction::OpenSettingsPage => {
                *main_page = MainMenuPage::Settings;
            }
            MenuButtonAction::BackToTitle => {
                *main_page = MainMenuPage::Title;
            }
            MenuButtonAction::ResumeGame => {
                game_state.0 = resume_state.0;
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::ReturnToTitle => {
                game_state.0 = Game_State::MainMenu;
                *main_page = MainMenuPage::Title;
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::RestartRun => {
                game_state.0 = Game_State::PartySelection;
                resume_state.0 = Game_State::Exploring;
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::PauseOpenSettings => {
                *pause_page = PauseMenuPage::Settings;
            }
            MenuButtonAction::PauseOpenParty => {
                *pause_page = PauseMenuPage::Party;
            }
            MenuButtonAction::MakeLeader(kind) => {
                leader_requests.write(SetLeaderRequest { kind: *kind });
            }
            MenuButtonAction::PauseBack => {
                *pause_page = PauseMenuPage::Main;
            }
            MenuButtonAction::LoadSlot1 => {
                save_requests.write(SaveRequest {
                    action: SaveAction::Load,
                    slot: SaveSlot::Slot1,
                });
                game_state.0 = Game_State::Exploring;
                resume_state.0 = Game_State::Exploring;
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::LoadSlot2 => {
                save_requests.write(SaveRequest {
                    action: SaveAction::Load,
                    slot: SaveSlot::Slot2,
                });
                game_state.0 = Game_State::Exploring;
                resume_state.0 = Game_State::Exploring;
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::LoadSlot3 => {
                save_requests.write(SaveRequest {
                    action: SaveAction::Load,
                    slot: SaveSlot::Slot3,
                });
                game_state.0 = Game_State::Exploring;
                resume_state.0 = Game_State::Exploring;
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::ToggleAutosave => {
                autosave.enabled = !autosave.enabled;
                autosave.timer.reset();
            }
            MenuButtonAction::ToggleGraphics(toggle) => {
                graphics.toggle(*toggle);
            }
        }
    }
}

fn update_graphics_toggle_text(
    graphics: Res<GraphicsSettings>,
    mut labels: Query<(&mut Text, &GraphicsToggleText)>,
) {
    for (mut text, marker) in &mut labels {
        let desired = graphics.label(marker.0);
        if text.0 != desired {
            text.0 = desired.to_string();
        }
    }
}

fn update_autosave_status_text(
    autosave: Res<AutoSaveSettings>,
    mut labels: Query<&mut Text, With<AutosaveStatusText>>,
) {
    let label = if autosave.enabled { "Autosave: On" } else { "Autosave: Off" };
    for mut text in &mut labels {
        text.0 = label.to_string();
    }
}

fn despawn_recursive(commands: &mut Commands, entity: Entity, children: &Query<&Children>) {
    if let Ok(child_entities) = children.get(entity) {
        for child in child_entities.iter() {
            despawn_recursive(commands, child, children);
        }
    }
    commands.entity(entity).despawn();
}
