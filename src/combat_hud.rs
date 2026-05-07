//! In-battle ability picker UI.
//!
//! Spawns a bottom-anchored panel during the player's turn that lists every
//! ability the active combatant has on its [`Abilities`] component, plus
//! Attack / Defend / Wait. Two-step targeting:
//!
//! 1. Click a button. Defend / Wait fire immediately. Attack and any ability
//!    transition the HUD into [`HudMode::AwaitingTarget`] and prompt for a
//!    world click.
//! 2. The next left-click in the world picks the closest enemy combatant
//!    inside [`TARGET_PICK_RADIUS`] and fires the action. Right-click or Esc
//!    cancels back to [`HudMode::Idle`].
//!
//! The target-click system runs before [`crate::movement::mouse_click`] and
//! consumes the mouse input when it acts, so the existing click-to-move
//! behaviour still works when the HUD is idle.

use bevy::prelude::*;

use crate::battle::{BattleParticipant, BattleSide};
use crate::combat_ability::{Ability, Ability_Tree};
use crate::combat_plugin::{
    Abilities, PendingPlayerAction, PlayerAction, PlayerActionEvent,
};
use crate::core::{GameState, Game_State, MainCamera};
use crate::ui_style::{
    button_node, button_text, button_visual, floating_panel, label_text, spacing,
};

/// Plugin entry point.
pub struct CombatHudPlugin;

impl Plugin for CombatHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatHudState>()
            // Spawn / despawn must run before button handlers so a freshly
            // spawned HUD is interactable on the same frame.
            .add_systems(Update, manage_combat_hud_lifetime)
            .add_systems(Update, handle_combat_hud_buttons.after(manage_combat_hud_lifetime))
            .add_systems(
                Update,
                handle_target_click.before(crate::movement::mouse_click),
            )
            .add_systems(Update, cancel_targeting_on_escape)
            .add_systems(Update, sync_hud_mode_text);
    }
}

/// Distance (in world units) within which a click counts as picking an enemy
/// for ability targeting. Roughly matches the 32-px sprite size used elsewhere
/// in [`crate::battle`].
const TARGET_PICK_RADIUS: f32 = 32.0;

/// Resource describing the HUD's current targeting state. Read by the click
/// handler in this module and by [`crate::movement::mouse_click`] to decide
/// which click semantics apply.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct CombatHudState {
    pub mode: HudMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HudMode {
    /// No pending target selection. Default.
    #[default]
    Idle,
    /// Player has picked an action; the next world click commits the target.
    AwaitingTarget(SelectedAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedAction {
    /// Basic weapon attack.
    Attack,
    /// Activated ability (spell or non-spell), keyed by id.
    Ability(u16),
}

/// Marker on the HUD root.
#[derive(Component)]
struct CombatHudRoot;

/// Marker on the "Target: ..." status label, so the indicator updates in place.
#[derive(Component)]
struct CombatHudStatusLabel;

/// One HUD button. The action variant tells the click handler what to fire.
#[derive(Component, Clone, Copy)]
struct CombatHudButton(HudButton);

#[derive(Clone, Copy)]
enum HudButton {
    Attack,
    Ability(u16),
    Defend,
    Wait,
}

/// Spawn the HUD when a player turn begins; despawn when no player turn is
/// pending. Re-spawns each turn so the ability list stays in sync with any
/// `UnlockAbility` events that fired since the last turn.
fn manage_combat_hud_lifetime(
    mut commands: Commands,
    game_state: Res<GameState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    mut state: ResMut<CombatHudState>,
    abilities_q: Query<&Abilities>,
    hud_q: Query<Entity, With<CombatHudRoot>>,
    children: Query<&Children>,
) {
    let in_battle = game_state.0 == Game_State::Battle;
    let active = pending.entity;

    let should_show = in_battle && active.is_some();
    let already_shown = !hud_q.is_empty();

    if should_show && !already_shown {
        if let Some(actor) = active {
            let abilities = abilities_q.get(actor).map(|a| a.0.clone()).unwrap_or_default();
            spawn_combat_hud(&mut commands, &abilities, ability_tree.as_deref());
        }
    } else if !should_show && already_shown {
        for entity in hud_q.iter() {
            despawn_recursive(&mut commands, entity, &children);
        }
        state.mode = HudMode::Idle;
    }
}

fn spawn_combat_hud(
    commands: &mut Commands,
    ability_ids: &[u16],
    ability_tree: Option<&Ability_Tree>,
) {
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(spacing::XL),
                left: Val::Percent(50.0),
                margin: UiRect {
                    left: Val::Px(-220.0),
                    ..default()
                },
                width: Val::Px(440.0),
                ..default()
            },
            CombatHudRoot,
        ))
        .id();

    commands.entity(root).with_children(|parent| {
        parent.spawn(floating_panel(440.0)).with_children(|col| {
            col.spawn((label_text("Click an action"), CombatHudStatusLabel));

            // Top row: basic actions.
            row(col, |row| {
                spawn_button(row, "Attack", CombatHudButton(HudButton::Attack));
                spawn_button(row, "Defend", CombatHudButton(HudButton::Defend));
                spawn_button(row, "Wait", CombatHudButton(HudButton::Wait));
            });

            if ability_ids.is_empty() {
                col.spawn(label_text("(no abilities)"));
                return;
            }

            col.spawn((
                label_text("Abilities"),
                Node {
                    margin: UiRect::top(Val::Px(spacing::SM)),
                    ..default()
                },
            ));

            // One column of ability buttons. Look the ability up so the label
            // shows name + AP / magic cost; fall back to the bare id if the
            // tree resource isn't available yet at first frame.
            for &id in ability_ids {
                let label = ability_tree
                    .and_then(|t| t.0.find(id))
                    .map(format_ability_label)
                    .unwrap_or_else(|| format!("Ability {id}"));
                spawn_button(col, &label, CombatHudButton(HudButton::Ability(id)));
            }
        });
    });
}

fn format_ability_label(ability: Ability) -> String {
    let mut parts = vec![format!("{}", ability.name)];
    if ability.action_point_cost > 0 {
        parts.push(format!("AP {}", ability.action_point_cost));
    }
    if ability.magic_cost > 0.0 {
        parts.push(format!(
            "{} {:.0}",
            magic_school_short(ability.magic_school),
            ability.magic_cost
        ));
    }
    parts.join("   ")
}

fn magic_school_short(school: crate::combat_ability::MagicSchool) -> &'static str {
    use crate::combat_ability::MagicSchool;
    match school {
        MagicSchool::Kiho => "Ki",
        MagicSchool::Chiseijutsu => "Chi",
        MagicSchool::Yokaijutsu => "Yo",
        MagicSchool::Kamishin => "Kami",
    }
}

fn row(parent: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(spacing::SM),
            ..default()
        })
        .with_children(|row| build(row));
}

fn spawn_button(parent: &mut ChildSpawnerCommands, label: &str, marker: CombatHudButton) {
    let mut node = button_node(36.0);
    node.flex_grow = 1.0;
    parent
        .spawn((Button::default(), node, button_visual(), marker))
        .with_children(|btn| {
            btn.spawn(button_text(label));
        });
}

/// Reads UI button presses and either fires the action immediately
/// (Defend/Wait) or transitions the HUD into [`HudMode::AwaitingTarget`]
/// (Attack/Ability) so the next world click commits the target.
fn handle_combat_hud_buttons(
    mut interactions: Query<(&Interaction, &CombatHudButton), (Changed<Interaction>, With<Button>)>,
    mut state: ResMut<CombatHudState>,
    mut actions: MessageWriter<PlayerActionEvent>,
) {
    for (interaction, button) in &mut interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button.0 {
            HudButton::Defend => {
                actions.write(PlayerActionEvent { action: PlayerAction::Defend });
                state.mode = HudMode::Idle;
            }
            HudButton::Wait => {
                actions.write(PlayerActionEvent { action: PlayerAction::Wait });
                state.mode = HudMode::Idle;
            }
            HudButton::Attack => {
                state.mode = HudMode::AwaitingTarget(SelectedAction::Attack);
            }
            HudButton::Ability(id) => {
                state.mode = HudMode::AwaitingTarget(SelectedAction::Ability(id));
            }
        }
    }
}

/// Updates the "Target: ..." label so the player knows when targeting is armed.
fn sync_hud_mode_text(
    state: Res<CombatHudState>,
    ability_tree: Option<Res<Ability_Tree>>,
    mut labels: Query<&mut Text, With<CombatHudStatusLabel>>,
) {
    let desired = match state.mode {
        HudMode::Idle => "Click an action".to_string(),
        HudMode::AwaitingTarget(SelectedAction::Attack) => {
            "Click an enemy to attack (right-click to cancel)".to_string()
        }
        HudMode::AwaitingTarget(SelectedAction::Ability(id)) => {
            let name = ability_tree
                .as_deref()
                .and_then(|t| t.0.find(id))
                .map(|a| a.name.clone())
                .unwrap_or_else(|| format!("Ability {id}"));
            format!("Click an enemy to use {name} (right-click to cancel)")
        }
    };
    for mut text in &mut labels {
        if text.0 != desired {
            text.0 = desired.clone();
        }
    }
}

/// When in [`HudMode::AwaitingTarget`], the next left-click that lands inside
/// [`TARGET_PICK_RADIUS`] of an enemy combatant fires the chosen action and
/// resets the HUD mode. A right-click cancels.
///
/// Consumes the mouse input via `clear_just_pressed` so the click is *not*
/// also seen by [`crate::movement::mouse_click`] (which would otherwise treat
/// it as a movement request).
fn handle_target_click(
    mut state: ResMut<CombatHudState>,
    mut mouse_input: ResMut<ButtonInput<MouseButton>>,
    game_state: Res<GameState>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    windows: Query<&Window>,
    enemies_q: Query<(Entity, &Transform, &BattleSide), With<BattleParticipant>>,
    mut actions: MessageWriter<PlayerActionEvent>,
) {
    if game_state.0 != Game_State::Battle {
        return;
    }
    let HudMode::AwaitingTarget(selected) = state.mode else {
        return;
    };

    if mouse_input.just_pressed(MouseButton::Right) {
        state.mode = HudMode::Idle;
        mouse_input.clear_just_pressed(MouseButton::Right);
        return;
    }
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }

    let Some((camera, camera_tf)) = camera_q.iter().next() else { return };
    let Some(window) = windows.iter().next() else { return };
    let Some(screen_pos) = window.cursor_position() else { return };
    let Ok(cursor_world) = camera.viewport_to_world_2d(camera_tf, screen_pos) else {
        return;
    };

    let Some(target) = nearest_enemy_within(&enemies_q, cursor_world, TARGET_PICK_RADIUS) else {
        // No enemy under the cursor; leave mode armed and don't consume the
        // click, so the player can try again. Movement handler will see this
        // click — that's fine in idle, but we're still in AwaitingTarget so
        // we do want to swallow it to avoid accidental movement.
        mouse_input.clear_just_pressed(MouseButton::Left);
        return;
    };

    match selected {
        SelectedAction::Attack => {
            actions.write(PlayerActionEvent {
                action: PlayerAction::Attack(target),
            });
        }
        SelectedAction::Ability(id) => {
            actions.write(PlayerActionEvent {
                action: PlayerAction::UseAbility(id as u32, target),
            });
        }
    }

    state.mode = HudMode::Idle;
    mouse_input.clear_just_pressed(MouseButton::Left);
}

fn nearest_enemy_within(
    enemies_q: &Query<(Entity, &Transform, &BattleSide), With<BattleParticipant>>,
    cursor_world: Vec2,
    radius: f32,
) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for (entity, tf, side) in enemies_q.iter() {
        if *side != BattleSide::Enemy {
            continue;
        }
        let pos = tf.translation.truncate();
        let d = pos.distance(cursor_world);
        if d > radius {
            continue;
        }
        match best {
            None => best = Some((entity, d)),
            Some((_, prev)) if d < prev => best = Some((entity, d)),
            _ => {}
        }
    }
    best.map(|(e, _)| e)
}

/// Esc clears any in-progress target selection without firing an action.
fn cancel_targeting_on_escape(
    mut state: ResMut<CombatHudState>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if state.mode != HudMode::Idle && keys.just_pressed(KeyCode::Escape) {
        state.mode = HudMode::Idle;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_default() {
        let state = CombatHudState::default();
        assert_eq!(state.mode, HudMode::Idle);
    }

    #[test]
    fn awaiting_target_compares_by_action() {
        let a = HudMode::AwaitingTarget(SelectedAction::Ability(42));
        let b = HudMode::AwaitingTarget(SelectedAction::Ability(42));
        let c = HudMode::AwaitingTarget(SelectedAction::Ability(43));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, HudMode::Idle);
    }
}
