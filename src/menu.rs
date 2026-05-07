use bevy::app::AppExit;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::core::{GameState, Game_State};
use crate::save::{AutoSaveSettings, SaveAction, SaveRequest, SaveSlot};
use crate::settings::{GraphicsSettings, GraphicsToggle, GRAPHICS_TOGGLES};
use crate::ui_style::{
    button_node, button_text, button_text_lg, button_visual, label_text, overlay_root, panel,
    palette, spacing, title_text,
};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ResumeState>()
            .add_systems(Update, spawn_main_menu_ui)
            .add_systems(Update, spawn_pause_menu_ui)
            .add_systems(Update, toggle_pause_state)
            .add_systems(Update, handle_menu_actions)
            .add_systems(Update, update_autosave_status_text)
            .add_systems(Update, update_graphics_toggle_text);
    }
}

#[derive(Resource)]
pub struct ResumeState(pub Game_State);

impl Default for ResumeState {
    fn default() -> Self {
        Self(Game_State::Exploring)
    }
}

#[derive(Component)]
struct MainMenuRoot;

#[derive(Component)]
struct PauseMenuRoot;

#[derive(Component, Clone, Copy)]
enum MenuButtonAction {
    StartGame,
    QuitGame,
    ResumeGame,
    ReturnToTitle,
    SaveSlot1,
    SaveSlot2,
    SaveSlot3,
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

const HERO_BTN: f32 = 52.0;
const ROW_BTN: f32 = 44.0;
const TOGGLE_BTN: f32 = 40.0;

fn spawn_section_label(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        label_text(text),
        Node {
            margin: UiRect::top(Val::Px(spacing::SM)),
            ..default()
        },
    ));
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

fn spawn_row_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: MenuButtonAction,
) {
    parent
        .spawn((
            Button::default(),
            button_node(ROW_BTN),
            button_visual(),
            action,
        ))
        .with_children(|btn| {
            btn.spawn(button_text(label));
        });
}

fn spawn_graphics_settings_section(parent: &mut ChildSpawnerCommands) {
    spawn_section_label(parent, "Performance");

    for toggle in GRAPHICS_TOGGLES {
        parent
            .spawn((
                Button::default(),
                button_node(TOGGLE_BTN),
                button_visual(),
                MenuButtonAction::ToggleGraphics(toggle),
            ))
            .with_children(|btn| {
                btn.spawn((button_text("..."), GraphicsToggleText(toggle)));
            });
    }
}

fn spawn_main_menu_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    existing: Query<Entity, With<MainMenuRoot>>,
) {
    if game_state.0 != Game_State::MainMenu || !existing.is_empty() {
        return;
    }

    let root = commands.spawn((overlay_root(), MainMenuRoot)).id();

    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(520.0)).with_children(|col| {
            col.spawn(title_text("Seirei Kuni"));
            col.spawn((
                Text::new("Choose your path and step into the world."),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(palette::TEXT_SECONDARY),
                Node {
                    margin: UiRect::bottom(Val::Px(spacing::SM)),
                    ..default()
                },
            ));

            spawn_hero_button(col, "Start Journey", MenuButtonAction::StartGame);

            spawn_section_label(col, "Save / Load");
            spawn_row_button(col, "Save Slot 1", MenuButtonAction::SaveSlot1);
            spawn_row_button(col, "Save Slot 2", MenuButtonAction::SaveSlot2);
            spawn_row_button(col, "Save Slot 3", MenuButtonAction::SaveSlot3);
            spawn_row_button(col, "Load Slot 1", MenuButtonAction::LoadSlot1);
            spawn_row_button(col, "Load Slot 2", MenuButtonAction::LoadSlot2);
            spawn_row_button(col, "Load Slot 3", MenuButtonAction::LoadSlot3);

            spawn_section_label(col, "Autosave");
            col.spawn((
                Button::default(),
                button_node(ROW_BTN),
                button_visual(),
                MenuButtonAction::ToggleAutosave,
            ))
            .with_children(|btn| {
                btn.spawn((button_text("Autosave: ..."), AutosaveStatusText));
            });

            spawn_graphics_settings_section(col);

            spawn_hero_button(col, "Quit", MenuButtonAction::QuitGame);
        });
    });
}

fn spawn_pause_menu_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    existing: Query<Entity, With<PauseMenuRoot>>,
    main_menu_root: Query<Entity, With<MainMenuRoot>>,
) {
    if game_state.0 != Game_State::Paused || !existing.is_empty() || !main_menu_root.is_empty() {
        return;
    }

    let root = commands.spawn((overlay_root(), PauseMenuRoot)).id();

    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(420.0)).with_children(|col| {
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
            spawn_hero_button(col, "Return to Title", MenuButtonAction::ReturnToTitle);

            spawn_graphics_settings_section(col);

            spawn_hero_button(col, "Quit", MenuButtonAction::QuitGame);
        });
    });
}

fn toggle_pause_state(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    mut resume_state: ResMut<ResumeState>,
    pause_menu: Query<Entity, With<PauseMenuRoot>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    if !input.just_pressed(KeyCode::Escape) {
        return;
    }

    match game_state.0 {
        Game_State::MainMenu => {}
        Game_State::Paused => {
            game_state.0 = resume_state.0;
            despawn_menu(&mut commands, &pause_menu, &children);
        }
        other => {
            resume_state.0 = other;
            game_state.0 = Game_State::Paused;
        }
    }
}

fn handle_menu_actions(
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    mut game_state: ResMut<GameState>,
    mut resume_state: ResMut<ResumeState>,
    mut autosave: ResMut<AutoSaveSettings>,
    mut graphics: ResMut<GraphicsSettings>,
    mut save_requests: ResMut<Messages<SaveRequest>>,
    main_menu: Query<Entity, With<MainMenuRoot>>,
    pause_menu: Query<Entity, With<PauseMenuRoot>>,
    children: Query<&Children>,
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
                game_state.0 = Game_State::Exploring;
                resume_state.0 = Game_State::Exploring;
                despawn_menu(&mut commands, &main_menu, &children);
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::QuitGame => {
                exit.write(AppExit::Success);
            }
            MenuButtonAction::ResumeGame => {
                game_state.0 = resume_state.0;
                despawn_menu(&mut commands, &pause_menu, &children);
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::ReturnToTitle => {
                game_state.0 = Game_State::MainMenu;
                despawn_menu(&mut commands, &pause_menu, &children);
                mouse_input.reset_all();
                key_input.clear();
            }
            MenuButtonAction::SaveSlot1 => {
                save_requests.write(SaveRequest {
                    action: SaveAction::Save,
                    slot: SaveSlot::Slot1,
                });
            }
            MenuButtonAction::SaveSlot2 => {
                save_requests.write(SaveRequest {
                    action: SaveAction::Save,
                    slot: SaveSlot::Slot2,
                });
            }
            MenuButtonAction::SaveSlot3 => {
                save_requests.write(SaveRequest {
                    action: SaveAction::Save,
                    slot: SaveSlot::Slot3,
                });
            }
            MenuButtonAction::LoadSlot1 => {
                save_requests.write(SaveRequest {
                    action: SaveAction::Load,
                    slot: SaveSlot::Slot1,
                });
            }
            MenuButtonAction::LoadSlot2 => {
                save_requests.write(SaveRequest {
                    action: SaveAction::Load,
                    slot: SaveSlot::Slot2,
                });
            }
            MenuButtonAction::LoadSlot3 => {
                save_requests.write(SaveRequest {
                    action: SaveAction::Load,
                    slot: SaveSlot::Slot3,
                });
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

fn despawn_menu<T: Component>(
    commands: &mut Commands,
    roots: &Query<Entity, With<T>>,
    children: &Query<&Children>,
) {
    for entity in roots.iter() {
        despawn_recursive(commands, entity, children);
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
