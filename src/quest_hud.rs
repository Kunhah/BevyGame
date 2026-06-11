//! Quest legibility & onboarding layer.
//!
//! Three jobs, none of which existed before — the main quest worked mechanically
//! but was invisible:
//!   * **Tracker** (top-left): the active quests and their objectives, always on
//!     during exploration, rebuilt whenever the [`QuestLog`] changes.
//!   * **Toasts** (top-centre): transient "Quest Added / Complete / Failed"
//!     banners, driven by diffing the log's per-quest status each change.
//!   * **Onboarding**: a persistent control-hint footer plus a contextual
//!     prompt ("X — Talk", "Space — Fight") when the player is next to an
//!     interactable or an enemy.
//!
//! Also hosts the full-screen quest-log overlay on `J`
//! ([`Game_State::QuestLog`]).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::battle::EnemyEncounter;
use crate::core::{GameState, Game_State, Player};
use crate::dialogue::CachedInteractables;
use crate::quests::{QuestLog, QuestStatus};
use crate::ui_style::{font_size, palette, radius, spacing};

pub struct QuestHudPlugin;

impl Plugin for QuestHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_quest_tracker)
            .add_systems(Startup, spawn_onboarding_ui)
            .add_systems(Update, toggle_quest_log)
            .add_systems(Update, rebuild_quest_tracker)
            .add_systems(Update, emit_quest_toasts)
            .add_systems(Update, fade_and_despawn_toasts)
            .add_systems(Update, update_onboarding_prompt)
            .add_systems(Update, sync_quest_log_overlay);
    }
}

// Quick predicate: HUD chrome shows only while wandering the world.
fn is_exploring(state: Game_State) -> bool {
    matches!(
        state,
        Game_State::Exploring
            | Game_State::Interacting
            | Game_State::MapOpen
            | Game_State::Traveling
            | Game_State::Shopping
    )
}

// ---------------------------------------------------------------------------
// Active-quest tracker (top-left)
// ---------------------------------------------------------------------------

#[derive(Component)]
struct QuestTrackerRoot;

fn spawn_quest_tracker(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(spacing::MD),
            left: Val::Px(spacing::MD),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(spacing::XS),
            padding: UiRect::all(Val::Px(spacing::MD)),
            border: UiRect::all(Val::Px(1.0)),
            max_width: Val::Px(320.0),
            border_radius: BorderRadius::all(Val::Px(radius::MD)),
            ..default()
        },
        BackgroundColor(palette::BG_PANEL),
        BorderColor::all(palette::BORDER_SUBTLE),
        Visibility::Hidden,
        QuestTrackerRoot,
    ));
}

fn rebuild_quest_tracker(
    mut commands: Commands,
    game_state: Res<GameState>,
    log: Res<QuestLog>,
    root_q: Query<Entity, With<QuestTrackerRoot>>,
    children_q: Query<&Children>,
    mut vis_q: Query<&mut Visibility, With<QuestTrackerRoot>>,
) {
    let active: Vec<&crate::quests::Quest> = log
        .quests
        .values()
        .filter(|q| q.status == QuestStatus::Active)
        .collect();

    // Visibility: hide entirely outside exploration or when there's nothing to
    // track.
    let show = is_exploring(game_state.0) && !active.is_empty();
    if let Ok(mut vis) = vis_q.single_mut() {
        let desired = if show { Visibility::Visible } else { Visibility::Hidden };
        if *vis != desired {
            *vis = desired;
        }
    }

    // Only repopulate the children when the log actually changed — cheap and
    // avoids rebuilding the tree every frame.
    if !log.is_changed() {
        return;
    }
    let Ok(root) = root_q.single() else {
        return;
    };
    // A freshly-spawned root has no `Children` component yet, so tolerate its
    // absence rather than bailing (which would leave the tracker empty forever).
    if let Ok(children) = children_q.get(root) {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    commands.entity(root).with_children(|panel| {
        panel.spawn((
            Text::new("QUESTS"),
            TextFont {
                font_size: font_size::LABEL,
                ..default()
            },
            TextColor(palette::TEXT_SECONDARY),
        ));

        // Deterministic order so the tracker doesn't reshuffle frame to frame.
        let mut active = active;
        active.sort_by_key(|q| q.id);

        for quest in active {
            panel.spawn((
                Text::new(quest.title.clone()),
                TextFont {
                    font_size: font_size::BODY,
                    ..default()
                },
                TextColor(palette::TEXT_HEADING),
                Node {
                    margin: UiRect::top(Val::Px(spacing::XS)),
                    ..default()
                },
            ));
            for obj in &quest.objectives {
                let (mark, color) = if obj.is_complete() {
                    ("✓", palette::ACCENT_SUCCESS)
                } else {
                    ("•", palette::TEXT_PRIMARY)
                };
                let line = if obj.required > 1 && !obj.is_complete() {
                    format!("{} {} ({}/{})", mark, obj.description, obj.progress, obj.required)
                } else {
                    format!("{} {}", mark, obj.description)
                };
                panel.spawn((
                    Text::new(line),
                    TextFont {
                        font_size: font_size::SMALL,
                        ..default()
                    },
                    TextColor(color),
                ));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Toasts (top-centre, transient)
// ---------------------------------------------------------------------------

#[derive(Component)]
struct ToastStack;

#[derive(Component)]
struct Toast {
    timer: Timer,
}

/// Diff the log's per-quest status each time it changes and raise a toast for
/// new quests and for completion / failure.
fn emit_quest_toasts(
    mut commands: Commands,
    log: Res<QuestLog>,
    mut seen: Local<HashMap<u32, QuestStatus>>,
    stack_q: Query<Entity, With<ToastStack>>,
) {
    if !log.is_changed() {
        return;
    }

    let mut announcements: Vec<(String, Color)> = Vec::new();
    for quest in log.quests.values() {
        match seen.get(&quest.id) {
            None => announcements.push((format!("New Quest: {}", quest.title), palette::BRAND)),
            Some(prev) if *prev != quest.status => match quest.status {
                QuestStatus::Completed => announcements
                    .push((format!("Quest Complete: {}", quest.title), palette::ACCENT_SUCCESS)),
                QuestStatus::Failed => announcements
                    .push((format!("Quest Failed: {}", quest.title), palette::ACCENT_DANGER)),
                QuestStatus::Active => {}
            },
            _ => {}
        }
    }
    // Quests can also be moved wholesale into `completed` (removed from the
    // active map); catch those too.
    seen.clear();
    for quest in log.quests.values() {
        seen.insert(quest.id, quest.status);
    }

    if announcements.is_empty() {
        return;
    }

    // Lazily create the stack container once.
    let stack = match stack_q.single() {
        Ok(e) => e,
        Err(_) => commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(72.0),
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(spacing::SM),
                    ..default()
                },
                ToastStack,
            ))
            .id(),
    };

    for (text, color) in announcements {
        commands.entity(stack).with_children(|s| {
            s.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(spacing::LG), Val::Px(spacing::SM)),
                    border: UiRect::all(Val::Px(1.5)),
                    border_radius: BorderRadius::all(Val::Px(radius::MD)),
                    ..default()
                },
                BackgroundColor(palette::BG_PANEL),
                BorderColor::all(color),
                Toast {
                    timer: Timer::from_seconds(4.0, TimerMode::Once),
                },
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(text),
                    TextFont {
                        font_size: font_size::BODY,
                        ..default()
                    },
                    TextColor(color),
                ));
            });
        });
    }
}

fn fade_and_despawn_toasts(
    mut commands: Commands,
    time: Res<Time>,
    mut toasts: Query<(Entity, &mut Toast)>,
) {
    for (entity, mut toast) in &mut toasts {
        toast.timer.tick(time.delta());
        if toast.timer.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

// ---------------------------------------------------------------------------
// Onboarding: control footer + contextual prompt
// ---------------------------------------------------------------------------

#[derive(Component)]
struct ControlFooter;

#[derive(Component)]
struct ContextPrompt;

fn spawn_onboarding_ui(mut commands: Commands) {
    // Persistent control hint, bottom-left.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(spacing::MD),
            left: Val::Px(spacing::MD),
            ..default()
        },
        Text::new("WASD move · X talk · Space fight · M map · J quests · K skills · C party"),
        TextFont {
            font_size: font_size::SMALL,
            ..default()
        },
        TextColor(palette::TEXT_DIM),
        Visibility::Hidden,
        ControlFooter,
    ));

    // Contextual action prompt, bottom-centre, just above the footer.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(56.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                display: Display::Flex,
                ..default()
            },
            Visibility::Hidden,
            ContextPrompt,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(spacing::MD), Val::Px(spacing::XS)),
                    border_radius: BorderRadius::all(Val::Px(radius::SM)),
                    ..default()
                },
                BackgroundColor(palette::BG_PANEL),
                Text::new(String::new()),
                TextFont {
                    font_size: font_size::BODY,
                    ..default()
                },
                TextColor(palette::BRAND),
            ));
        });
}

#[allow(clippy::type_complexity)]
fn update_onboarding_prompt(
    game_state: Res<GameState>,
    player_q: Query<&Transform, With<Player>>,
    interactables: Res<CachedInteractables>,
    enemy_q: Query<&Transform, With<EnemyEncounter>>,
    mut footer_q: Query<&mut Visibility, (With<ControlFooter>, Without<ContextPrompt>)>,
    mut prompt_root_q: Query<
        (&mut Visibility, &Children),
        (With<ContextPrompt>, Without<ControlFooter>),
    >,
    mut text_q: Query<&mut Text>,
) {
    let exploring = is_exploring(game_state.0);
    set_vis(&mut footer_q, exploring);

    let Ok((mut prompt_vis, children)) = prompt_root_q.single_mut() else {
        return;
    };

    // Find the nearest interactable / enemy within engage range of the player.
    let prompt = player_q.single().ok().and_then(|player_tf| {
        if !exploring {
            return None;
        }
        let p = player_tf.translation.truncate();
        let near_talk = interactables
            .0
            .iter()
            .any(|(t, _)| t.translation.truncate().distance(p) <= 40.0);
        if near_talk {
            return Some("X — Talk");
        }
        let near_enemy = enemy_q
            .iter()
            .any(|t| t.translation.truncate().distance(p) <= 40.0);
        if near_enemy {
            return Some("Space — Fight");
        }
        None
    });

    let desired = if prompt.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if *prompt_vis != desired {
        *prompt_vis = desired;
    }
    if let Some(label) = prompt {
        // The prompt's single grandchild text node carries the label.
        for child in children.iter() {
            if let Ok(mut text) = text_q.get_mut(child) {
                if text.0 != label {
                    text.0 = label.to_string();
                }
            }
        }
    }
}

fn set_vis<F: bevy::ecs::query::QueryFilter>(
    q: &mut Query<&mut Visibility, F>,
    visible: bool,
) {
    let desired = if visible { Visibility::Visible } else { Visibility::Hidden };
    for mut vis in q.iter_mut() {
        if *vis != desired {
            *vis = desired;
        }
    }
}

// ---------------------------------------------------------------------------
// Full quest-log overlay (J)
// ---------------------------------------------------------------------------

#[derive(Component)]
struct QuestLogRoot;

fn toggle_quest_log(input: Res<ButtonInput<KeyCode>>, mut game_state: ResMut<GameState>) {
    if !input.just_pressed(KeyCode::KeyJ) {
        return;
    }
    game_state.0 = match game_state.0 {
        Game_State::Exploring => Game_State::QuestLog,
        Game_State::QuestLog => Game_State::Exploring,
        other => other,
    };
}

fn sync_quest_log_overlay(
    mut commands: Commands,
    game_state: Res<GameState>,
    log: Res<QuestLog>,
    existing: Query<Entity, With<QuestLogRoot>>,
) {
    if game_state.0 != Game_State::QuestLog {
        for e in existing.iter() {
            commands.entity(e).despawn();
        }
        return;
    }
    if !existing.is_empty() {
        return;
    }

    let mut quests: Vec<&crate::quests::Quest> = log.quests.values().collect();
    quests.sort_by_key(|q| (status_rank(q.status), q.id));

    commands
        .spawn((
            crate::ui_style::overlay_root(),
            QuestLogRoot,
        ))
        .with_children(|root| {
            root.spawn(crate::ui_style::panel(640.0)).with_children(|col| {
                col.spawn((
                    Text::new("Quest Log"),
                    TextFont {
                        font_size: font_size::HEADING,
                        ..default()
                    },
                    TextColor(palette::TEXT_HEADING),
                    Node {
                        margin: UiRect::bottom(Val::Px(spacing::SM)),
                        ..default()
                    },
                ));

                if quests.is_empty() {
                    col.spawn((
                        Text::new("No quests yet. Speak with the village elder to begin."),
                        TextFont {
                            font_size: font_size::BODY,
                            ..default()
                        },
                        TextColor(palette::TEXT_SECONDARY),
                    ));
                }

                for quest in quests {
                    let (tag, tag_color) = match quest.status {
                        QuestStatus::Active => ("ACTIVE", palette::BRAND),
                        QuestStatus::Completed => ("DONE", palette::ACCENT_SUCCESS),
                        QuestStatus::Failed => ("FAILED", palette::ACCENT_DANGER),
                    };
                    col.spawn((
                        Text::new(format!("[{}]  {}", tag, quest.title)),
                        TextFont {
                            font_size: font_size::BODY_LG,
                            ..default()
                        },
                        TextColor(tag_color),
                        Node {
                            margin: UiRect::top(Val::Px(spacing::MD)),
                            ..default()
                        },
                    ));
                    col.spawn((
                        Text::new(quest.description.clone()),
                        TextFont {
                            font_size: font_size::SMALL,
                            ..default()
                        },
                        TextColor(palette::TEXT_SECONDARY),
                    ));
                    for obj in &quest.objectives {
                        let (mark, color) = if obj.is_complete() {
                            ("✓", palette::ACCENT_SUCCESS)
                        } else {
                            ("•", palette::TEXT_PRIMARY)
                        };
                        let line = if obj.required > 1 {
                            format!("   {} {} ({}/{})", mark, obj.description, obj.progress, obj.required)
                        } else {
                            format!("   {} {}", mark, obj.description)
                        };
                        col.spawn((
                            Text::new(line),
                            TextFont {
                                font_size: font_size::LABEL,
                                ..default()
                            },
                            TextColor(color),
                        ));
                    }
                }

                col.spawn((
                    Text::new("J or Esc — close"),
                    TextFont {
                        font_size: font_size::SMALL,
                        ..default()
                    },
                    TextColor(palette::TEXT_DIM),
                    Node {
                        margin: UiRect::top(Val::Px(spacing::LG)),
                        ..default()
                    },
                ));
            });
        });
}

fn status_rank(status: QuestStatus) -> u8 {
    match status {
        QuestStatus::Active => 0,
        QuestStatus::Failed => 1,
        QuestStatus::Completed => 2,
    }
}
