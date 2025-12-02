use bevy::ecs::event;
use bevy::prelude::*;
use rand::Rng;
use std::collections::VecDeque;
use bevy::ecs::system::command::{insert_batch, insert_resource};
use bevy::prelude::*;
use std::fmt::Debug;
use std::f32::consts::PI;
use serde::{Serialize, Deserialize};
use std::cmp::Ordering;
use std::sync::Arc;
use std::sync::RwLock;
use std::fs;
use std::f64::log;

/// -----------------------------
/// Components & Types
/// -----------------------------

#[derive(Component, Debug)]
pub struct CharacterId(pub u32);

#[derive(Component, Debug)]
pub struct Name(pub String);

#[derive(Component, Debug)]
pub struct Class(pub String);

#[derive(Debug, Clone, Copy)]
pub enum DamageType {
    Physical,
    Fire,
    Ice,
    True,
}

/// Current / max / regen components
#[derive(Component, Debug)]
pub struct Health {
    pub current: i32,
    pub max: i32,
    pub regen: i32,
}

#[derive(Component, Debug)]
pub struct Magic {
    pub current: i32,
    pub max: i32,
    pub regen: i32,
}

#[derive(Component, Debug)]
pub struct Stamina {
    pub current: i32,
    pub max: i32,
    pub regen: i32,
}

/// Base stats that describe the character (unchanged by temporary modifiers)
#[derive(Component, Debug)]
pub struct CombatStats {
    pub base_lethality: i32,
    pub base_hit: i32,
    pub base_armor: i32,
    pub base_agility: i32,
    pub base_mind: i32,
    pub base_morale: i32,
    pub movement: i32,
}

#[derive(Debug, Clone, Copy)]
pub enum Stat {
    Health,
    HealthRegen,
    Magic,
    MagicRegen,
    Stamina,
    StaminaRegen,
    Lethality,
    Hit,
    Agility,
    Mind,
    Morale,
}

fn get_stat_value(
    stat: Stat,
    combat_stats: Option<&CombatStats>,
    health: Option<&Health>,
    magic: Option<&Magic>,
    stamina: Option<&Stamina>,
) -> i32 {
    match stat {
        Stat::Lethality => combat_stats.map(|c| c.base_lethality as i32).unwrap_or(0),
        Stat::Hit => combat_stats.map(|c| c.base_hit as i32).unwrap_or(0),
        Stat::Agility => combat_stats.map(|c| c.base_agility as i32).unwrap_or(0),
        Stat::Mind => combat_stats.map(|c| c.base_mind as i32).unwrap_or(0),
        Stat::Morale => combat_stats.map(|c| c.base_morale as i32).unwrap_or(0),
        Stat::Health => health.map(|h| h.current as i32).unwrap_or(0),
        Stat::Magic => magic.map(|m| m.current as i32).unwrap_or(0),
        Stat::Stamina => stamina.map(|s| s.current as i32).unwrap_or(0),
        // Add other mappings if you have regen or other derived stats
        _ => 0,
    }
}


// The attributes the player distributes.
// All small (u8), simple, and easy to balance.
#[derive(Component, Debug, Default)]
pub struct GrowthAttributes {
    pub vitality: u8,   // influences Health curve, little influence on endurance and power
    pub endurance: u8,  // influences Stamina curve, little influence on vitality and power
    pub spirit: u8,     // influences Magic curve, little influence on insight
    pub power: u8,      // influences lethality
    pub control: u8,    // influences hit, little influence on agility
    pub agility: u8,    // influences agility, little influence on control
    pub insight: u8,    // influences mind, little influence on resolve
    pub resolve: u8,    // influences morale
}

// A character-specific growth curve.
// These are multipliers (or offsets) applied on top of the level up formulas.
#[derive(Component, Debug, Clone)]
pub struct GrowthCurve {
    pub hp_curve: f32,
    pub stamina_curve: f32,
    pub magic_curve: f32,

    pub lethality_curve: f32,
    pub hit_curve: f32,
    pub agility_curve: f32,
    pub mind_curve: f32,
    pub morale_curve: f32,
}

// Example: default balanced curve
impl Default for GrowthCurve {
    fn default() -> Self {
        Self {
            hp_curve: 1.0,
            stamina_curve: 1.0,
            magic_curve: 1.0,
            lethality_curve: 1.0,
            hit_curve: 1.0,
            agility_curve: 1.0,
            mind_curve: 1.0,
            morale_curve: 1.0,
        }
    }
}

/// Special negative values:
/// -1 = MISS
/// -2 = DODGE
/// -3 = HIT_KILL (guaranteed kill)
/// ... (you define what you need)
#[derive(Debug, Clone)]
pub enum DamageSignal {
    Miss = -1,
    Dodge = -2,
    HitKill = -3,
}

#[derive(Debug, Clone)]
pub struct QueuedDamage {
    pub attacker: Entity,
    pub target: Entity,
    pub amount: i32,                 // Pre-defense damage (>= 0). Negative reserved for signals.
    pub damage_type: DamageType,

    /// Attacker-side scaling: (stat, multiplier). These should be used when constructing
    /// the amount (we fill them here but process_attack_intent will apply them immediately).
    pub scaled_with: Vec<(Stat, f32)>,

    /// Defender-side stats to be used to reduce damage (stat, multiplier).
    /// e.g. vec![(Stat::Armor, 1.0)] means subtract defender.armor * 1.0 (scaled).
    pub defended_with: Vec<(Stat, f32)>,

    /// Optional override: force accuracy (0.0..1.0)
    pub accuracy_override: Option<f32>,

    pub crit_chance: f32,

    /// Optional tags for special behavior (from ability id, critical, reflect etc.)
    pub tags: Vec<u32>,
}

#[derive(Resource, Default, Debug)]
pub struct DamageQueue(pub Vec<QueuedDamage>);

/// Abilities placeholder (extend later)
#[derive(Component, Debug, Default)]
pub struct Abilities(pub Vec<u16>);

#[derive(Component, Debug, Default)]
pub struct AttributePointPool {
    pub available: u32,
    pub spent: u32,
}

/// Equipment slots hold Entities referencing equipment items
#[derive(Component, Debug, Default)]
pub struct EquipmentSlots {
    pub weapon: Option<Entity>,
    pub armor: Option<Entity>,
    pub accessories: Vec<Entity>,
}

#[derive(Component, Debug)]
pub enum PlayerAction {
    Attack(Entity),                // choose target
    UseAbility(u32, Entity),       // ability_id + target
    UseItem(u32, Option<Entity>),  // item_id
    Defend,
    Wait,
}

#[derive(Component, Debug, Default)]
pub struct PlayerControlled;

/// Tag components for class-specific logic (optional; systems can query these)
#[derive(Component, Debug)]
pub struct PaladinBehavior; // Petrus

#[derive(Component, Debug)]
pub struct RogueBehavior; // Niira

#[derive(Component)]
pub struct SpiritMediumBehavior; // Toshiko

/// Equipment entity
#[derive(Component, Debug)]
pub struct Equipment {
    pub id: u32,
    pub name: String,
    pub lethality: i32,
    pub hit: i32,
    pub armor: i32,
    pub agility: i32,
    pub mind: i32,
    pub morale: i32,
}

/// A single equipment-provided effect that can react to events.
/// This is data only — systems will read it and modify stats or emit events.
#[derive(Clone, Debug)]
pub enum EquipHook {
    /// On BeforeAttack: multiply lethality by multiplier for this attack only
    BeforeAttackMultiplier { stat: Stat, multiplier: f32, duration_turns: u32 },
    /// On BeforeHit: add flat damage
    BeforeHitFlatDamage { flat: i32 },
    // Add additional hook types as you need
}

/// Attach hooks to Equipment via a separate component so equipment is still simple
#[derive(Component, Debug)]
pub struct EquipmentHooks(pub Vec<EquipHook>);

/// Buff entity (applied to a character). Modeled as separate entity so it can store lifetime and effects.
#[derive(Component, Debug)]
pub struct Buff {
    pub stat: Stat,
    pub multiplier: f32,
    pub remaining_turns: u32,
    pub source: Option<Entity>, // which equipment/ability created it (optional)
}

/// Temporary stat modifiers applied to a character for a limited duration (e.g., one attack)
#[derive(Component, Debug)]
pub struct StatModifiers(pub Vec<StatModifier>);

#[derive(Clone, Debug)]
pub struct StatModifier {
    pub stat: Stat,
    pub multiplier: f32, // multiplicative (e.g., 1.2 => +20%)
    pub expires_in_turns: Option<u32>, // None => permanent until explicitly removed
    pub source: Option<Entity>,
}

/// Simple experience / level component (placeholder)
#[derive(Component, Debug)]
pub struct Experience(pub u32);

#[derive(Component, Debug)]
pub struct Level(pub u32);

#[derive(Component, Debug)]
pub struct AccumulatedAgility(pub u32);

impl Default for AccumulatedAgility {
    fn default() -> Self {
        Self(0)
    }
}

/// AI parameters (kept as component)
#[derive(Component, Debug)]
pub struct AIParameters {
    pub aggressiveness: u8,
    pub caution: u8,
    pub curiosity: u8,
    pub perception: u8,
    pub bravery: u8,
    pub patience: u8,
}

impl Default for AIParameters {
    fn default() -> Self {
        Self {
            aggressiveness: 5,
            caution: 5,
            curiosity: 5,
            perception: 5,
            bravery: 5,
            patience: 5,
        }
    }
}

/// -----------------------------
/// Events (FULL EVENTS model)
/// -----------------------------

#[derive(Debug, Clone, Message)]
pub struct AttackIntentEvent {
    pub attacker: Entity,
    pub target: Entity,
    //pub ability_id: Option<u16>, // optional: which ability triggered the attack
}

pub struct AbilityIntentEvent {
    pub user: Entity,
    pub ability_id: u16,
}

#[derive(Debug, Clone, Message)]
pub struct DefendIntentEvent {
    pub defender: Entity,
}

#[derive(Debug, Clone, Message)]
pub struct BeforeAttackEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub context: AttackContext,
}

#[derive(Debug, Clone, Message)]
pub struct AttackExecuteEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub context: AttackContext,
}

#[derive(Debug, Clone, Message)]
pub struct BeforeHitEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub context: AttackContext,
}

#[derive(Debug, Clone, Message)]
pub struct DamageEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub amount: i32,
    pub damage_type: DamageType,
}

#[derive(Debug, Clone, Message)]
pub struct HealEvent {
    pub healer: Entity,
    pub target: Entity,
    pub amount: u32,
}

#[derive(Debug, Clone, Message)]
pub struct ApplyBuffEvent {
    pub applier: Entity,
    pub target: Entity,
    pub stat: Stat,
    pub multiplier: f32,
    pub duration_in_ticks: u32,
    pub additional_effects: Option<Vec<AbilityEffect>>,
    pub applied_at: u32,
}

#[derive(Debug, Clone, Message)]
pub struct AfterHitEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub amount: i32,
    pub damage_type: DamageType,
}

#[derive(Debug, Clone, Message)]
pub struct AfterAttackEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub context: AttackContext,
}

#[derive(Debug, Clone, Message)]
pub struct IncomingDamageEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub amount: u32,
    pub damage_type: DamageType,
}

#[derive(Debug, Clone, Message)]
pub struct LevelUpEvent {
    pub who: Entity,
    pub old_level: u8,
    pub new_level: u8,
}

/// Turn & timeline events
#[derive(Debug, Clone, Message)]
pub struct TurnOrderCalculatedEvent; // signals the TurnOrder resource was updated

#[derive(Debug, Clone, Message)]
pub struct TurnStartEvent {
    pub who: Entity,
}

#[derive(Debug, Clone, Message)]
pub struct TurnEndEvent {
    pub who: Entity,
}

#[derive(Debug, Clone, Message)]
pub struct RoundStartEvent;

#[derive(Debug, Clone, Message)]
pub struct RoundEndEvent;

#[derive(Debug, Clone, Message)]
pub struct RespecEvent {
    pub who: Entity,
    pub full_reset: bool, // if true: clears all, sets to 0
    pub refund_all_points: bool, // if true: gives player all their spent points back
}

#[derive(Debug, Clone, Component)]
pub struct InCombat;
// q: Query<(Entity, &Agility, &AccumulatedAgility), With<InCombat>>, THIS SHOULD BE THE CORRECT QUERY

#[derive(Debug, Clone, Component)]
pub struct Dead;

#[derive(Debug, Clone, Component)]
pub struct PermanentlyDead;

#[derive(Debug, Clone, Component)]
pub struct AllyDeathBehavior;


/// Context shared along the attack pipeline; systems may mutate `meta` or read values.
#[derive(Debug, Clone)]
pub struct AttackContext {
    pub base_lethality: i32,
    pub base_hit: i32,
    pub extra_flat_damage: i32,
    pub multipliers: Vec<StatModifier>, // trackers for multiplicative modifiers applied during flow
}

impl Default for AttackContext {
    fn default() -> Self {
        Self {
            base_lethality: 0,
            base_hit: 0,
            extra_flat_damage: 0,
            multipliers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Message)]
pub struct AwardXpEvent {
    pub recipient: Entity,
    pub amount: u32,
}

#[derive(Debug, Clone, Message)]
pub struct LootEvent {
    pub loot: Vec<LootItem>,
    pub dropped_by: Entity,
}

pub struct LootItem {
    pub id: u32,
    pub quantity: u32,
}

#[derive(Debug, Clone, Resource)]
pub struct BattleLoot {
    pub items: Vec<LootItem>,
}

#[derive(Debug, Clone, Resource)]
pub struct PendingPlayerAction {
    pub entity: Option<Entity>,
}

#[derive(Debug, Clone, Message)]
pub struct PlayerActionEvent {
    pub action: PlayerAction,
}

#[derive(Debug, Clone, Message)]
pub struct DeathEvent {
    pub entity: Entity,
    pub killer: Option<Entity>,
}

pub trait DeathBehavior: Send + Sync + 'static {
    fn on_death(
        &self,
        entity: Entity,
        killer: Option<Entity>,
        commands: &mut Commands,
        loot_writer: &mut MessageWriter<LootEvent>,
        xp_writer: &mut MessageWriter<AwardXpEvent>,
        tm: &mut TurnManager,
    );
}

pub struct EnemyDeathBehavior {
    pub xp_reward: u32,
    pub loot_table: Vec<LootItem>,
}

impl DeathBehavior for EnemyDeathBehavior {
    fn on_death(
        &self,
        entity: Entity,
        killer: Option<Entity>,
        commands: &mut Commands,
        loot_writer: &mut MessageWriter<LootEvent>,
        xp_writer: &mut MessageWriter<AwardXpEvent>,
        tm: &mut TurnManager,
    ) {
        // Remove from combat
        tm.participants.retain(|e| *e != entity);

        // Drop loot
        loot_writer.send(LootEvent {
            loot: self.loot_table.clone(),
            dropped_by: entity,
        });

        // Award XP to killer if exists
        if let Some(killer) = killer {
            xp_writer.send(AwardXpEvent {
                recipient: killer,
                amount: self.xp_reward,
            });
        }

        // Optional: despawn corpse or mark dead
        commands.entity(entity).insert(Dead);
    }
}

impl DeathBehavior for AllyDeathBehavior {
    fn on_death(
        &self,
        entity: Entity,
        killer: Option<Entity>,
        commands: &mut Commands,
        _loot_writer: &mut MessageWriter<LootEvent>,
        _xp_writer: &mut MessageWriter<AwardXpEvent>,
        tm: &mut TurnManager,
    ) {
        // Remove from turn order
        tm.participants.retain(|e| *e != entity);

        // Mark dead
        commands.entity(entity).insert(Dead);

        // Optional: trigger special ally-death effects (morale drop, buffs)
        info!("An ally has fallen.");
    }
}

fn award_xp_system(
    mut events: MessageReader<AwardXpEvent>,
    mut events_level: MessageWriter<LevelUpEvent>,
    mut query: Query<(&mut Experience, &mut Level)>,
) {
    for evt in events.read() {
        if let Ok((mut xp, mut lvl)) = query.get_mut(evt.recipient) {
            xp.0 += evt.amount;
            let new_level = (xp.0 >> 16) as u8;
            events_level.send(LevelUpEvent {
                who: evt.recipient,
                old_level: lvl.0,
                new_level,
            });
        }
    }
}

fn loot_system(
    mut events: MessageReader<LootEvent>,
    mut loot_res: ResMut<BattleLoot>,
) {
    for evt in events.read() {
        loot_res.items.extend(evt.loot.clone());
    }
}


#[derive(Clone, Debug, Component)]
pub struct ExtraHp {
    pub current: u32,
    pub max: u32,
}

fn spirit_medium_absorb_system(
    mut incoming: MessageReader<IncomingDamageEvent>,
    mut q: Query<(&mut ExtraHp, &mut Health), With<SpiritMediumBehavior>>,
    mut dmg_queue: ResMut<DamageQueue>,
) {
    for ev in incoming.iter() {
        if let Ok((mut extra, mut hp)) = q.get_mut(ev.target) {

            let mut dmg = ev.amount;

            // absorb from extra hp
            let absorbed = dmg.min(extra.current);
            extra.current -= absorbed;
            dmg -= absorbed;

            if dmg == 0 {
                dmg_queue.push(DamageEvent {
                    attacker: ev.attacker,
                    target: ev.target,
                    amount: 0,
                    damage_type: ev.damage_type,
                });
                continue;
            }

            // apply remaining to normal HP
            let applied = hp.current.min(dmg);
            hp.current -= applied;

            dmg_queue.push(DamageEvent {
                attacker: ev.attacker,
                target: ev.target,
                amount: applied,
                damage_type: ev.damage_type,
            });
        }
    }
}

fn paladin_before_attack_system(
    mut events: MessageReader<BeforeAttackEvent>,
    mut paladins: Query<(), With<PaladinBehavior>>,
) {
    for ev in events.iter_mut() {
        if paladins.get(ev.attacker).is_ok() {
            ev.attack_stats.hit =
                (ev.attack_stats.hit as f32 * 1.10) as u32;
        }
    }
}

fn paladin_damage_reduction_system(
    mut incoming: MessageReader<IncomingDamageEvent>,
    paladins: Query<(), With<PaladinBehavior>>,
    mut dmg_queue: ResMut<DamageQueue>,
) {
    for ev in incoming.iter() {
        if paladins.get(ev.target).is_ok() {
            let reduced = ev.amount.saturating_sub(1);
            dmg_queue.push(DamageEvent {
                attacker: ev.attacker,
                target: ev.target,
                amount: reduced,
                damage_type: ev.damage_type,
            });
        }
    }
}

fn rogue_backstab_system(
    mut events: MessageReader<BeforeAttackEvent>,
    rogues: Query<&Transform, With<RogueBehavior>>,
    targets: Query<&Transform>,
) {
    for ev in events.iter_mut() {
        if let Ok(rogue_tf) = rogues.get(ev.attacker) {
            if let Ok(target_tf) = targets.get(ev.target) {
                let dir = target_tf.translation - rogue_tf.translation;
                let back = target_tf.forward();

                if dir.length() < 2.0 && dir.dot(back) > 0.8 {
                    ev.attack_stats.lethality += 20;
                }
            }
        }
    }
}

fn rogue_dodge_system(
    mut incoming: MessageReader<IncomingDamageEvent>,
    rogues: Query<&CombatStats, With<RogueBehavior>>,
    mut dmg_queue: ResMut<DamageQueue>,
) {
    for ev in incoming.iter() {
        if let Ok(stats) = rogues.get(ev.target) {
            let chance = stats.agility as f32 / 100.0;
            if rand::random::<f32>() < chance {
                // Dodged → send 0 damage
                dmg_queue.push(DamageEvent {
                    attacker: ev.attacker,
                    target: ev.target,
                    amount: 0,
                    damage_type: ev.damage_type,
                });
                continue;
            }
        }

        // not dodged → push normal damage
        dmg_queue.push(DamageEvent {
            attacker: ev.attacker,
            target: ev.target,
            amount: ev.amount,
            damage_type: ev.damage_type,
        });
    }
}


/// -----------------------------
/// Systems
/// -----------------------------

/// Generic equipment system: reacts to BeforeAttackEvent and applies stat modifiers when equipment has matching hooks.
fn equipment_before_attack_listener(
    mut befores: MessageReader<BeforeAttackEvent>,
    equipment_q: Query<(Entity, &Equipment, &EquipmentHooks)>,
    slots_q: Query<&EquipmentSlots>,
    mut commands: Commands,
) {
    for ev in befores.iter() {
        // find equipment in attacker's slots
        if let Ok(slots) = slots_q.get(ev.attacker) {
            // weapon
            if let Some(weapon_entity) = slots.weapon {
                if let Ok((equip_entity, _equip, hooks)) = equipment_q.get(weapon_entity) {
                    for hook in &hooks.0 {
                        match hook {
                            EquipHook::BeforeAttackMultiplier {
                                stat,
                                multiplier,
                                duration_turns,
                            } => {
                                // add a temporary StatModifier to the attacker
                                commands.entity(ev.attacker).insert(if let Some(mut sm) = commands
                                    .get_entity(ev.attacker)
                                    .and_then(|e| e.get::<StatModifiers>().cloned())
                                {
                                    // This code path is not ideal since Commands does not give easy get() at runtime.
                                    // Instead we always push a new StatModifiers component with the modifier appended.
                                    StatModifiers(vec![StatModifier {
                                        stat: *stat,
                                        multiplier: *multiplier,
                                        expires_in_turns: Some(*duration_turns),
                                        source: Some(equip_entity),
                                    }])
                                } else {
                                    StatModifiers(vec![StatModifier {
                                        stat: *stat,
                                        multiplier: *multiplier,
                                        expires_in_turns: Some(*duration_turns),
                                        source: Some(equip_entity),
                                    }])
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
            // TODO: check armor and accessories similarly
        }
    }
}

/// After all BeforeAttack listeners ran, we push an AttackExecuteEvent so the pipeline continues
fn before_to_execute(
    mut befores: MessageReader<BeforeAttackEvent>,
    mut execs: MessageWriter<AttackExecuteEvent>,
) {
    for ev in befores.iter() {
        execs.send(AttackExecuteEvent {
            attacker: ev.attacker,
            target: ev.target,
            context: ev.context.clone(),
        });
    }
}

/// BeforeHit listeners: weapons or buffs may add flat damage or additional multipliers
fn before_hit_listeners(
    mut executes: MessageReader<AttackExecuteEvent>,
    mut before_hits: MessageWriter<BeforeHitEvent>,
) {
    for ev in executes.iter() {
        // For now, forward to BeforeHitEvent; systems can mutate context by listening here (we pass clone)
        before_hits.send(BeforeHitEvent {
            attacker: ev.attacker,
            target: ev.target,
            context: ev.context.clone(),
        });
    }
}

/// Execute the hit: compute damage using CombatStats + StatModifiers + context
// fn execute_hit_system(
//     mut before_hits: MessageReader<BeforeHitEvent>,
//     mut damage_writer: MessageWriter<DamageEvent>,
//     stats_q: Query<&CombatStats>,
//     modifiers_q: Query<&StatModifiers>,
// ) {
//     for ev in before_hits.iter() {
//         // base lethality from context (usually came from attacker's stats)
//         let mut base_leth = ev.context.base_lethality;
//         let base_hit = ev.context.base_hit;
//         let mut flat = ev.context.extra_flat_damage;

//         // Apply stat modifiers for attacker, multiplicatively
//         if let Ok(mods) = modifiers_q.get(ev.attacker) {
//             for m in &mods.0 {
//                 if m.stat == Stat::Lethality {
//                     base_leth = ((base_leth as f32) * m.multiplier).round() as i32;
//                 }
//                 if m.stat == Stat::Hit {
//                     // not used here, but you can transform to hit chance later
//                 }
//             }
//         }

//         // Combine with attacker's combat stats if needed
//         if let Ok(att_stats) = stats_q.get(ev.attacker) {
//             base_leth += att_stats.base_lethality - ev.context.base_lethality; // ensure differences considered
//         }

//         // A very simple damage formula: final = base_leth + flat
//         let final_damage = (base_leth + flat).max(0);

//         damage_writer.send(DamageEvent {
//             attacker: ev.attacker,
//             target: ev.target,
//             amount: final_damage,
//             damage_type: DamageType::Physical,
//         });
//     }
// }

/// Process AttackIntentEvent -> send BeforeAttackEvent
fn process_attack_intent(
    mut dq: ResMut<DamageQueue>,
    mut intents: MessageReader<AttackIntentEvent>,
    stats_q: Query<&CombatStats>,
    modifiers_q: Query<&StatModifiers>,
    targets_stats_q: Query<&CombatStats>,  // to read target agility for hit roll
    health_q: Query<&Health>,              // for any stat scaling needing HP
    magic_q: Query<&Magic>,
    stamina_q: Query<&Stamina>,
) {
    for intent in intents.iter() {
        let attacker = intent.attacker;
        let target = intent.target;

        // Build base context from attacker's CombatStats (or ability context if present)
        let att_stats = stats_q.get(attacker).ok();
        let mut base_leth = att_stats.map(|s| s.base_lethality as i32).unwrap_or(0);
        let base_hit = att_stats.map(|s| s.base_hit as i32).unwrap_or(50);
        let mut flat = intent.context.extra_flat_damage as i32; // keep flat

        // Apply attacker's StatModifiers multiplicatively for attacker-side scaling
        if let Ok(mods) = modifiers_q.get(attacker) {
            for m in &mods.0 {
                match m.stat {
                    Stat::Lethality => {
                        base_leth = ((base_leth as f32) * m.multiplier).round() as i32;
                    }
                    Stat::Hit => {
                        // we don't change base_leth here, but we might store to adjust accuracy
                    }
                    Stat::Agility | Stat::Mind | Stat::Morale => {
                        // If your modifiers can scale other stats used for scaling,
                        // handle them here (optional).
                    }
                    _ => {}
                }
            }
        }

        // If this intent was produced by an Ability, examine ability.effects
        // and collect scaled_with / defended_with; otherwise use defaults for a basic attack.
        let mut scaled_with: Vec<(Stat, f32)> = Vec::new();
        let mut defended_with: Vec<(Stat, f32)> = Vec::new();
        if let Some(ability) = intent.ability.as_ref() {
            // gather scaling/defense info from ability.effects
            for eff in &ability.effects {
                match eff {
                    AbilityEffect::Damage { scaled_with: s, defended_with: d, .. } => {
                        scaled_with.push((*s, 1.0));   // default multiplier 1.0 unless ability includes one
                        defended_with.push((*d, 1.0));
                    }
                    AbilityEffect::Heal { .. } => { /* skip */ }
                    AbilityEffect::Buff { .. } => { /* skip */ }
                }
            }
        }

        // Default for normal attack if none specified
        if scaled_with.is_empty() {
            scaled_with.push((Stat::Lethality, 1.0));
        }
        if defended_with.is_empty() {
            // physical attacks are typically defended by armor (or base_armor)
            defended_with.push((Stat::Armor, 1.0));
        }

        // APPLY ATTACKER-SIDE SCALING IMMEDIATELY (so amount is pre-defense):
        // For each (stat, mult) in scaled_with add attacker_stat * mult to base_leth
        if let Some(a_stats) = att_stats {
            for (stat, mult) in &scaled_with {
                let val = get_stat_value(*stat, Some(a_stats), None, None, None);
                // apply rounding and scale factor — tweak divisor as needed
                base_leth += (val as f32 * *mult / 10.0).round() as i32;
                // NOTE: dividing by 10 here prevents massive values; tune to taste.
            }
        }

        // Calculate final pre-defense damage (base lethality + flat)
        let pre_def_damage = (base_leth + flat).max(0);

        // --- HIT CHANCE (do this now, using target agility) -----------------------
        // Use attacker hit vs target agility for miss roll
        let attacker_hit_f = base_hit as f32;
        let target_agi_f = targets_stats_q.get(target).map(|t| t.base_agility as f32).unwrap_or(0.0);

        // chance formula: hit / (hit + agility)
        let chance = attacker_hit_f / (attacker_hit_f + target_agi_f + 0.0001);

        if rand::random::<f32>() > chance {
            // It's a miss — push a MISS special entry or skip pushing damage.
            dq.0.push(QueuedDamage {
                attacker,
                target,
                amount: DamageSignal::Miss as i32,
                damage_type: DamageType::Physical,
                scaled_with: vec![],
                defended_with: vec![],
                accuracy_override: None,
                tags: vec![],
            });
            continue;
        }

        // If hit, push the pre-defense damage into the queue together with the defender-side stats
        dq.0.push(QueuedDamage {
            attacker,
            target,
            amount: pre_def_damage,
            damage_type: intent.context.damage_type.unwrap_or(DamageType::Physical),
            scaled_with,   // we keep this as metadata, though we've already applied them
            defended_with, // used by the damage processor to subtract defenses
            accuracy_override: None,
            tags: vec![ intent.ability.as_ref().map(|a| a.id).unwrap_or(0) ],
        });
    }
}


fn process_damage_queue_system(
    mut dq: ResMut<DamageQueue>,
    stats_q: Query<&CombatStats>,
    hp_q: Query<&Health>,
    mp_q: Query<&Magic>,
    mut damage_writer: MessageWriter<DamageEvent>,
) {
    for mut entry in dq.0.drain(..) {
        // SPECIAL NEGATIVE VALUES -------------------------------------------
        match entry.amount {
            -1 => continue, // MISS
            -2 => continue, // DODGE
            -3 => {         // HITKILL
                damage_writer.send(DamageEvent {
                    attacker: entry.attacker,
                    target: entry.target,
                    amount: u32::MAX,
                    damage_type: entry.damage_type,
                });
                continue;
            }
            // If less than 0 but not one of the above, it's an error
            _ => {}
        }

        // FETCH STATS --------------------------------------------------------
        let atk = stats_q.get(entry.attacker).ok();
        let tgt = stats_q.get(entry.target).ok();
        let tgt_hp = hp_q.get(entry.target).ok();
        let tgt_mp = mp_q.get(entry.target).ok();

        // HIT CHANCE ---------------------------------------------------------
        let hit = entry.accuracy_override
            .or_else(|| atk.map(|a| a.base_hit as f32))
            .unwrap_or(50.0);

        let evade = tgt.map(|t| t.base_agility as f32).unwrap_or(0.0);

        let chance = hit / (hit + evade + 0.01);

        if rand::random::<f32>() > chance {
            continue; // MISS
        }

        // SCALING ------------------------------------------------------------
        if let Some(a) = atk {
            for (stat, mult) in &entry.scaled_with {
                entry.amount += (get_stat_value(*stat, a, tgt_hp, tgt_mp) as f32 * mult) as i32;
            }
        }

        // DEFENSE -------------------------------------------------------------
        if let Some(t) = tgt {
            for (stat, mult) in &entry.defended_with {
                entry.amount -= (get_stat_value(*stat, t, tgt_hp, tgt_mp) as f32 * mult) as i32;
            }
        }

        entry.amount = entry.amount.max(0);

        // FINAL DAMAGE --------------------------------------------------------
        damage_writer.send(DamageEvent {
            attacker: entry.attacker,
            target: entry.target,
            amount: entry.amount as u32,
            damage_type: entry.damage_type,
        });
    }
}


/// Apply damage to target's Health and emit AfterHitEvent
fn apply_damage_system(
    mut reader: MessageReader<DamageEvent>,
    mut health_q: Query<&mut Health>,
    mut after_writer: MessageWriter<AfterHitEvent>,
    mut death_writer: MessageWriter<DeathEvent>,
) {
    for ev in reader.iter() {
        if let Ok(mut hp) = health_q.get_mut(ev.target) {
            let before = hp.current;
            hp.current = hp.current.saturating_sub(ev.amount);
            let applied = before - hp.current;

            after_writer.send(AfterHitEvent {
                attacker: ev.attacker,
                target: ev.target,
                amount: applied,
                damage_type: ev.damage_type,
            });

            if hp.current == 0 {
                death_writer.send(DeathEvent {
                    entity: ev.target,
                    killer: Some(ev.attacker),
                });
            }
        }
    }
}


/// After hit: allow equipment or buffs to react (e.g., lifesteal)
fn after_hit_listeners(
    mut after_hits: MessageReader<AfterHitEvent>,
    mut after_attack_writer: MessageWriter<AfterAttackEvent>,
) {
    for ev in after_hits.iter() {
        // Could apply on-hit effects here
        after_attack_writer.send(AfterAttackEvent {
            attacker: ev.attacker,
            target: ev.target,
            context: AttackContext::default(),
        });
    }
}

/// Cleanup after attack (final stage)
fn after_attack_finalizers(mut after_attacks: MessageReader<AfterAttackEvent>) {
    for ev in after_attacks.iter() {
        info!("AfterAttack: attacker={:?}, target={:?}", ev.attacker, ev.target);
        // Trigger visual effects, animations, etc. from here
    }
}

/// Buff tick system: decrease durations and remove expired modifiers/buffs
fn buff_tick_system(
    mut commands: Commands,
    mut query_mods: Query<(Entity, &mut StatModifiers)>,
    mut query_buffs: Query<(Entity, &mut Buff)>,
) {
    // Decrease stat modifiers' expires_in_turns, remove those with 0
    for (entity, mut mods) in query_mods.iter_mut() {
        let mut keep = Vec::new();
        for mut m in mods.0.drain(..) {
            if let Some(mut turns) = m.expires_in_turns {
                if turns > 0 {
                    turns -= 1;
                    if turns > 0 {
                        m.expires_in_turns = Some(turns);
                        keep.push(m);
                    } else {
                        // expired: drop it
                    }
                } else {
                    // already expired
                }
            } else {
                // permanent: keep
                keep.push(m);
            }
        }
        // reinsert kept modifiers
        commands.entity(entity).insert(StatModifiers(keep));
    }

    // Decrease buffs
    for (entity, mut buff) in query_buffs.iter_mut() {
        if buff.remaining_turns > 0 {
            buff.remaining_turns -= 1;
            if buff.remaining_turns == 0 {
                // remove the buff entity
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Simple regeneration system (health/magic/stamina)
fn regen_system(mut qh: Query<&mut Health>, qm: Query<&mut Magic>, qs: Query<&mut Stamina>) {
    for mut h in qh.iter_mut() {
        h.current = (h.current + h.regen).min(h.max);
    }
    for mut m in qm.iter_mut() {
        m.current = (m.current + m.regen).min(m.max);
    }
    for mut s in qs.iter_mut() {
        s.current = (s.current + s.regen).min(s.max);
    }
}

/// Example AI system that makes a simple attack intent for demo
fn demo_ai_system(
    mut intents: MessageWriter<AttackIntentEvent>,
    query_chars: Query<Entity, With<CombatStats>>,
) {
    // Very naive: if there are at least two entities, make one attack the next
    let ents: Vec<Entity> = query_chars.iter().collect();
    if ents.len() >= 2 {
        // pick two distinct entities
        let attacker = ents[0];
        let target = ents[1];
        // send intent (this would normally be based on AIParameters)
        intents.send(AttackIntentEvent {
            attacker,
            target,
            ability_id: None,
        });
    }
}


/// -----------------------------
/// Turn Manager resource
/// -----------------------------

/// Holds the current computed turn order (queue of entities ready to act).
#[derive(Resource, Default)]
pub struct TurnOrder {
    pub queue: VecDeque<Entity>,
}

/// Resource that knows which entities should participate in turn calc.
/// For simplicity we store a Vec<Entity> that is maintained at spawn time.
#[derive(Resource, Default)]
pub struct TurnManager {
    pub participants: Vec<Entity>,
    pub turn_threshold: u32,
    pub maximum_value: u32, // random jitter max
}

impl TurnManager {
    pub fn recompute_params(&mut self, stats_q: &Query<&CombatStats>, levels_q: &Query<&Level>) {
        // compute avg agility and avg level across participants that still exist
        let mut total_agility: u32 = 0;
        let mut total_level: u32 = 0;
        let mut count: u32 = 0;
        for &e in &self.participants {
            if let Ok(stats) = stats_q.get(e) {
                total_agility += stats.base_agility.max(0) as u32;
                if let Ok(level) = levels_q.get(e) {
                    total_level += level.0 as u32;
                } else {
                    total_level += 1; // default level if missing
                }
                count += 1;
            }
        }
        if count == 0 {
            self.turn_threshold = 100; // fallback
            self.maximum_value = 10;
            return;
        }
        let avg_agility = (total_agility / count).max(1);
        self.turn_threshold = avg_agility * 2; // original used <<1
        let avg_level = (total_level / count).max(1);
        self.maximum_value = avg_level << 3; // original used <<3
    }

    /// Calculate a precise turn order based on accumulated agility.
    /// For each participant:
    ///   accumulated += base_agility + rand(0..maximum_value)
    ///   while accumulated >= turn_threshold: push to order and subtract threshold
    pub fn calculate_turn_order(
        &mut self,
        mut acc_q: &mut Query<&mut AccumulatedAgility>,
        stats_q: &Query<&CombatStats>,
    ) -> Vec<Entity> {
        let mut rng = rand::rng();
        let mut order: Vec<Entity> = Vec::new();

        // iterate participants in stable order
        for &entity in &self.participants {
            // get accumulated agility, if missing insert default (0)
            if let Ok(mut acc) = acc_q.get_mut(entity) {
                let agility = stats_q
                    .get(entity)
                    .map(|s| s.base_agility.max(0) as u32)
                    .unwrap_or(0);
                let jitter: u32 = if self.maximum_value > 0 {
                    rng.gen_range(0..self.maximum_value)
                } else {
                    0
                };

                let mut current = acc.0;
                // add base agility + random jitter
                current = current.saturating_add(agility).saturating_add(jitter);
                // while enough to take a turn
                while current >= self.turn_threshold && self.turn_threshold > 0 {
                    current = current.saturating_sub(self.turn_threshold);
                    order.push(entity);
                }
                acc.0 = current;
            } else {
                // entity missing AccumulatedAgility -> skip or insert?
                // We skip; spawn-time code should ensure AccumulatedAgility exists for participants.
            }
        }
        order
    }
}

/// -----------------------------
/// Systems: XP / Leveling
/// -----------------------------

/// Calculate XP awarded given enemy_experience and receiver_experience (from original formula).
/// This mirrors your original approach but with safe floating arithmetic and guards.
fn calculate_xp_award(receiver_experience: u32, enemy_experience: u32) -> u32 {
    // original used ratio = enemy_experience / self.experience
    // guard: if receiver_experience == 0, treat ratio as 1.0 to avoid div-by-zero
    let ratio: f32 = if receiver_experience == 0 {
        // if receiver has 0 xp, award something small proportional to enemy XP
        (enemy_experience as f32) / 1.0f32
    } else {
        (enemy_experience as f32) / (receiver_experience as f32)
    };

    // thresholds from original: shift left 14 (= *16384)
    // let multiplier_base = 16384.0_f32;

    // clamp ratio to avoid NaNs
    let ratio = ratio.max(0.0);

    let amount_f: f32 = if ratio > 0.946 {
        // ((ratio - 0.2).ln() / 1.25.ln() + 1.5) << 14  converted to *16384
        let inner = (ratio - 0.2).max(0.0001);
        let value = ((inner.ln() / 1.25f32.ln()) + 1.5) << 14;
        // clamp to non-negative
        value.max(0.0)
    } else {
        // ratio.powf(30.2) << 14
        ratio.powf(30.2) << 14
    };

    // avoid huge values; clamp to u32::MAX-1
    if amount_f.is_nan() || amount_f <= 0.0 {
        0
    } else if amount_f >= (u32::MAX as f32) {
        u32::MAX - 1
    } else {
        amount_f.round() as u32
    }
}

/// Level up handler: apply stat increases using functions derived from original file.
/// The original used strange formulas; here we approximate the same behavior while keeping types safe.
/// Each function will increase appropriate components.
fn curve_growth_u32(attr: u8, base: f32, exponent: f32) -> u32 {
    // step 1: left shift the attribute by 1 (u8 -> u32 then shift)
    let shifted_attr = ((attr as u32) << 1) as f32; // matches option B

    // step 2: compute power safely
    let power = shifted_attr.powf(exponent);

    // step 3: compute inner
    let inner = base - power;

    // step 4: clamp negative to zero; convert to u64 for safe shifting
    let truncated: u64 = if inner.is_nan() || inner <= 0.0 {
        0u64
    } else {
        inner as u64
    };

    // step 5: right shift by 19 (like your original '>> 19')
    let shifted_right: u32 = (truncated >> 19) as u32;

    // step 6: compute minimum allowed growth (base >> 3)
    let min_growth: u32 = ((base as u32) >> 3).max(1);

    std::cmp::max(shifted_right, min_growth)
}

/// Similar helper but returning signed i32 (for stats that are i32)
fn curve_growth_i32(attr: u8, base: f32, exponent: f32) -> i32 {
    curve_growth_u32(attr, base, exponent) as i32
}

/// --------------- Level up system using your confirmed parameters ---------------

/// Event: LevelUpEvent { who: Entity, old_level: u8, new_level: u8 }
/// (assumes you already defined LevelUpEvent elsewhere and registered it)
pub fn level_up_system(
    mut level_up_events: MessageReader<LevelUpEvent>,
    mut q_stats: Query<(
        &mut CombatStats,
        Option<&mut Health>,
        Option<&mut Stamina>,
        Option<&mut Magic>,
        &GrowthAttributes,
        // Keep GrowthCurve in the signature if you want to keep per-character curves later.
        Option<&GrowthCurve>,
    )>,
) {

    // With base of 500, 4.20927 goes to 50, 3.65860 goes to 100, 3.39852 goes to 150, 3.23534 goes to 200, 3.11917 goes to 250, 3.03027 goes to 300, 2.95896 goes to 350, 2.89986 goes to 400, 2.84964 goes to 450, 2.80618 goes to 500
    // With base of 375, 4.14680 goes to 50, 3.60423 goes to 100, 3.34808 goes to 150, 3.18732 goes to 200, 3.07288 goes to 250, 2.98530 goes to 300, 2.91505 goes to 350, 2.85682 goes to 400, 2.80736 goes to 450, 2.76453 goes to 500
    // With base of 250, 4.05875 goes to 50, 3.52777 goes to 100, 3.2699 goes to 150, 3.11965 goes to 200, 3.00763 goes to 250, 2.92191 goes to 300, 2.85316 goes to 350, 2.79616 goes to 400, 2.74775 goes to 450, 2.70584 goes to 500
    // With base of 175, 3.98130 goes to 50, 3.46045 goes to 100, 3.21446 goes to 150, 3.06012 goes to 200, 2.95024 goes to 250, 2.86616 goes to 300, 2.79871 goes to 350, 2.74280 goes to 400, 2.69531 goes to 450, 2.65420 goes to 500
    // With base of 100, 3.85978 goes to 50, 3.35483 goes to 100, 3.11635 goes to 150, 2.96671 goes to 200, 2.86019 goes to 250, 2.77867 goes to 300, 2.71329 goes to 350, 2.65909 goes to 400, 2.61305 goes to 450, 2.57319 goes to 500
    // With base of 50, 5,70205 goes to 10, 4,36649 goes to 25, 3.70927 goes to 50, 3.22401 goes to 100, 2.99482 goes to 150, 2.85103 goes to 200, 2.74866 goes to 250, 2.67032 goes to 300, 2.60748 goes to 350, 2.55539 goes to 400, 2.51115 goes to 450, 2.47285 goes to 500
    // With base of 25, 5,47067 goes to 10, 4,18931 goes to 25, 3.55875 goes to 50, 3.09318 goes to 100, 2.87330 goes to 150, 2.73534 goes to 200, 2.63712 goes to 250, 2.56196 goes to 300, 2.50167 goes to 350, 2.45170 goes to 400, 2.40925 goes to 450, 2.37250 goes to 500
    // With base of 10, 5,16481 goes to 10, 3,95508 goes to 25, 3.35978 goes to 50, 2.92024 goes to 100, 2.71265 goes to 150, 2.58240 goes to 200, 2.48968 goes to 250, 2.41872 goes to 300, 2.36181 goes to 350, 2.31463 goes to 400, 2.27455 goes to 450, 2.23986 goes to 500
    // There is a spreadsheet with all the values for initial value and maximum value

    for ev in level_up_events.iter() {
        if let Ok((mut stats, h_opt, s_opt, m_opt, growth_attr, _curve_opt)) =
            q_stats.get_mut(ev.who)
        {
            let level_gained = (ev.new_level as i32) - (ev.old_level as i32);
            if level_gained <= 0 {
                continue;
            }

            for _ in 0..level_gained {
                // -----------------------
                // HEALTH MAX & REGEN
                // -----------------------
                if let Some(mut h) = h_opt {
                    // Health Max using your provided sample (base=250, exponent=3.007632509)
                    let base_hp = 250.0_f32;
                    let add_hp = curve_growth_u32(
                        growth_attr.vitality,
                        base_hp,
                        3.007632509_f32,
                    );
                    // apply
                    h.max = h.max.saturating_add(add_hp as i32);
                    h.current = h.current.saturating_add(add_hp as i32);

                    // Health regen using base=35, exponent=2.691262945
                    let base_regen = 35.0_f32;
                    let add_regen = curve_growth_u32(
                        growth_attr.vitality,
                        base_regen,
                        2.691262945_f32,
                    );
                    // regen is an integer; apply
                    h.regen = h.regen.saturating_add(add_regen as i32);
                }

                // -----------------------
                // STAMINA MAX & REGEN
                // -----------------------
                if let Some(mut s) = s_opt {
                    // Use the standardized formula with the chosen base/exponent for stamina
                    let base_stamina = 200.0_f32; // as you confirmed
                    let add_stam = curve_growth_u32(growth_attr.endurance, base_stamina, 2.9_f32);
                    s.max = s.max.saturating_add(add_stam as i32);
                    s.current = s.current.saturating_add(add_stam as i32);

                    // Stamina regen with base 25 exponent 2.4
                    let base_stam_reg = 25.0_f32;
                    let add_stam_reg = curve_growth_u32(growth_attr.endurance, base_stam_reg, 2.4_f32);
                    s.regen = s.regen.saturating_add(add_stam_reg as i32);
                }

                // -----------------------
                // MAGIC MAX & REGEN
                // -----------------------
                if let Some(mut m) = m_opt {
                    let base_magic = 225.0_f32;
                    let add_magic = curve_growth_u32(growth_attr.spirit, base_magic, 3.1_f32);
                    m.max = m.max.saturating_add(add_magic as i32);
                    m.current = m.current.saturating_add(add_magic as i32);

                    let base_magic_reg = 30.0_f32;
                    let add_magic_reg = curve_growth_u32(growth_attr.spirit, base_magic_reg, 2.8_f32);
                    m.regen = m.regen.saturating_add(add_magic_reg as i32);
                }

                // -----------------------
                // COMBAT STATS (LETHALITY, HIT, AGILITY, MIND, MORALE)
                // All follow the same pattern with base=250, exponent=3.0
                // -----------------------

                // Lethality
                let add_leth = curve_growth_i32(growth_attr.power, 250.0_f32, 3.0_f32);
                stats.base_lethality = stats.base_lethality.saturating_add(add_leth);

                // Hit
                let add_hit = curve_growth_i32(growth_attr.control, 250.0_f32, 3.0_f32);
                stats.base_hit = stats.base_hit.saturating_add(add_hit);

                // Agility
                let add_agi = curve_growth_i32(growth_attr.agility, 250.0_f32, 3.0_f32);
                stats.base_agility = stats.base_agility.saturating_add(add_agi);

                // Mind
                let add_mind = curve_growth_i32(growth_attr.insight, 250.0_f32, 3.0_f32);
                stats.base_mind = stats.base_mind.saturating_add(add_mind);

                // Morale
                let add_morale = curve_growth_i32(growth_attr.resolve, 250.0_f32, 3.0_f32);
                stats.base_morale = stats.base_morale.saturating_add(add_morale);
            }

            info!(
                "Level up applied to {:?}: {} -> {}",
                ev.who, ev.old_level, ev.new_level
            );
        }
    }
}

pub fn respec_system(
    mut ev_respec: MessageReader<RespecEvent>,
    mut q: Query<(
        &mut GrowthAttributes,
        &mut AttributePointPool,
        Option<&GrowthCurve>,
    )>,
) {
    for ev in ev_respec.read() {
        if let Ok((mut attributes, mut pool, _curve)) = q.get_mut(ev.who) {
            
            // 1. Calculate how many points were allocated
            let total_spent = attributes.vitality as u32
                + attributes.endurance as u32
                + attributes.spirit as u32
                + attributes.power as u32
                + attributes.control as u32
                + attributes.agility as u32
                + attributes.insight as u32
                + attributes.resolve as u32;

            // 2. Reset attributes (full reset)
            if ev.full_reset {
                *attributes = GrowthAttributes::default();
            } else {
                // partial reset? (implement if needed)
                // For now full reset always.
                *attributes = GrowthAttributes::default();
            }

            // 3. Refund points
            if ev.refund_all_points {
                pool.available += total_spent;
                pool.spent = 0;
            }

            info!(
                "Character {:?} RESET. Refunded {} points. Now has {} available.",
                ev.who,
                total_spent,
                pool.available
            );
        }
    }
}

/// -----------------------------
/// Systems: Turn manager & Turn order calculation
/// -----------------------------

/// A system that ensures TurnManager participants are kept in sync with spawned characters.
/// Call this whenever you spawn or despawn participants.
fn register_participants_system(
    mut tm: ResMut<TurnManager>,
    query_chars: Query<Entity, With<CombatStats>>,
) {
    // simple strategy: replace participants with all entities that have CombatStats
    tm.participants = query_chars.iter().collect();
}

/// Calculate turn order each "tick" (you may call this on a schedule or when you want a fresh order)
fn compute_turn_order_system(
    mut tm: ResMut<TurnManager>,
    mut turn_order: ResMut<TurnOrder>,
    mut acc_q: Query<&mut AccumulatedAgility>,
    stats_q: Query<&CombatStats>,
    levels_q: Query<&Level>,
    mut ev_writer: MessageWriter<TurnOrderCalculatedEvent>,
    ev_reader: MessageReader<RoundEndEvent>,
) {
    // recompute threshold / max jitter based on participants
    tm.recompute_params(&stats_q, &levels_q);

    // Important: make acc_q mutable borrow optional; pass as &mut Query below
    // But in bevy we cannot pass &mut Query into resource functions; we call method and use acc_q directly
    // We'll call calculate_turn_order in-place:
    let mut order_vec: Vec<Entity> = Vec::new();
    // Create a temporary mutable reference to acc_q by using the Query directly
    // call tm.calculate_turn_order(mut acc_q, &stats_q)
    // Unfortunately we cannot pass Query into a method expecting &mut Query, so inline behavior here:

    let mut rng = rand::rng();
    for &entity in &tm.participants {
        if let Ok(mut acc) = acc_q.get_mut(entity) {
            let agility = stats_q.get(entity).map(|s| s.base_agility.max(0) as u32).unwrap_or(0);
            let jitter: u32 = if tm.maximum_value > 0 { rng.gen_range(0..tm.maximum_value) } else { 0 };
            let mut current = acc.0;
            current = current.saturating_add(agility).saturating_add(jitter);
            while current >= tm.turn_threshold && tm.turn_threshold > 0 {
                current = current.saturating_sub(tm.turn_threshold);
                order_vec.push(entity);
            }
            acc.0 = current;
        }
    }

    // place order_vec into TurnOrder queue
    turn_order.queue.clear();
    for e in order_vec {
        turn_order.queue.push_back(e);
    }

    ev_writer.send(TurnOrderCalculatedEvent);
}

/// Splits out the next entity from TurnOrder and emits a TurnStartEvent
fn advance_turn_system(mut turn_order: ResMut<TurnOrder>, mut ev_writer: MessageWriter<TurnStartEvent>) {
    if let Some(next) = turn_order.queue.pop_front() {
        ev_writer.send(TurnStartEvent { who: next });
    }
    else{
        ev_writer.send(RoundEndEvent);
    }
}

/// Example: when a turn starts for an entity, we allow AI or player to emit AttackIntentEvent.
/// For simplicity demo AI will fire an intent against any other participant.
fn on_turn_start_system(
    mut ev_reader: MessageReader<TurnStartEvent>,
    q_participants: Query<Entity, With<CombatStats>>,
    mut intent_writer: MessageWriter<AttackIntentEvent>,
) {
    for ev in ev_reader.iter() {
        // simple demo: find first entity different from ev.who and issue attack
        let mut target_opt: Option<Entity> = None;
        for e in q_participants.iter() {
            if e != ev.who {
                target_opt = Some(e);
                break;
            }
        }
        if let Some(target) = target_opt {
            intent_writer.send(AttackIntentEvent {
                attacker: ev.who,
                target,
                ability_id: None,
            });
        }
    }
}

fn process_player_action_system(
    mut ev: MessageReader<PlayerActionEvent>,
    mut pending: ResMut<PendingPlayerAction>,
    mut intent_writer: MessageWriter<AttackIntentEvent>,
    mut defend_writer: MessageWriter<DefendIntentEvent>,
    mut wait_writer: MessageWriter<WaitIntentEvent>,
) {
    if pending.entity.is_none() {
        return; // no player turn pending
    }

    let actor = pending.entity.unwrap();

    for e in ev.iter() {
        match &e.action {
            PlayerAction::Attack(target) => {
                intent_writer.send(AttackIntentEvent {
                    attacker: actor,
                    target: *target,
                    ability_id: None,
                });
            }

            PlayerAction::UseAbility(ability_id, target) => {
                intent_writer.send(AttackIntentEvent {
                    attacker: actor,
                    target: *target,
                    ability_id: Some(*ability_id),
                });
            }

            PlayerAction::UseItem(item_id, target) => {
                // TODO: call your item system
            }

            PlayerAction::Defend => {
                defend_writer.send(DefendIntentEvent { who: actor });
            }

            PlayerAction::Wait => {
                wait_writer.send(WaitIntentEvent { who: actor });
            }
        }

        // Player decision consumed → clear pending
        pending.entity = None;
        break; // only one action per turn
    }
}


/// At the end of a turn, we emit TurnEndEvent to allow cleanup and buff ticks if you prefer to tie buff durations to turns.
fn on_turn_end_system(mut ev_reader: MessageReader<TurnEndEvent>, mut _commands: Commands) {
    for ev in ev_reader.iter() {
        info!("Turn ended for {:?}", ev.who);
        // You can do per-turn cleanup here if necessary
    }
}

/// A helper system that consumes TurnOrderCalculatedEvent and then advances the turn automatically.
/// (Optional: you may want to call advance once per frame or per game tick)
fn auto_advance_after_order(
    mut ev_reader: MessageReader<TurnOrderCalculatedEvent>,
    mut turn_order: ResMut<TurnOrder>,
    mut ev_writer: MessageWriter<TurnStartEvent>,
) {
    for _ in ev_reader.iter() {
        if let Some(next) = turn_order.queue.pop_front() {
            ev_writer.send(TurnStartEvent { who: next });
        }
    }
}

/// Buff tick per turn: when a TurnStartEvent occurs for a character, decrement their buff durations (so durations map to turns).
fn buff_tick_on_turn_start_system(
    mut ev_reader: MessageReader<TurnStartEvent>,
    mut query_buffs: Query<(Entity, &mut Buff)>,
    mut commands: Commands,
    mut modifiers_q: Query<(Entity, &mut StatModifiers)>,
) {
    for ev in ev_reader.iter() {
        // Decrement global Buff entities that have source == ev.who (optional design)
        for (entity, mut buff) in query_buffs.iter_mut() {
            if let Some(src) = buff.source {
                if src == ev.who {
                    if buff.remaining_turns > 0 {
                        buff.remaining_turns -= 1;
                        if buff.remaining_turns == 0 {
                            commands.entity(entity).despawn();
                        }
                    }
                }
            }
        }

        // Also decrement StatModifiers on the actor
        if let Ok((entity, mut mods)) = modifiers_q.get_mut(ev.who) {
            let mut keep: Vec<StatModifier> = Vec::new();
            for mut m in mods.0.drain(..) {
                if let Some(turns) = m.expires_in_turns {
                    if turns > 1 {
                        m.expires_in_turns = Some(turns - 1);
                        keep.push(m);
                    } else {
                        // expires now -> drop
                    }
                } else {
                    // permanent -> keep
                    keep.push(m);
                }
            }
            // reinsert updated modifiers
            commands.entity(entity).insert(StatModifiers(keep));
        }
    }
}

/// -----------------------------
/// Minimal Combat pipeline (unchanged core) — only key systems are included here,
/// refer to earlier code for full pipeline. We keep the key entry point systems.
/// -----------------------------


/// -----------------------------
/// Supporting systems
/// -----------------------------


/// Debug print of characters status
fn debug_print_system(
    q: Query<(
        &Name,
        &CharacterId,
        &Health,
        &CombatStats,
        Option<&StatModifiers>,
        Option<&EquipmentSlots>,
        Option<&Level>,
        Option<&Experience>,
        Option<&AccumulatedAgility>,
    )>,
) {
    for (name, id, health, stats, mods, slots, lvl, xp, acc) in q.iter() {
        let level = lvl.map(|l| l.0).unwrap_or(1);
        let xp_val = xp.map(|x| x.0).unwrap_or(0);
        let acc_text = acc.map(|a| a.0.to_string()).unwrap_or_else(|| "N/A".into());
        let mut s = format!(
            "{}({:?}) L{} XP:{} HP: {}/{} Leth:{} Hit:{} Acc:{}",
            name.0, id.0, level, xp_val, health.current, health.max, stats.base_lethality, stats.base_hit, acc_text
        );
        if let Some(mods) = mods {
            if !mods.0.is_empty() {
                s.push_str(&format!(" Mods: {:?}", mods.0));
            }
        }
        if let Some(slots) = slots {
            if slots.weapon.is_some() {
                s.push_str(" WeaponEquipped");
            }
        }
        info!("{}", s);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityEffect {
    Heal { floor: u32, ceiling: u32, scaled_with: Stat },
    Damage { floor: u32, ceiling: u32, damage_type: DamageType, scaled_with: Stat, defended_with: Stat },
    Buff { stat: Stat, multiplier: f32, effects: Option<Vec<u16>>, scaled_with: Stat },   // e.g. "agility", 1.2 multiplier, optional effects (ability ids), doesn't have duration because the ability struct already haves it
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityShape {
    Radius(f32),
    Line { length: f32, thickness: f32 },
    Cone { angle: f32, radius: f32 },
    Select,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ability {
    pub id: u16, // first 8 bits is level, second 8 bits is sub-id
    pub next_id: Option<u16>,
    pub name: String,
    pub health_cost: i32,
    pub magic_cost: i32,
    pub stamina_cost: i32,
    pub cooldown: u8,
    pub description: String,
    pub effects: Vec<AbilityEffect>,
    pub shape: AbilityShape,
    pub duration: u8, // 0 for single turn instantenous, 
    pub targets: u8,
}

impl Ability {
    pub fn get_level(&self) -> u8 { (self.id << 8).try_into().unwrap() }
    pub fn get_sub_id(&self) -> u8 { (self.id >> 8).try_into().unwrap() }
}

//
// === Binary Tree for Abilities ===
//

#[derive(Clone)]
pub struct AbilityNode {
    pub ability: Ability,
    pub left: Option<Arc<RwLock<AbilityNode>>>,
    pub right: Option<Arc<RwLock<AbilityNode>>>,
}

impl AbilityNode {
    pub fn new(ability: Ability) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(AbilityNode {
            ability,
            left: None,
            right: None,
        }))
    }
}

#[derive(Resource, Clone)]
pub struct Ability_Tree(AbilityTree);

pub struct AbilityTree {
    pub root: Option<Arc<RwLock<AbilityNode>>>,
}

impl AbilityTree {
    pub fn new() -> Self {
        AbilityTree { root: None }
    }

    pub fn insert(&mut self, ability: Ability) {
        let node = AbilityNode::new(ability.clone());

        match &self.root {
            None => self.root = Some(node),
            Some(root) => Self::insert_node(root.clone(), node),
        }
    }

    fn insert_node(current: Arc<RwLock<AbilityNode>>, new_node: Arc<RwLock<AbilityNode>>) {
        
        // INSERTION MUST BE MADE WITH THE FIRST SUB-ID OF EACH LEVEL IN ORDER, SO THE ENTIRE TREE IS AT THE LEFT AND THE LEVELS ARE ALL ONE IN THE RIGHT OF THE OTHER
        let new_id = new_node.read().unwrap().ability.id;
        let current_id = current.read().unwrap().ability.id;

        match new_id.cmp(&current_id) {
            Ordering::Less => {
                if let Some(left) = &current.read().unwrap().left {
                    Self::insert_node(left.clone(), new_node);
                } else {
                    current.write().unwrap().left = Some(new_node);
                }
            }
            Ordering::Greater => {
                if let Some(right) = &current.read().unwrap().right {
                    Self::insert_node(right.clone(), new_node);
                } else {
                    current.write().unwrap().right = Some(new_node);
                }
            }
            Ordering::Equal => {
                // duplicate ID; ignore or replace
                current.write().unwrap().ability = new_node.read().unwrap().ability.clone();
            }
        }
    }

    pub fn find(&self, id: u16) -> Option<Ability> {
        Self::find_node(self.root.clone(), id)
    }

    fn find_node(node: Option<Arc<RwLock<AbilityNode>>>, id: u16) -> Option<Ability> {
        if let Some(n) = node {
            let n_borrow = n.read().unwrap();
            if id == n_borrow.ability.id {
                return Some(n_borrow.ability.clone());
            } else if id < n_borrow.ability.id {
                return Self::find_node(n_borrow.left.clone(), id);
            } else {
                return Self::find_node(n_borrow.right.clone(), id);
            }
        }
        None
    }

    fn find_all_level(&self, level: u8) -> Option<Vec<Ability>> {
        let mut current_node = self.root.clone();

        while let Some(n) = current_node {
            let n_borrow = n.read().unwrap();
            if n_borrow.ability.get_level() == level {
                let mut results = Vec::new();
                Self::collect_level_abilities(self.root.clone(), level, &mut results);
                return Some(results);

            } else {
                current_node = n_borrow.right.clone();
            }
        }
        None
    }

    fn collect_level_abilities(
        node: Option<Arc<RwLock<AbilityNode>>>,
        level: u8,
        results: &mut Vec<Ability>,
    ) {
        if let Some(n) = node {

            let n_borrow = n.read().unwrap();
            results.push(n_borrow.ability.clone());

            // Explore children safely
            Self::collect_level_abilities(n_borrow.left.clone(), level, results);
            Self::collect_level_abilities(n_borrow.right.clone(), level, results);
        }
    }

    pub fn traverse_all(&self) -> Vec<Ability> {
        let mut all = Vec::new();
        Self::collect_all(self.root.clone(), &mut all);
        all
    }

    fn collect_all(node: Option<Arc<RwLock<AbilityNode>>>, all: &mut Vec<Ability>) {
        if let Some(n) = node {
            let n_borrow = n.read().unwrap();
            all.push(n_borrow.ability.clone());
            Self::collect_all(n_borrow.left.clone(), all);
            Self::collect_all(n_borrow.right.clone(), all);
        }
    }
}

pub fn handle_ability(
    caster: Entity,
    ability: &Ability,
    affected: &[Entity],
    now: u32,
    mut dq: ResMut<DamageQueue>,
    mut attack_intent_events: MessageWriter<AttackIntentEvent>,
    mut heal_events: MessageWriter<HealEvent>,
    mut buff_events: MessageWriter<ApplyBuffEvent>,
) {
    for &target in affected {
        for effect in &ability.effects {
            match effect {
                AbilityEffect::Heal { floor, ceiling, .. } => {
                    let amount = rand::rng().gen_range(*floor..*ceiling);
                    heal_events.send(HealEvent {
                        healer: caster,
                        target,
                        amount,
                    });
                }

                AbilityEffect::Damage {
                    floor,
                    ceiling,
                    damage_type,
                    scaled_with,
                    defended_with
                } => {
                    let base = rand::rng().gen_range(*floor..*ceiling) as i32;

                    dq.0.push(QueuedDamage {
                        attacker: caster,
                        target,
                        amount: base,
                        damage_type: *damage_type,

                        scaled_with: vec![(*scaled_with, 1.0)],
                        defended_with: vec![(*defended_with, 1.0)],

                        accuracy_override: None,
                        crit_chance: None,
                        tags: vec![DamageTag::FromAbility(ability.id)],
                    });

                    attack_intent_events.send(AttackIntentEvent {
                        attacker: caster,
                        target,
                    });
                }

                AbilityEffect::Buff { stat, multiplier, effects, .. } => {
                    buff_events.send(ApplyBuffEvent {
                        applier: caster,
                        target,
                        stat: *stat,
                        multiplier: *multiplier,
                        duration_in_ticks: ability.duration,
                        additional_effects: effects.clone(),
                        applied_at: now,
                    });
                }
            }
        }
    }

    // TODO: FIND A WAY TO TARGET IN THE NEXT ABILITY, I THINK THAT THE BEST OPTION IS TO CALL THIS MULTIPLE TIMES INSTEAD OF CALLING THIS FUNCTION RECURSIVELY

    // if ability.next_id.is_some() {
    //     let next_id = ability.next_id.clone().unwrap();
    //     let next_ability = ability_tree.find(next_id).unwrap();
    //     let next_affected_characters = get_affected_characters(&next_ability, affected_characters, ); 
    //     handle_ability(&next_ability, &next_affected_characters)
    // }
}

pub fn get_affected_characters(
    ability: &Ability,
    player_entity: Entity,
    cursor_position: (f32, f32),
    query: &Query<(Entity, &Position)>,
    player_position_query: &Query<&Position>,
) -> Vec<Entity> {
    let mut affected = Vec::new();

    let player_pos = player_position_query.get(player_entity).unwrap();
    let player_position = (player_pos.x, player_pos.y);

    for (entity, pos) in query.iter() {
        let target_position = (pos.x, pos.y);

        let is_affected = match &ability.shape {
            AbilityShape::Radius(radius) => {
                is_in_radius(*radius, player_position, target_position)
            }

            AbilityShape::Line { length, thickness } => {
                is_in_line(*length, *thickness, player_position, cursor_position, target_position)
            }

            AbilityShape::Cone { angle, radius } => {
                is_in_cone(*angle, *radius, player_position, cursor_position, target_position)
            }

            AbilityShape::Select => {
                distance(target_position, cursor_position) < 0.5
            }
        };

        if is_affected {
            affected.push(entity);
        }
    }

    affected
}


//
// === Geometry Helpers ===
//

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Check if position is inside a circle (radius AoE)
fn is_in_radius(radius: f32, origin: (f32, f32), target: (f32, f32)) -> bool {
    distance(origin, target) <= radius
}

/// Check if position is inside a rectangular line AoE
fn is_in_line(length: f32, thickness: f32, origin: (f32, f32), cursor: (f32, f32), target: (f32, f32)) -> bool {
    // Direction vector (normalized)
    let dir = normalize((cursor.0 - origin.0, cursor.1 - origin.1));
    let to_target = (target.0 - origin.0, target.1 - origin.1);

    // Projection length along the line
    let proj = dot(to_target, dir);

    if proj < 0.0 || proj > length {
        return false;
    }

    // Perpendicular distance to line
    let closest = (origin.0 + dir.0 * proj, origin.1 + dir.1 * proj);
    let dist = distance(closest, target);
    dist <= thickness / 2.0
}

/// Check if position is inside a cone (angle, radius)
fn is_in_cone(angle_deg: f32, radius: f32, origin: (f32, f32), cursor: (f32, f32), target: (f32, f32)) -> bool {
    let dir = normalize((cursor.0 - origin.0, cursor.1 - origin.1));
    let to_target = (target.0 - origin.0, target.1 - origin.1);
    let dist = length(to_target);

    if dist > radius {
        return false;
    }

    let norm_target = normalize(to_target);
    let dot_val = dot(dir, norm_target).clamp(-1.0, 1.0);
    let angle_to_target = dot_val.acos() * (180.0 / PI); // convert to degrees

    angle_to_target <= angle_deg / 2.0
}

//
// === Vector Math ===
//

fn length(v: (f32, f32)) -> f32 {
    (v.0 * v.0 + v.1 * v.1).sqrt()
}

fn normalize(v: (f32, f32)) -> (f32, f32) {
    let len = length(v);
    if len == 0.0 {
        (0.0, 0.0)
    } else {
        (v.0 / len, v.1 / len)
    }
}

fn dot(a: (f32, f32), b: (f32, f32)) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

/// -----------------------------
/// Startup spawn examples (with XP, Level, AccumulatedAgility)
/// -----------------------------
fn spawn_examples(mut commands: Commands, mut tm: ResMut<TurnManager>) {
    // spawn sword
    let sword = commands
        .spawn((
            Equipment {
                id: 5001,
                name: "Silversteel Blade".to_string(),
                lethality: 10,
                hit: 5,
                armor: 0,
                agility: 2,
                mind: 0,
                morale: 0,
            },
            EquipmentHooks(vec![EquipHook::BeforeAttackMultiplier {
                stat: Stat::Lethality,
                multiplier: 1.15,
                duration_turns: 1,
            }]),
        ))
        .id();

    // --------------------------------------
    // Petrus – Paladin
    // --------------------------------------
    let petrus = commands
        .spawn((
            Name("Petrus".to_string()),
            CharacterId(1),
            Class("Paladin".to_string()),
            Health {
                current: 180,
                max: 180,
                regen: 2,
            },
            Magic {
                current: 60,
                max: 60,
                regen: 1,
            },
            Stamina {
                current: 100,
                max: 100,
                regen: 3,
            },
            CombatStats {
                base_lethality: 18,
                base_hit: 80,
                base_armor: 20,
                base_agility: 7,
                base_mind: 10,
                base_morale: 95,
                movement: 5,
            },
            GrowthAttributes {
                vitality: 20,
                endurance: 14,
                spirit: 10,
                power: 12,
                control: 10,
                agility: 8,
                insight: 8,
                resolve: 18,
            },
            GrowthCurve::paladin_curve(),
            EquipmentSlots {
                weapon: Some(sword),
                ..Default::default()
            },
            Abilities(vec![]),
            Experience(0),
            Level(1),
            AccumulatedAgility(0),
            PaladinBehavior,
            StatModifiers(Vec::new()),
        ))
        .id();

    // --------------------------------------
    // Rina – Rogue
    // --------------------------------------
    let rina = commands
        .spawn((
            Name("Rina".to_string()),
            CharacterId(2),
            Class("Rogue".to_string()),
            Health {
                current: 90,
                max: 90,
                regen: 1,
            },
            Magic {
                current: 40,
                max: 40,
                regen: 1,
            },
            Stamina {
                current: 80,
                max: 80,
                regen: 2,
            },
            CombatStats {
                base_lethality: 14,
                base_hit: 90,
                base_armor: 10,
                base_agility: 14,
                base_mind: 9,
                base_morale: 85,
                movement: 7,
            },
            GrowthAttributes {
                vitality: 10,
                endurance: 11,
                spirit: 8,
                power: 12,
                control: 20,
                agility: 22, // main stat
                insight: 12,
                resolve: 11,
            },
            GrowthCurve::rogue_curve(),
            EquipmentSlots::default(),
            Abilities(vec![]),
            Experience(0),
            Level(1),
            AccumulatedAgility(0),
            RogueBehavior,
            StatModifiers(Vec::new()),
        ))
        .id();

    // --------------------------------------
    // Toshiko – Spirit Medium (SPECIAL EXTRA HP MECHANIC)
    // --------------------------------------
    let toshiko = commands
        .spawn((
            Name("Toshiko".to_string()),
            CharacterId(3),
            Class("Spirit Medium".to_string()),
            Health {
                current: 70,
                max: 70,
                regen: 1,
            },
            Magic {
                current: 120,
                max: 120,
                regen: 4,
            },
            Stamina {
                current: 60,
                max: 60,
                regen: 1,
            },
            CombatStats {
                base_lethality: 8,
                base_hit: 75,
                base_armor: 6,
                base_agility: 10,
                base_mind: 20,
                base_morale: 90,
                movement: 5,
            },
            GrowthAttributes {
                vitality: 12,
                endurance: 10,
                spirit: 25, // core stat
                power: 6,
                control: 9,
                agility: 10,
                insight: 20,
                resolve: 16,
            },
            GrowthCurve::spirit_mage_curve(),
            ExtraHp {
                current: 40,
                max: 40,
            },
            EquipmentSlots::default(),
            Abilities(vec![]),
            Experience(0),
            Level(1),
            AccumulatedAgility(0),
            SpiritMediumBehavior,
            StatModifiers(Vec::new()),
        ))
        .id();
    
    // register participants in turn manager
    tm.participants.push(petrus);
    tm.participants.push(rina);

    // Optional: spawn a buff entity (e.g., Blessing of Courage) applied to Petrus
    let blessing = commands
        .spawn((
            Buff {
                stat: Stat::Hit,
                multiplier: 1.10, // +10% hit
                remaining_turns: 3,
                source: None,
            },
            // link it to Petrus by adding a marker component or by storing ApplyTo resource. Simpler approach:
        ))
        .id();

    // For demonstration: attach the buff to petrus by inserting a StatModifier directly
    commands.entity(petrus).insert(StatModifiers(vec![StatModifier {
        stat: Stat::Hit,
        multiplier: 1.10,
        expires_in_turns: Some(3),
        source: Some(blessing),
    }]));

    info!("Spawned Petrus {:?} and Rina {:?} (sword {:?})", petrus, rina, sword);
}

/// -----------------------------
/// App Setup
/// -----------------------------
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        // TO DO: insert all systems correctly
        app.insert_resource(TurnOrder::default())
            .insert_resource(TurnManager::default())
            // events
            .add_event::<AttackIntentEvent>()
            .add_event::<BeforeAttackEvent>()
            .add_event::<AttackExecuteEvent>()
            .add_event::<BeforeHitEvent>()
            .add_event::<DamageEvent>()
            .add_event::<AfterHitEvent>()
            .add_event::<AfterAttackEvent>()
            .add_event::<LevelUpEvent>()
            .add_event::<TurnOrderCalculatedEvent>()
            .add_event::<TurnStartEvent>()
            .add_event::<TurnEndEvent>()
            // startup
            .add_startup_system(spawn_examples)
            // xp / leveling systems
            .add_system(award_xp_system)
            .add_system(level_up_system.after(award_xp_system))
            // turn systems
            .add_system(register_participants_system)
            .add_system(compute_turn_order_system.after(register_participants_system))
            .add_system(auto_advance_after_order.after(compute_turn_order_system))
            .add_system(on_turn_start_system.after(auto_advance_after_order))
            .add_system(buff_tick_on_turn_start_system.after(on_turn_start_system))
            .add_system(advance_turn_system.after(compute_turn_order_system))
            // combat pipeline (core)
            .add_system(process_attack_intent)
            .add_system(before_to_execute.after(process_attack_intent))
            .add_system(before_hit_listeners.after(before_to_execute))
            .add_system(process_attack_intent.after(before_hit_listeners))
            .add_system(apply_damage_system.after(process_attack_intent))
            .add_system(after_hit_listeners.after(apply_damage_system))
            .add_system(after_attack_finalizers.after(after_hit_listeners))
            // supporting
            .add_system(regen_system)
            .add_system(debug_print_system);
    }
}

// fn main() {
//     App::new()
//         .add_plugins(DefaultPlugins)
//         .add_plugin(CombatPlugin)
//         .run();
// }

