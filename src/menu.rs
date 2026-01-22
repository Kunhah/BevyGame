use bevy::app::AppExit;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::core::{GameState, Game_State};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ResumeState>()
            .add_systems(Update, spawn_main_menu_ui)
            .add_systems(Update, spawn_pause_menu_ui)
            .add_systems(Update, toggle_pause_state)
            .add_systems(Update, update_button_interaction_visuals)
            .add_systems(Update, handle_menu_actions);
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
}

fn spawn_main_menu_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    existing: Query<Entity, With<MainMenuRoot>>,
) {
    if game_state.0 != Game_State::MainMenu || !existing.is_empty() {
        return;
    }

    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(16.0),
                position_type: PositionType::Absolute,
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.08, 0.92)),
            MainMenuRoot,
        ))
        .id();

    commands.entity(root).with_children(|parent| {
        parent
            .spawn((
                Node {
                    width: Val::Px(520.0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Stretch,
                    row_gap: Val::Px(12.0),
                    padding: UiRect::all(Val::Px(24.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.08, 0.1, 0.16, 0.85)),
                BorderRadius::all(Val::Px(14.0)),
            ))
            .with_children(|col| {
                col.spawn((
                    Text::new("Seirei Kuni"),
                    TextFont {
                        font_size: 48.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.94, 0.98)),
                ));

                col.spawn((
                    Text::new("Choose your path and step into the world."),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.75, 0.8, 0.88)),
                ));

                col
                    .spawn((
                        Button::default(),
                        Node {
                            height: Val::Px(52.0),
                            display: Display::Flex,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            padding: UiRect::all(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(1.5)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.14, 0.19, 0.28, 0.95)),
                        BorderRadius::all(Val::Px(10.0)),
                        BorderColor::all(Color::srgba(0.26, 0.34, 0.48, 1.0)),
                        MenuButtonAction::StartGame,
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("Start Journey"),
                            TextFont {
                                font_size: 22.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.9, 0.92, 0.96)),
                        ));
                    });

                col
                    .spawn((
                        Button::default(),
                        Node {
                            height: Val::Px(52.0),
                            display: Display::Flex,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            padding: UiRect::all(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(1.5)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.14, 0.19, 0.28, 0.95)),
                        BorderRadius::all(Val::Px(10.0)),
                        BorderColor::all(Color::srgba(0.26, 0.34, 0.48, 1.0)),
                        MenuButtonAction::QuitGame,
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("Quit"),
                            TextFont {
                                font_size: 22.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.9, 0.92, 0.96)),
                        ));
                    });
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

    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                position_type: PositionType::Absolute,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.7)),
            PauseMenuRoot,
        ))
        .id();

    commands.entity(root).with_children(|parent| {
        parent
            .spawn((
                Node {
                    width: Val::Px(420.0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Stretch,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::all(Val::Px(18.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.07, 0.1, 0.15, 0.9)),
                BorderRadius::all(Val::Px(12.0)),
            ))
            .with_children(|col| {
                col.spawn((
                    Text::new("Paused"),
                    TextFont {
                        font_size: 36.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.94, 0.98)),
                ));

                col
                    .spawn((
                        Button::default(),
                        Node {
                            height: Val::Px(52.0),
                            display: Display::Flex,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            padding: UiRect::all(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(1.5)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.14, 0.19, 0.28, 0.95)),
                        BorderRadius::all(Val::Px(10.0)),
                        BorderColor::all(Color::srgba(0.26, 0.34, 0.48, 1.0)),
                        MenuButtonAction::ResumeGame,
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("Resume"),
                            TextFont {
                                font_size: 22.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.9, 0.92, 0.96)),
                        ));
                    });

                col
                    .spawn((
                        Button::default(),
                        Node {
                            height: Val::Px(52.0),
                            display: Display::Flex,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            padding: UiRect::all(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(1.5)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.14, 0.19, 0.28, 0.95)),
                        BorderRadius::all(Val::Px(10.0)),
                        BorderColor::all(Color::srgba(0.26, 0.34, 0.48, 1.0)),
                        MenuButtonAction::ReturnToTitle,
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("Return to Title"),
                            TextFont {
                                font_size: 22.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.9, 0.92, 0.96)),
                        ));
                    });

                col
                    .spawn((
                        Button::default(),
                        Node {
                            height: Val::Px(52.0),
                            display: Display::Flex,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            padding: UiRect::all(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(1.5)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.14, 0.19, 0.28, 0.95)),
                        BorderRadius::all(Val::Px(10.0)),
                        BorderColor::all(Color::srgba(0.26, 0.34, 0.48, 1.0)),
                        MenuButtonAction::QuitGame,
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("Quit"),
                            TextFont {
                                font_size: 22.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.9, 0.92, 0.96)),
                        ));
                    });
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

fn update_button_interaction_visuals(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        match *interaction {
            Interaction::Pressed => {
                bg.0 = Color::srgba(0.2, 0.26, 0.4, 1.0);
                set_border_color(&mut border, Color::srgba(0.34, 0.44, 0.62, 1.0));
            }
            Interaction::Hovered => {
                bg.0 = Color::srgba(0.16, 0.22, 0.32, 1.0);
                set_border_color(&mut border, Color::srgba(0.3, 0.4, 0.56, 1.0));
            }
            Interaction::None => {
                bg.0 = Color::srgba(0.14, 0.19, 0.28, 0.95);
                set_border_color(&mut border, Color::srgba(0.26, 0.34, 0.48, 1.0));
            }
        }
    }
}

fn set_border_color(border: &mut BorderColor, color: Color) {
    border.top = color;
    border.right = color;
    border.bottom = color;
    border.left = color;
}

fn handle_menu_actions(
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    mut game_state: ResMut<GameState>,
    mut resume_state: ResMut<ResumeState>,
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
        }
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
