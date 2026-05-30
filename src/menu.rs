use std::path::Path;

use bevy::app::AppExit;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::core::{GameState, Game_State};
use crate::save::{AutoSaveSettings, SaveAction, SaveRequest, SaveSlot};
use crate::settings::{GraphicsSettings, GraphicsToggle, GRAPHICS_TOGGLES};
use crate::ui_style::{
    button_node, button_text, button_text_lg, button_visual, label_text, overlay_root, palette,
    panel, spacing, title_text,
};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ResumeState>()
            .init_resource::<MainMenuPage>()
            .init_resource::<PauseMenuPage>()
            .add_systems(Update, sync_main_menu_page)
            .add_systems(Update, sync_pause_menu_page)
            .add_systems(Update, spawn_main_menu_ui)
            .add_systems(Update, spawn_pause_menu_ui)
            .add_systems(Update, toggle_pause_state)
            .add_systems(Update, handle_menu_actions)
            .add_systems(Update, update_autosave_status_text)
            .add_systems(Update, update_graphics_toggle_text)
            .add_systems(Update, update_load_slot_status);
    }
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
    PauseOpenSettings,
    PauseBack,
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
const TITLE_PANEL_WIDTH: f32 = 460.0;
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

    let root = commands.spawn((overlay_root(), MainMenuRoot(*page))).id();

    match *page {
        MainMenuPage::Title => spawn_title_page(&mut commands, root),
        MainMenuPage::Load => spawn_load_page(&mut commands, root),
        MainMenuPage::Settings => {
            spawn_settings_page(&mut commands, root, /* is_pause */ false)
        }
    }
}

fn spawn_title_page(commands: &mut Commands, root: Entity) {
    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(TITLE_PANEL_WIDTH)).with_children(|col| {
            col.spawn(title_text("Seirei Kuni"));
            col.spawn((
                Text::new("A journey through spirits and steel."),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(palette::TEXT_SECONDARY),
                Node {
                    margin: UiRect::bottom(Val::Px(spacing::LG)),
                    ..default()
                },
            ));

            spawn_hero_button(col, "New Game", MenuButtonAction::StartGame);
            spawn_hero_button(col, "Load Game", MenuButtonAction::OpenLoadPage);
            spawn_hero_button(col, "Settings", MenuButtonAction::OpenSettingsPage);
            spawn_hero_button(col, "Quit", MenuButtonAction::QuitGame);
        });
    });
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
        if root.0 == *page {
            return;
        }
        despawn_recursive(&mut commands, entity, &children);
    }

    let root = commands.spawn((overlay_root(), PauseMenuRoot(*page))).id();

    match *page {
        PauseMenuPage::Main => spawn_pause_main_page(&mut commands, root),
        PauseMenuPage::Settings => spawn_settings_page(&mut commands, root, /* is_pause */ true),
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
            spawn_hero_button(col, "Settings", MenuButtonAction::PauseOpenSettings);
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
            MenuButtonAction::PauseOpenSettings => {
                *pause_page = PauseMenuPage::Settings;
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
