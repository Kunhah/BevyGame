//! Behavior-tree driven combat AI.
//!
//! Profiles are authored as RON ([`assets/data/decision_tree.ron`]) and
//! deserialised into a tree of [`BtNode`]s. On each non-player turn start,
//! [`evaluate_behavior_tree_system`] walks the tree for the active actor and
//! turns the leaf decision into an attack/ability/defend/wait intent that the
//! existing combat plumbing already handles.
//!
//! The tree mixes parameter-sensitive conditions (HP%, AP, AI temperament
//! values like aggressiveness/bravery/panic_threshold) with traditional BT
//! composites (Selector/Sequence/Random). That gives content authors a way
//! to express "if my HP is below my panic threshold, try to heal; otherwise
//! pick the most damaging ability my AP allows; otherwise basic attack" in
//! plain data.

use std::collections::HashMap;
use std::fs;

use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::battle::{BattleSide, PendingAiMove, AI_MELEE_RANGE, AI_MOVE_CAP};
use crate::constants::PLAYER_SPEED;
use crate::combat_ability::{Ability, AbilityEffect, Ability_Tree};
use crate::combat_plugin::{
    Abilities, AIParameters, AbilityIntentEvent, ActionCause, AttackContext, AttackIntentEvent,
    CombatStats, DefendIntentEvent, PlayerControlled, TargetFocus, TurnEndEvent, TurnInProgress,
    TurnStartEvent, WaitIntentEvent,
};

const BEHAVIOR_TREE_PATH: &str = "assets/data/decision_tree.ron";

// ---------------------------------------------------------------------------
// Tree node types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BtNode {
    // ---- Composites ----
    /// Try children in order; return Success on the first child that
    /// succeeds. Returns Failure only if every child fails.
    Selector(Vec<BtNode>),
    /// Run children in order; return Failure on the first child that fails.
    /// Returns Success only if every child succeeds.
    Sequence(Vec<BtNode>),
    /// Pick a child weighted by its first-element u32, then tick it.
    Random(Vec<(u32, BtNode)>),

    // ---- Decorators ----
    Inverter(Box<BtNode>),
    AlwaysSucceed(Box<BtNode>),

    // ---- Conditions ----
    HpBelow { percent: u8 },
    HpAbove { percent: u8 },
    /// Any *living* ally (same BattleSide) has HP below `percent`.
    AllyHpBelow { percent: u8 },
    /// Total magic across all schools is below `percent` of total max.
    MagicBelow { percent: u8 },
    /// Self has at least this much current AP.
    ActionPointsAtLeast { amount: i32 },
    AggressivenessAtLeast { value: u8 },
    BraveryAtLeast { value: u8 },
    CautionAtLeast { value: u8 },
    /// True when current HP% is at or below the actor's panic threshold.
    InPanic,
    EnemiesAlive { at_least: u8 },
    /// Living allies (excluding self).
    AlliesAlive { at_least: u8 },
    HasAbility { id: u16 },
    /// Coin flip — succeeds with the given probability.
    Chance { percent: u8 },

    // ---- Actions (succeed when an intent has been recorded) ----
    BasicAttack,
    UseAbility { id: u16 },
    /// Picks the first owned ability whose effects are all `Heal`.
    UseFirstHealingAbility,
    /// Picks the owned damage ability with the highest expected damage that
    /// the actor can pay for (AP cost only — magic cost is ignored for now).
    UseHighestDamageAbility,
    Defend,
    Wait,
}

// ---------------------------------------------------------------------------
// Profile data + asset loader
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIProfile {
    pub logic: BtNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AIBehaviors {
    pub profiles: HashMap<String, AIProfile>,
}

#[derive(Resource, Default)]
pub struct BehaviorTreeAssets(pub AIBehaviors);

/// Component pointing at a profile inside [`BehaviorTreeAssets`]. Attach this
/// to any non-player combat actor that should be driven by the BT.
#[derive(Component, Debug, Clone)]
pub struct BehaviorTreeProfile(pub String);

fn load_behavior_trees(mut assets: ResMut<BehaviorTreeAssets>) {
    match fs::read_to_string(BEHAVIOR_TREE_PATH) {
        Ok(text) => match ron::de::from_str::<AIBehaviors>(&text) {
            Ok(data) => {
                let count = data.profiles.len();
                assets.0 = data;
                info!("Loaded {count} AI profile(s) from {BEHAVIOR_TREE_PATH}");
            }
            Err(err) => warn!("Failed to parse {BEHAVIOR_TREE_PATH}: {err}"),
        },
        Err(err) => warn!("Failed to read {BEHAVIOR_TREE_PATH}: {err}"),
    }
}

// ---------------------------------------------------------------------------
// Plugin + system wiring
// ---------------------------------------------------------------------------

pub struct AiDecisionPlugin;

impl Plugin for AiDecisionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BehaviorTreeAssets>()
            .add_systems(Startup, load_behavior_trees)
            .add_systems(Update, evaluate_behavior_tree_system);
    }
}

// ---------------------------------------------------------------------------
// Evaluation context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum AiAction {
    Attack { target: Entity },
    Ability { ability_id: u16, target: Entity },
    Defend,
    Wait,
}

#[derive(Debug, Clone)]
pub struct ActorSnapshot {
    pub entity: Entity,
    pub side: BattleSide,
    pub hp_percent: u8,
    pub magic_percent: u8,
    pub action_points: i32,
    pub abilities: Vec<u16>,
    pub params: AIParameters,
    pub position: Vec2,
}

pub struct BtContext<'a> {
    pub actor: ActorSnapshot,
    pub allies: Vec<ActorSnapshot>,
    pub enemies: Vec<ActorSnapshot>,
    pub ability_tree: Option<&'a Ability_Tree>,
    pub decision: Option<AiAction>,
}

impl BtContext<'_> {
    fn target_for_focus(&self, focus: TargetFocus) -> Option<&ActorSnapshot> {
        if self.enemies.is_empty() {
            return None;
        }
        match focus {
            TargetFocus::LowestHp => self.enemies.iter().min_by_key(|t| t.hp_percent),
            TargetFocus::HighestHp => self.enemies.iter().max_by_key(|t| t.hp_percent),
            TargetFocus::Closest => self.enemies.iter().min_by(|a, b| {
                let da = self.actor.position.distance_squared(a.position);
                let db = self.actor.position.distance_squared(b.position);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            }),
            TargetFocus::Furthest => self.enemies.iter().max_by(|a, b| {
                let da = self.actor.position.distance_squared(a.position);
                let db = self.actor.position.distance_squared(b.position);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            }),
        }
    }

    fn weakest_ally(&self) -> Option<&ActorSnapshot> {
        self.allies.iter().min_by_key(|a| a.hp_percent)
    }
}

// ---------------------------------------------------------------------------
// Evaluator
// ---------------------------------------------------------------------------

pub fn tick(node: &BtNode, ctx: &mut BtContext, rng: &mut impl Rng) -> BtStatus {
    use BtStatus::*;
    match node {
        BtNode::Selector(children) => {
            for child in children {
                if tick(child, ctx, rng) == Success {
                    return Success;
                }
            }
            Failure
        }
        BtNode::Sequence(children) => {
            for child in children {
                if tick(child, ctx, rng) == Failure {
                    return Failure;
                }
            }
            Success
        }
        BtNode::Random(weighted) => {
            let total: u32 = weighted.iter().map(|(w, _)| *w).sum();
            if total == 0 || weighted.is_empty() {
                return Failure;
            }
            let mut roll = rng.gen_range(0..total);
            for (weight, child) in weighted {
                if roll < *weight {
                    return tick(child, ctx, rng);
                }
                roll -= *weight;
            }
            Failure
        }
        BtNode::Inverter(child) => {
            if tick(child, ctx, rng) == Success {
                Failure
            } else {
                Success
            }
        }
        BtNode::AlwaysSucceed(child) => {
            tick(child, ctx, rng);
            Success
        }
        BtNode::HpBelow { percent } => bool_to_status(ctx.actor.hp_percent < *percent),
        BtNode::HpAbove { percent } => bool_to_status(ctx.actor.hp_percent > *percent),
        BtNode::AllyHpBelow { percent } => {
            bool_to_status(ctx.allies.iter().any(|a| a.hp_percent < *percent))
        }
        BtNode::MagicBelow { percent } => {
            bool_to_status(ctx.actor.magic_percent < *percent)
        }
        BtNode::ActionPointsAtLeast { amount } => {
            bool_to_status(ctx.actor.action_points >= *amount)
        }
        BtNode::AggressivenessAtLeast { value } => {
            bool_to_status(ctx.actor.params.aggressiveness >= *value)
        }
        BtNode::BraveryAtLeast { value } => {
            bool_to_status(ctx.actor.params.bravery >= *value)
        }
        BtNode::CautionAtLeast { value } => {
            bool_to_status(ctx.actor.params.caution >= *value)
        }
        BtNode::InPanic => bool_to_status(ctx.actor.hp_percent <= ctx.actor.params.panic_threshold),
        BtNode::EnemiesAlive { at_least } => {
            bool_to_status(ctx.enemies.len() as u8 >= *at_least)
        }
        BtNode::AlliesAlive { at_least } => {
            bool_to_status(ctx.allies.len() as u8 >= *at_least)
        }
        BtNode::HasAbility { id } => {
            bool_to_status(ctx.actor.abilities.iter().any(|owned| owned == id))
        }
        BtNode::Chance { percent } => {
            let threshold = (*percent).min(100) as u32;
            bool_to_status(rng.gen_range(0..100) < threshold)
        }
        BtNode::BasicAttack => {
            if let Some(target) = ctx.target_for_focus(ctx.actor.params.focus_preference) {
                ctx.decision = Some(AiAction::Attack { target: target.entity });
                Success
            } else {
                Failure
            }
        }
        BtNode::UseAbility { id } => {
            if !ctx.actor.abilities.iter().any(|owned| owned == id) {
                return Failure;
            }
            let Some(target) = ctx.target_for_focus(ctx.actor.params.focus_preference) else {
                return Failure;
            };
            ctx.decision = Some(AiAction::Ability {
                ability_id: *id,
                target: target.entity,
            });
            Success
        }
        BtNode::UseFirstHealingAbility => {
            let Some(tree) = ctx.ability_tree.as_ref() else {
                return Failure;
            };
            for &owned_id in &ctx.actor.abilities {
                let Some(ability) = tree.0.find(owned_id) else {
                    continue;
                };
                if ability.effects.is_empty() {
                    continue;
                }
                if !ability
                    .effects
                    .iter()
                    .all(|e| matches!(e, AbilityEffect::Heal { .. }))
                {
                    continue;
                }
                let target = ctx
                    .weakest_ally()
                    .map(|a| a.entity)
                    .unwrap_or(ctx.actor.entity);
                ctx.decision = Some(AiAction::Ability {
                    ability_id: owned_id,
                    target,
                });
                return Success;
            }
            Failure
        }
        BtNode::UseHighestDamageAbility => {
            let Some(tree) = ctx.ability_tree.as_ref() else {
                return Failure;
            };
            let mut best: Option<(u16, u32)> = None;
            for &owned_id in &ctx.actor.abilities {
                let Some(ability) = tree.0.find(owned_id) else {
                    continue;
                };
                if ability.action_point_cost > ctx.actor.action_points {
                    continue;
                }
                let damage = expected_damage(&ability);
                if damage == 0 {
                    continue;
                }
                if best.map_or(true, |(_, d)| damage > d) {
                    best = Some((owned_id, damage));
                }
            }
            let Some((id, _)) = best else {
                return Failure;
            };
            let Some(target) = ctx.target_for_focus(ctx.actor.params.focus_preference) else {
                return Failure;
            };
            ctx.decision = Some(AiAction::Ability {
                ability_id: id,
                target: target.entity,
            });
            Success
        }
        BtNode::Defend => {
            ctx.decision = Some(AiAction::Defend);
            Success
        }
        BtNode::Wait => {
            ctx.decision = Some(AiAction::Wait);
            Success
        }
    }
}

fn bool_to_status(v: bool) -> BtStatus {
    if v { BtStatus::Success } else { BtStatus::Failure }
}

fn expected_damage(ability: &Ability) -> u32 {
    ability
        .effects
        .iter()
        .map(|e| match e {
            AbilityEffect::Damage { floor, ceiling, .. } => (*floor + *ceiling) / 2,
            _ => 0,
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Bevy system: walk the BT for whichever non-player actor just started a turn
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn evaluate_behavior_tree_system(
    mut commands: Commands,
    mut turn_start_reader: MessageReader<TurnStartEvent>,
    profiles: Res<BehaviorTreeAssets>,
    ability_tree: Option<Res<Ability_Tree>>,
    actors: Query<(
        Entity,
        &BattleSide,
        &CombatStats,
        Option<&Abilities>,
        Option<&AIParameters>,
        Option<&GlobalTransform>,
    )>,
    profile_q: Query<&BehaviorTreeProfile>,
    player_q: Query<(), With<PlayerControlled>>,
    mut intent_writer: MessageWriter<AttackIntentEvent>,
    mut ability_writer: MessageWriter<AbilityIntentEvent>,
    mut defend_writer: MessageWriter<DefendIntentEvent>,
    mut wait_writer: MessageWriter<WaitIntentEvent>,
    mut turn_end_writer: MessageWriter<TurnEndEvent>,
    mut turn_in_progress: ResMut<TurnInProgress>,
) {
    let mut rng = rand::rng();
    for ev in turn_start_reader.read() {
        if player_q.get(ev.who).is_ok() {
            continue;
        }
        let Ok(profile_name) = profile_q.get(ev.who) else {
            // Falls through to the legacy demo AI in `on_turn_start_system`.
            continue;
        };
        let Some(profile) = profiles.0.profiles.get(&profile_name.0) else {
            warn!("AI profile not found: {}", profile_name.0);
            continue;
        };

        let Some(actor_snapshot) = build_snapshot(&actors, ev.who) else {
            continue;
        };

        let mut allies = Vec::new();
        let mut enemies = Vec::new();
        for (entity, _side, _, _, _, _) in actors.iter() {
            if entity == ev.who {
                continue;
            }
            let Some(snap) = build_snapshot(&actors, entity) else {
                continue;
            };
            let same_side = matches!(
                (actor_snapshot.side, snap.side),
                (BattleSide::Ally, BattleSide::Ally) | (BattleSide::Enemy, BattleSide::Enemy),
            );
            if same_side {
                allies.push(snap);
            } else {
                enemies.push(snap);
            }
        }

        let mut ctx = BtContext {
            actor: actor_snapshot,
            allies,
            enemies,
            ability_tree: ability_tree.as_deref(),
            decision: None,
        };
        tick(&profile.logic, &mut ctx, &mut rng);

        let actor = ev.who;
        // When a melee attacker's target is out of reach, defer the strike:
        // stash a `PendingAiMove` and hold the turn open so
        // `ai_combat_movement_system` can walk the unit in (through any hazards)
        // before the attack lands. All other decisions resolve instantly.
        let mut deferred_to_movement = false;
        match ctx.decision {
            Some(AiAction::Attack { target }) => {
                let target_pos = ctx
                    .enemies
                    .iter()
                    .find(|s| s.entity == target)
                    .map(|s| s.position);
                let out_of_range = target_pos
                    .map(|tp| ctx.actor.position.distance(tp) > AI_MELEE_RANGE)
                    .unwrap_or(false);
                if out_of_range {
                    let movement = actors
                        .get(actor)
                        .map(|(_, _, stats, _, _, _)| stats.movement.current.max(0) as f32)
                        .unwrap_or(0.0);
                    let budget = (movement * PLAYER_SPEED).min(AI_MOVE_CAP);
                    if budget > 0.0 {
                        commands.entity(actor).insert(PendingAiMove {
                            target,
                            remaining: budget,
                        });
                        deferred_to_movement = true;
                    }
                }
                if !deferred_to_movement {
                    intent_writer.write(AttackIntentEvent {
                        attacker: actor,
                        target,
                        ability: None,
                        context: AttackContext::default(),
                        cause: ActionCause::Ai,
                    });
                }
            }
            Some(AiAction::Ability { ability_id, target }) => {
                // The behaviour tree already picked the target (an enemy for
                // offensive spells, the weakest ally / self for support); pass
                // it through so `resolve_ai_ability_intent_system` can apply the
                // ability's effects, mirroring the player's single-target path.
                ability_writer.write(AbilityIntentEvent {
                    user: actor,
                    ability_id,
                    target,
                });
            }
            Some(AiAction::Defend) => {
                defend_writer.write(DefendIntentEvent { defender: actor });
            }
            Some(AiAction::Wait) | None => {
                wait_writer.write(WaitIntentEvent { waiter: actor });
            }
        }
        // A deferred attacker keeps the turn open until it finishes moving;
        // `ai_combat_movement_system` will emit TurnEndEvent / clear the lock.
        if !deferred_to_movement {
            turn_end_writer.write(TurnEndEvent { who: actor });
            turn_in_progress.0 = false;
        }
    }
}

#[allow(clippy::type_complexity)]
fn build_snapshot(
    actors: &Query<(
        Entity,
        &BattleSide,
        &CombatStats,
        Option<&Abilities>,
        Option<&AIParameters>,
        Option<&GlobalTransform>,
    )>,
    entity: Entity,
) -> Option<ActorSnapshot> {
    let (e, side, stats, abilities, params, transform) = actors.get(entity).ok()?;
    if stats.health.current <= 0 {
        return None;
    }
    let hp_percent = if stats.health.base > 0 {
        ((stats.health.current.max(0) as i64 * 100) / stats.health.base as i64).clamp(0, 100) as u8
    } else {
        0
    };
    let total_max = stats.total_magic_base();
    let magic_percent = if total_max > 0.0 {
        ((stats.total_magic_current() / total_max) * 100.0).clamp(0.0, 100.0) as u8
    } else {
        100
    };
    let abilities = abilities.map(|a| a.0.clone()).unwrap_or_default();
    let params = params.copied().unwrap_or_default();
    let position = transform
        .map(|t| t.translation().truncate())
        .unwrap_or_default();
    Some(ActorSnapshot {
        entity: e,
        side: *side,
        hp_percent,
        magic_percent,
        action_points: stats.action_points.current,
        abilities,
        params,
        position,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped profiles must round-trip through serde or the game won't
    /// load any AI behaviour.
    #[test]
    fn shipped_profiles_parse() {
        let text = std::fs::read_to_string(BEHAVIOR_TREE_PATH)
            .expect("decision_tree.ron exists at the documented path");
        let parsed: AIBehaviors = ron::de::from_str(&text)
            .expect("decision_tree.ron deserialises into AIBehaviors");
        for name in ["aggressive", "defensive", "opportunistic", "mage", "coward"] {
            assert!(
                parsed.profiles.contains_key(name),
                "expected profile `{name}` in decision_tree.ron",
            );
        }
    }
}

