use bevy::prelude::*;
use bevy::ecs::message::{MessageIterator, MessageMutIterator};
use rand::Rng;
use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::f32::consts::PI;
use serde::{Deserialize, Serialize};

use crate::combat_ability::*;
pub use crate::combat_ability::MagicSchool;
use crate::constants::{
    BASIC_ATTACK_ACTION_POINT_COST, DEFAULT_ACTION_POINTS, DEFAULT_MAGIC_REGEN_PER_TICK,
    ITEM_ACTION_POINT_COST,
};
use crate::core::Timestamp;
use crate::skill_tree::{LearnedSkills, MagicCostMultipliers, SkillPoints, SkillTreeAccess, SkillTreeKind};

const HIT_CHANCE_LOGISTIC_K: f32 = 0.03;

/// A successful hit roll lands in the critical window when the random roll is
/// within this fraction of the upper end of the hit chance — i.e. a "barely
/// landed" hit. With the default 0.10, the top 10% of the rolls that still hit
/// become critical hits.
const CRITICAL_HIT_FRACTION: f32 = 0.10;

/// Multiplicative damage bonus applied when a hit rolls into the critical
/// window. Stacks multiplicatively with weakness multipliers.
const CRITICAL_HIT_DAMAGE_MULTIPLIER: f32 = 1.5;

/// TO DO: Implement what the AI pointed out bellow
/// One important note: the current turn flow still allows one committed action per turn. So AP now exists, is configurable per character, and is refilled correctly, but spending multiple actions inside a single turn is not implemented yet. If you want, I can do that next.
/// One caveat: the combat runtime still does not spend ability magic costs at cast time, because that path was already not implemented before this change. The data model is ready for school-specific costs now, but the actual resource deduction logic is still the next step.

// Compatibility helpers for Bevy Messages (0.17) to keep older `send/iter` style calls compiling.
trait MessageWriterSendExt<E: Message> {
    fn send(&mut self, event: E);
}
impl<'w, E: Message> MessageWriterSendExt<E> for MessageWriter<'w, E> {
    fn send(&mut self, event: E) {
        self.write(event);
    }
}

trait MessageReaderIterExt<E: Message> {
    fn iter(&mut self) -> MessageIterator<'_, E>;
}
impl<'w, 's, E: Message> MessageReaderIterExt<E> for MessageReader<'w, 's, E> {
    fn iter(&mut self) -> MessageIterator<'_, E> {
        self.read()
    }
}

trait MessageMutatorIterExt<E: Message> {
    fn iter_mut(&mut self) -> MessageMutIterator<'_, E>;
}
impl<'w, 's, E: Message> MessageMutatorIterExt<E> for MessageMutator<'w, 's, E> {
    fn iter_mut(&mut self) -> MessageMutIterator<'_, E> {
        self.read()
    }
}

/// -----------------------------
/// Components & Types
/// -----------------------------

#[derive(Component, Debug)]
pub struct CharacterId(pub u32);

#[derive(Component, Debug)]
pub struct Name(pub String);

#[derive(Component, Debug)]
pub struct Class(pub String);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DamageType {
    Physical,
    Fire,
    Ice,
    True,
}

/// Generic current/base pair for stats. `base` is the natural ceiling that
/// permanent changes (level-up, equipment) modify. `current` is the live
/// gameplay value, which can drop below `base` (depletion, debuffs) or, with
/// future buffs, exceed it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StatPool<T: Copy> {
    pub current: T,
    pub base: T,
}

impl<T: Copy + Default> Default for StatPool<T> {
    fn default() -> Self {
        Self { current: T::default(), base: T::default() }
    }
}

impl StatPool<i32> {
    pub fn new(base: i32) -> Self {
        let base = base.max(0);
        Self { current: base, base }
    }

    /// Permanent change to the natural ceiling. Keeps `current` proportional —
    /// when raising `base`, also raise `current` by the same amount so a
    /// freshly-leveled character doesn't immediately feel partially depleted.
    pub fn add_to_base(&mut self, amount: i32) {
        self.base = (self.base + amount).max(0);
        self.current = (self.current + amount).max(0);
    }

    /// True if `cost` can be paid from `current`.
    pub fn can_spend(&self, cost: i32) -> bool {
        cost <= 0 || self.current >= cost
    }

    pub fn spend(&mut self, cost: i32) -> bool {
        if !self.can_spend(cost) {
            return false;
        }
        self.current -= cost.max(0);
        true
    }

    /// Restore `amount` toward `base`. Buffs can let `current` exceed `base`,
    /// but plain restoration clamps at `base`.
    pub fn restore_to_base(&mut self, amount: i32) {
        self.current = (self.current + amount).min(self.base);
    }
}

impl StatPool<f32> {
    pub fn new(base: f32) -> Self {
        let base = base.max(0.0);
        Self { current: base, base }
    }

    pub fn add_to_base(&mut self, amount: f32) {
        self.base = (self.base + amount).max(0.0);
        self.current = (self.current + amount).max(0.0);
    }

    pub fn can_spend(&self, cost: f32) -> bool {
        cost <= 0.0 || self.current + f32::EPSILON >= cost
    }

    pub fn spend(&mut self, cost: f32) -> bool {
        if !self.can_spend(cost) {
            return false;
        }
        self.current = (self.current - cost).max(0.0);
        true
    }

    pub fn restore_to_base(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.base);
    }
}

/// Single source of truth for everything used in combat.
///
/// - Resource pools (`health`, `morale`, `action_points`, `movement`, the four
///   magic schools): `current` is the live value, `base` is the natural
///   ceiling.
/// - Capability stats (`lethality`, `hit`, `armor`, `speed`, `evasion`,
///   `mind`): `current` is recomputed from `base` plus active modifiers by
///   `recompute_combat_capability_system`. Permanent changes write to `base`;
///   transient debuffs and buffs feed in via the recompute pass.
/// - Regen rates are *per hour of rest*, applied when a `RestEvent` fires.
///   Magic does not auto-regenerate during exploration; only rest restores it.
#[derive(Component, Debug, Clone)]
pub struct CombatStats {
    // --- Depleting resources ---------------------------------------------
    pub health: StatPool<i32>,
    pub morale: StatPool<i32>,
    pub action_points: StatPool<i32>,
    pub movement: StatPool<i32>,
    pub kiho: StatPool<f32>,
    pub onmyodo: StatPool<f32>,
    pub yokaijutsu: StatPool<f32>,
    pub kamishin: StatPool<f32>,

    // --- Combat capability (current = base + buffs - debuffs, recomputed) -
    pub lethality: StatPool<i32>,
    pub hit: StatPool<i32>,
    pub armor: StatPool<i32>,
    /// Drives turn-order accumulation and movement points per turn.
    pub speed: StatPool<i32>,
    /// Reduces the attacker's hit chance against this character.
    pub evasion: StatPool<i32>,
    pub mind: StatPool<i32>,

    // --- Regen rates, applied per hour of rest ---------------------------
    pub health_per_rest_hour: i32,
    pub morale_per_rest_hour: i32,
    pub kiho_per_rest_hour: f32,
    pub onmyodo_per_rest_hour: f32,
    pub yokaijutsu_per_rest_hour: f32,
    pub kamishin_per_rest_hour: f32,
}

impl Default for CombatStats {
    fn default() -> Self {
        Self {
            health: <StatPool<i32>>::new(0),
            morale: <StatPool<i32>>::new(0),
            action_points: <StatPool<i32>>::new(DEFAULT_ACTION_POINTS),
            movement: <StatPool<i32>>::new(0),
            kiho: <StatPool<f32>>::new(0.0),
            onmyodo: <StatPool<f32>>::new(0.0),
            yokaijutsu: <StatPool<f32>>::new(0.0),
            kamishin: <StatPool<f32>>::new(0.0),
            lethality: <StatPool<i32>>::new(0),
            hit: <StatPool<i32>>::new(0),
            armor: <StatPool<i32>>::new(0),
            speed: <StatPool<i32>>::new(0),
            evasion: <StatPool<i32>>::new(0),
            mind: <StatPool<i32>>::new(0),
            health_per_rest_hour: 0,
            morale_per_rest_hour: 0,
            kiho_per_rest_hour: 0.0,
            onmyodo_per_rest_hour: 0.0,
            yokaijutsu_per_rest_hour: 0.0,
            kamishin_per_rest_hour: 0.0,
        }
    }
}

impl CombatStats {
    pub fn pool(&self, school: MagicSchool) -> &StatPool<f32> {
        match school {
            MagicSchool::Kiho => &self.kiho,
            MagicSchool::Onmyodo => &self.onmyodo,
            MagicSchool::Yokaijutsu => &self.yokaijutsu,
            MagicSchool::Kamishin => &self.kamishin,
        }
    }

    pub fn pool_mut(&mut self, school: MagicSchool) -> &mut StatPool<f32> {
        match school {
            MagicSchool::Kiho => &mut self.kiho,
            MagicSchool::Onmyodo => &mut self.onmyodo,
            MagicSchool::Yokaijutsu => &mut self.yokaijutsu,
            MagicSchool::Kamishin => &mut self.kamishin,
        }
    }

    pub fn total_magic_current(&self) -> f32 {
        self.kiho.current + self.onmyodo.current + self.yokaijutsu.current + self.kamishin.current
    }

    pub fn total_magic_base(&self) -> f32 {
        self.kiho.base + self.onmyodo.base + self.yokaijutsu.base + self.kamishin.base
    }
}

/// Request to rest for `hours`. Pipeline: external code fires `RestEvent`,
/// `expand_rest_intent_system` fans it out into one `BeforeRestEvent` per
/// affected entity (mutable so listeners may modify hours), `rest_regen_system`
/// reads `BeforeRestEvent` and applies regen, then writes `AfterRestEvent` for
/// post-rest reactions (status cleanups, world-time advance, etc.).
#[derive(Debug, Clone, Message)]
pub struct RestEvent {
    /// Optional target. When `None`, applies to every entity with `CombatStats`.
    pub target: Option<Entity>,
    pub hours: u32,
    pub cause: ActionCause,
}

/// Fires once per affected entity, before regen is applied. Listeners may
/// mutate `hours` (e.g. rest interruption from `Starved` debuff, equipment
/// that grants extra rest, sleep-quality buffs).
#[derive(Debug, Clone, Message)]
pub struct BeforeRestEvent {
    pub target: Entity,
    pub hours: u32,
    pub cause: ActionCause,
}

/// Fires once per affected entity, after regen has been applied. `hours` is
/// the value that was actually used (post any `BeforeRestEvent` mutation).
#[derive(Debug, Clone, Message)]
pub struct AfterRestEvent {
    pub target: Entity,
    pub hours: u32,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Stat {
    Health,
    Magic,
    Kiho,
    Onmyodo,
    Yokaijutsu,
    Kamishin,
    ActionPoints,
    Movement,
    Lethality,
    Hit,
    Speed,
    Evasion,
    Armor,
    Mind,
    Morale,
}

fn get_stat_value(stat: Stat, combat_stats: Option<&CombatStats>) -> i32 {
    let Some(c) = combat_stats else { return 0 };
    match stat {
        Stat::Lethality => c.lethality.current,
        Stat::Hit => c.hit.current,
        Stat::Speed => c.speed.current,
        Stat::Evasion => c.evasion.current,
        Stat::Armor => c.armor.current,
        Stat::Mind => c.mind.current,
        Stat::Morale => c.morale.current,
        Stat::Health => c.health.current,
        Stat::ActionPoints => c.action_points.current,
        Stat::Movement => c.movement.current,
        Stat::Magic => c.total_magic_current().round() as i32,
        Stat::Kiho => c.kiho.current.round() as i32,
        Stat::Onmyodo => c.onmyodo.current.round() as i32,
        Stat::Yokaijutsu => c.yokaijutsu.current.round() as i32,
        Stat::Kamishin => c.kamishin.current.round() as i32,
    }
}


// The attributes the player distributes (the GDD's "fake attributes").
// Magic schools are no longer here — the four schools are runtime pools on
// `CombatStats`. Spirit-derived growth distributes evenly across them for now;
// the GDD's player-driven distribution is a TODO once a UI exists.
//
// Agility is split into Speed and Evasion: Speed drives turn order and
// movement, Evasion drives hit avoidance.
/// The "fake attributes" the player allocates points into at level-up. None of
/// these are read by combat directly — they only feed the growth contribution
/// table and shape how a character's `CombatStats` grow over time.
///
/// Names are intentionally abstract (qualities, not stats) so that no growth
/// attribute shares a name with the combat stat it grows. For example,
/// `celerity` grows the combat `speed` and `movement` stats; `reflex` grows
/// the combat `evasion` stat.
#[derive(Component, Debug, Default)]
pub struct GrowthAttributes {
    pub vitality: u8,   // grows Health (max + per-rest-hour regen)
    pub endurance: u8,  // grows Onmyodo (place-bound earth practice)
    pub spirit: u8,     // produces 3 distribution points per spirit; small magic baseline
    pub power: u8,      // grows Lethality + small Yokaijutsu (forbidden strength)
    pub control: u8,    // grows Hit
    pub celerity: u8,   // grows Speed (turn-order accumulation) and Movement
    pub reflex: u8,     // grows Evasion (hit avoidance)
    pub insight: u8,    // grows Mind + small Kamishin (theological understanding)
    pub resolve: u8,    // grows Morale + small Kiho (disciplined inner force)
    /// Per-magic-school distribution of the points produced by `spirit`.
    /// Per the GDD: each spirit point yields 3 distribution points the player
    /// allocates among the four schools. The level-up system reads the four
    /// counts here as four independent growth sources, so a character with
    /// `kiho: 30` will gain a lot of Kiho per level even if their `spirit` is
    /// modest. Soft constraint: sum ≤ 3 * spirit (validated at allocation
    /// time once a UI exists; not enforced here).
    pub magic_distribution: MagicDistribution,
}

/// Sub-allocation of spirit's derived distribution points, one count per
/// magic school.
#[derive(Debug, Default, Clone, Copy)]
pub struct MagicDistribution {
    pub kiho: u8,
    pub onmyodo: u8,
    pub yokaijutsu: u8,
    pub kamishin: u8,
}

/// A specific combat-stat field that growth points can grow.
///
/// Distinct from [`Stat`] because growth writes to `base` (a permanent change)
/// for capacity stats and to the `*_per_rest_hour` rates for regens, while
/// `Stat` is for combat-time reads of `current`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GrowthTarget {
    Health,
    HealthRegen,
    Morale,
    MoraleRegen,
    Lethality,
    Hit,
    Armor,
    Speed,
    Evasion,
    Mind,
    Movement,
    Kiho,
    Onmyodo,
    Yokaijutsu,
    Kamishin,
}

/// A single (target, base, exponent) triple. Per the GDD's level-up curve:
/// `gain = base / 8 - (2 * points)^exponent / 524288`
/// (computed by `curve_growth_tactical`). One growth attribute can declare
/// multiple contributions — e.g. `vitality` grows both health max and
/// per-rest-hour health regen.
#[derive(Debug, Clone, Copy)]
pub struct GrowthContribution {
    pub target: GrowthTarget,
    pub base: f32,
    pub exponent: f32,
}

impl GrowthAttributes {
    /// Returns one `(points_in_attribute, &[contributions])` pair per growth
    /// attribute. The level-up system iterates this and applies each
    /// contribution using `curve_growth_tactical(points, base, exponent)`.
    pub fn iter_contributions(&self) -> [(u8, &'static [GrowthContribution]); 13] {
        [
            (self.vitality, &VITALITY_CONTRIBUTIONS[..]),
            (self.endurance, &ENDURANCE_CONTRIBUTIONS[..]),
            (self.spirit, &SPIRIT_CONTRIBUTIONS[..]),
            (self.power, &POWER_CONTRIBUTIONS[..]),
            (self.control, &CONTROL_CONTRIBUTIONS[..]),
            (self.celerity, &CELERITY_CONTRIBUTIONS[..]),
            (self.reflex, &REFLEX_CONTRIBUTIONS[..]),
            (self.insight, &INSIGHT_CONTRIBUTIONS[..]),
            (self.resolve, &RESOLVE_CONTRIBUTIONS[..]),
            // Spirit's distribution: each per-school count is its own growth
            // source, feeding the corresponding magic pool's curve.
            (self.magic_distribution.kiho, &KIHO_DIST_CONTRIBUTIONS[..]),
            (self.magic_distribution.onmyodo, &ONMYODO_DIST_CONTRIBUTIONS[..]),
            (self.magic_distribution.yokaijutsu, &YOKAIJUTSU_DIST_CONTRIBUTIONS[..]),
            (self.magic_distribution.kamishin, &KAMISHIN_DIST_CONTRIBUTIONS[..]),
        ]
    }
}

const VITALITY_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Health, base: 100.0, exponent: 3.007632509 },
    GrowthContribution { target: GrowthTarget::HealthRegen, base: 10.0, exponent: 2.691262945 },
];

// Endurance — earth-bound, place-rooted. Themed for onmyodo (nature
// magic, "place-bound practice" per the GDD's school description).
const ENDURANCE_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Onmyodo, base: 8.0, exponent: 3.0 },
];

// Spirit is primarily a *budget* — each point produces 3 distribution points
// the player allocates among the four schools (see `MagicDistribution`).
// Spirit also provides a small across-the-board magical baseline so a
// character with spirit but no manual distribution still grows magic.
const SPIRIT_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Kiho, base: 6.0, exponent: 3.0 },
    GrowthContribution { target: GrowthTarget::Onmyodo, base: 6.0, exponent: 3.0 },
    GrowthContribution { target: GrowthTarget::Yokaijutsu, base: 6.0, exponent: 3.0 },
    GrowthContribution { target: GrowthTarget::Kamishin, base: 6.0, exponent: 3.0 },
];

// Power — themed for yokaijutsu (occult / forbidden strength).
const POWER_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Lethality, base: 250.0, exponent: 3.0 },
    GrowthContribution { target: GrowthTarget::Yokaijutsu, base: 8.0, exponent: 3.0 },
];

const CONTROL_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Hit, base: 250.0, exponent: 3.0 },
];

const CELERITY_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Speed, base: 250.0, exponent: 3.0 },
    GrowthContribution { target: GrowthTarget::Movement, base: 50.0, exponent: 3.5 },
];

const REFLEX_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Evasion, base: 250.0, exponent: 3.0 },
];

// Insight — themed for kamishin (divine knowledge, theological understanding).
const INSIGHT_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Mind, base: 250.0, exponent: 3.0 },
    GrowthContribution { target: GrowthTarget::Kamishin, base: 8.0, exponent: 3.0 },
];

// Resolve — themed for kiho (disciplined inner force, mental focus).
const RESOLVE_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Morale, base: 250.0, exponent: 3.0 },
    GrowthContribution { target: GrowthTarget::MoraleRegen, base: 8.0, exponent: 2.7 },
    GrowthContribution { target: GrowthTarget::Kiho, base: 8.0, exponent: 3.0 },
];

// Per-school distribution contributions. Each is the *primary* growth path
// for its school — the player decides how to split spirit's derived points
// across these four sub-allocations.
const KIHO_DIST_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Kiho, base: 25.0, exponent: 3.0 },
];

const ONMYODO_DIST_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Onmyodo, base: 25.0, exponent: 3.0 },
];

const YOKAIJUTSU_DIST_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Yokaijutsu, base: 25.0, exponent: 3.0 },
];

const KAMISHIN_DIST_CONTRIBUTIONS: &[GrowthContribution] = &[
    GrowthContribution { target: GrowthTarget::Kamishin, base: 25.0, exponent: 3.0 },
];

// A character-specific growth curve.
// These are multipliers (or offsets) applied on top of the level up formulas.
#[derive(Component, Debug, Clone)]
pub struct GrowthCurve {
    pub hp_curve: f32,
    pub magic_curve: f32,
    pub lethality_curve: f32,
    pub hit_curve: f32,
    pub speed_curve: f32,
    pub evasion_curve: f32,
    pub mind_curve: f32,
    pub morale_curve: f32,
}

// Example: default balanced curve
impl Default for GrowthCurve {
    fn default() -> Self {
        Self {
            hp_curve: 1.0,
            magic_curve: 1.0,
            lethality_curve: 1.0,
            hit_curve: 1.0,
            speed_curve: 1.0,
            evasion_curve: 1.0,
            mind_curve: 1.0,
            morale_curve: 1.0,
        }
    }
}

impl GrowthCurve {
    pub fn paladin_curve() -> Self {
        Self {
            hp_curve: 1.2,
            magic_curve: 0.9,
            lethality_curve: 1.0,
            hit_curve: 1.0,
            speed_curve: 0.9,
            evasion_curve: 0.9,
            mind_curve: 1.0,
            morale_curve: 1.2,
        }
    }

    pub fn rogue_curve() -> Self {
        Self {
            hp_curve: 0.9,
            magic_curve: 0.9,
            lethality_curve: 1.1,
            hit_curve: 1.1,
            speed_curve: 1.2,
            evasion_curve: 1.2,
            mind_curve: 1.0,
            morale_curve: 1.0,
        }
    }

    pub fn spirit_mage_curve() -> Self {
        Self {
            hp_curve: 0.9,
            magic_curve: 1.3,
            lethality_curve: 0.9,
            hit_curve: 1.0,
            speed_curve: 1.0,
            evasion_curve: 1.0,
            mind_curve: 1.2,
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
pub enum DamageTag {
    FromAbility(u16),
    Critical,
}

/// Per-target multipliers for incoming damage by type. `1.0` is neutral,
/// `< 1.0` is resistant, `> 1.0` is weak. Applied in
/// `process_damage_queue_system` after armor/incoming-mods, multiplicatively
/// alongside any crit multiplier so the two stack.
#[derive(Component, Debug, Clone, Copy)]
pub struct DamageWeaknesses {
    pub physical: f32,
    pub fire: f32,
    pub ice: f32,
    pub true_dmg: f32,
}

impl Default for DamageWeaknesses {
    fn default() -> Self {
        Self { physical: 1.0, fire: 1.0, ice: 1.0, true_dmg: 1.0 }
    }
}

impl DamageWeaknesses {
    pub fn multiplier_for(&self, dt: DamageType) -> f32 {
        match dt {
            DamageType::Physical => self.physical,
            DamageType::Fire => self.fire,
            DamageType::Ice => self.ice,
            DamageType::True => self.true_dmg,
        }
    }
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

    /// Multiplicative crit bonus already applied in
    /// `process_damage_queue_system`. `1.0` = normal hit, `> 1.0` = critical.
    /// Decided at hit-roll time in `queue_damage_from_before_attack`.
    pub crit_multiplier: f32,

    /// Optional tags for special behavior (from ability id, critical, reflect etc.)
    pub tags: Vec<DamageTag>,

    /// What activated this damage. Threaded into the resulting `DamageEvent` /
    /// `AfterHitEvent` so listeners (status reactors, equipment procs) can react
    /// based on origin and skip self-feedback.
    pub cause: ActionCause,
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

#[derive(Component, Debug, Default, Clone)]
pub struct Inventory {
    pub item_ids: Vec<u16>,
}

impl Inventory {
    pub fn has_item(&self, item_id: u16) -> bool {
        self.item_ids.contains(&item_id)
    }

    pub fn add_item(&mut self, item_id: u16) {
        self.item_ids.push(item_id);
    }

    pub fn remove_item(&mut self, item_id: u16) -> bool {
        if let Some(index) = self.item_ids.iter().position(|id| *id == item_id) {
            self.item_ids.remove(index);
            return true;
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EquipmentSlotType {
    Weapon,
    Armor,
    Accessory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WeaponType {
    Sword,
    Dagger,
    Staff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArmorType {
    HeavyArmor,
    LightArmor,
    Robe,
    Shield,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccessoryType {
    Charm,
    Ring,
    Relic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EquipmentType {
    Weapon(WeaponType),
    Armor(ArmorType),
    Accessory(AccessoryType),
}

impl EquipmentType {
    pub fn slot_type(self) -> EquipmentSlotType {
        match self {
            EquipmentType::Weapon(_) => EquipmentSlotType::Weapon,
            EquipmentType::Armor(_) => EquipmentSlotType::Armor,
            EquipmentType::Accessory(_) => EquipmentSlotType::Accessory,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentSlot {
    pub slot_type: EquipmentSlotType,
    pub allowed_types: Vec<EquipmentType>,
    pub equipped: Option<Entity>,
}

/// Per-character equipment rules and current state.
/// The slots present in `slots` define what this character can equip.
#[derive(Component, Debug, Default, Clone)]
pub struct EquipmentLoadout {
    pub slots: Vec<EquipmentSlot>,
}

impl EquipmentLoadout {
    pub fn with_slots(slot_types: impl IntoIterator<Item = EquipmentSlotType>) -> Self {
        Self {
            slots: slot_types
                .into_iter()
                .map(|slot_type| EquipmentSlot {
                    slot_type,
                    allowed_types: default_allowed_types_for_slot(slot_type),
                    equipped: None,
                })
                .collect(),
        }
    }

    pub fn with_allowed_types(
        slots: impl IntoIterator<Item = (EquipmentSlotType, Vec<EquipmentType>)>,
    ) -> Self {
        Self {
            slots: slots
                .into_iter()
                .map(|(slot_type, allowed_types)| EquipmentSlot {
                    slot_type,
                    allowed_types,
                    equipped: None,
                })
                .collect(),
        }
    }

    pub fn equip_in_first_matching_slot(
        &mut self,
        equipment_type: EquipmentType,
        item: Entity,
    ) -> bool {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| {
                slot.slot_type == equipment_type.slot_type()
                    && slot.allowed_types.contains(&equipment_type)
                    && slot.equipped.is_none()
            })
        {
            slot.equipped = Some(item);
            return true;
        }

        if let Some(slot) = self.slots.iter_mut().find(|slot| {
            slot.slot_type == equipment_type.slot_type()
                && slot.allowed_types.contains(&equipment_type)
        }) {
            slot.equipped = Some(item);
            return true;
        }

        false
    }

    pub fn equipped_in_slot(&self, slot_type: EquipmentSlotType) -> Option<Entity> {
        self.slots
            .iter()
            .find(|slot| slot.slot_type == slot_type)
            .and_then(|slot| slot.equipped)
    }

    pub fn equipped_items(&self) -> impl Iterator<Item = Entity> + '_ {
        self.slots.iter().filter_map(|slot| slot.equipped)
    }

    pub fn has_equipped_item_id(
        &self,
        item_id: u16,
        equipment_q: &Query<&Equipment>,
    ) -> bool {
        self.equipped_items().any(|entity| {
            equipment_q
                .get(entity)
                .map(|equipment| equipment.id == item_id)
                .unwrap_or(false)
        })
    }
}

#[derive(Component, Debug, Clone)]
pub enum PlayerAction {
    Attack(Entity),                // choose target
    UseAbility(u32, Entity),       // ability_id + target
    UseItem(u16, Option<Entity>),  // item_id
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

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct Equipment {
    pub id: u16,
    pub name: String,
    pub equipment_type: EquipmentType,
    pub base_price: u32,
    pub materials: Vec<ItemMaterialCost>,
    pub lethality: i32,
    pub hit: i32,
    pub armor: i32,
    pub agility: i32,
    pub mind: i32,
    pub morale: i32,
}

#[derive(Component, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WeaponSharpness {
    pub current: u8,
    pub loss_per_attack: u8,
}

impl WeaponSharpness {
    pub fn new(current: u8, loss_per_attack: u8) -> Self {
        Self {
            current: current.min(100),
            loss_per_attack,
        }
    }

    pub fn damage_multiplier(&self) -> f32 {
        0.6 + (self.current.min(100) as f32 / 100.0) * 0.4
    }

    pub fn dull_on_attack(&mut self) {
        self.current = self.current.saturating_sub(self.loss_per_attack);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WeaponBeforeAttackEffect {
    AddFlatDamage { flat: i32 },
    MultiplyBaseDamage { multiplier: f32 },
    AddHit { amount: i32 },
    OverrideDamageType { damage_type: DamageType },
    ApplyTemporaryStatModifier {
        stat: Stat,
        multiplier: f32,
        duration_turns: u32,
    },
    BonusWhenSharp {
        minimum_sharpness: u8,
        flat_damage: i32,
    },
}

#[derive(Component, Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeaponBeforeAttackEffects(pub Vec<WeaponBeforeAttackEffect>);

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemMaterial {
    IronIngot = 600,
    SilverSteelIngot = 1000,
    OakWood = 400,
    Leather = 500,
    Cloth = 300,
    CrystalDust = 800,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemMaterialCost {
    pub material: ItemMaterial,
    pub quantity: u32,
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
    pub ends_at_timestamp: u32,
    pub source: Option<Entity>, // which equipment/ability created it (optional)
}

/// Temporary stat modifiers applied to a character for a limited duration (e.g., one attack)
#[derive(Component, Debug)]
pub struct StatModifiers(pub Vec<StatModifier>);

#[derive(Clone, Debug)]
pub struct StatModifier {
    pub stat: Stat,
    pub multiplier: f32, // multiplicative (e.g., 1.2 => +20%)
    pub expires_at_timestamp: Option<u32>, // None => permanent until explicitly removed
    pub source: Option<Entity>,
}

/// Simple experience / level component (placeholder)
#[derive(Component, Debug)]
pub struct Experience(pub u32);

#[derive(Component, Debug)]
pub struct Level(pub u32);

#[derive(Component, Debug)]
pub struct AccumulatedSpeed(pub u32);

impl Default for AccumulatedSpeed {
    fn default() -> Self {
        Self(0)
    }
}

/// AI parameters (kept as component). Cheap to copy so the BT evaluator
/// can take a snapshot.
#[derive(Component, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AIParameters {
    /// 0..=10, how eagerly the AI commits to offense.
    pub aggressiveness: u8,
    /// 0..=10, how strongly the AI prefers safe plays.
    pub caution: u8,
    /// 0..=10, willingness to investigate / probe (currently unused by BT).
    pub curiosity: u8,
    /// 0..=10, how reliably the AI can detect and target threats.
    pub perception: u8,
    /// 0..=10, willingness to stay engaged when wounded.
    pub bravery: u8,
    /// 0..=10, how long the AI will hold position before pressing attacks.
    pub patience: u8,
    /// HP percentage at which the AI flips into panic behaviour
    /// (heal/flee/defend rather than attack). Default 25.
    pub panic_threshold: u8,
    /// 0..=10, higher values cause the AI to spend magic more freely.
    /// Lower values save mana for emergencies.
    pub magic_thrift: u8,
    /// 0..=10, weight given to ally protection over self-preservation.
    pub group_loyalty: u8,
    /// Which target the AI prefers when picking among living enemies.
    pub focus_preference: TargetFocus,
    /// Whether the AI prefers melee, ranged, or any range when picking
    /// abilities (used by BT conditions; unused for basic attacks).
    pub preferred_range: PreferredRange,
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
            panic_threshold: 25,
            magic_thrift: 5,
            group_loyalty: 5,
            focus_preference: TargetFocus::default(),
            preferred_range: PreferredRange::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TargetFocus {
    #[default]
    LowestHp,
    HighestHp,
    Closest,
    Furthest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PreferredRange {
    #[default]
    Any,
    Melee,
    Ranged,
}

/// -----------------------------
/// Events (FULL EVENTS model)
/// -----------------------------

/// Identifies what activated a given action / event. Threaded through Before/After
/// event pairs so listeners can react conditionally on the origin (equipment proc,
/// status reaction, ability cast, world event, etc.) and so reactive damage from
/// status effects can be filtered out to prevent feedback loops.
#[derive(Debug, Clone)]
pub enum ActionCause {
    /// Triggered explicitly by the player.
    Player,
    /// Decided by an AI controller.
    Ai,
    /// Resolved as part of an ability (id is the ability id).
    Ability { id: u16 },
    /// A piece of equipment fired this (weapon proc, armor reaction, ...).
    Equipment { item: Entity },
    /// A status effect on `source` caused this (used by reactive status systems).
    StatusEffect { source: Entity },
    /// A passive ability or trait on `source` caused this.
    Passive { source: Entity },
    /// A reaction / interrupt by `reactor` running ability `ability_id`.
    Reaction { reactor: Entity, ability_id: u16 },
    /// World-driven cause (tile event, dialog, scripted encounter).
    World,
    /// Cause unknown or not yet attributed.
    Other,
}

impl Default for ActionCause {
    fn default() -> Self { Self::Other }
}

#[derive(Debug, Clone, Message)]
pub struct AttackIntentEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub ability: Option<Ability>,
    pub context: AttackContext,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Message)]
pub struct AbilityIntentEvent {
    pub user: Entity,
    pub ability_id: u16,
}

#[derive(Debug, Clone, Message)]
pub struct DefendIntentEvent {
    pub defender: Entity,
}

#[derive(Debug, Clone, Message)]
pub struct WaitIntentEvent {
    pub waiter: Entity,
}

#[derive(Debug, Clone, Message)]
pub struct BeforeAttackEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub ability: Option<Ability>,
    pub context: AttackContext,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Message)]
pub struct AttackExecuteEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub ability: Option<Ability>,
    pub context: AttackContext,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Message)]
pub struct BeforeHitEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub ability: Option<Ability>,
    pub context: AttackContext,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Message)]
pub struct DamageEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub amount: i32,
    pub damage_type: DamageType,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Message)]
pub struct HealEvent {
    pub healer: Entity,
    pub target: Entity,
    pub amount: u32,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Message)]
pub struct ApplyBuffEvent {
    pub applier: Entity,
    pub target: Entity,
    pub stat: Stat,
    pub multiplier: f32,
    pub duration_in_ticks: u32,
    pub additional_effects: Option<Vec<u16>>,
    pub applied_at: u32,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Message)]
pub struct AfterHitEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub amount: i32,
    pub damage_type: DamageType,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Message)]
pub struct AfterAttackEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub context: AttackContext,
    pub cause: ActionCause,
}

#[derive(Debug, Clone, Message)]
pub struct IncomingDamageEvent {
    pub attacker: Entity,
    pub target: Entity,
    pub amount: u32,
    pub damage_type: DamageType,
    pub cause: ActionCause,
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

#[derive(Debug, Clone, Component)]
pub struct Dead;

#[derive(Debug, Clone, Component)]
pub struct PermanentlyDead;

#[derive(Debug, Clone, Component)]
pub struct AllyDeathBehavior;

// ---------------------------------------------------------------------------
// Contract / Resurrection
// ---------------------------------------------------------------------------

/// Marker for characters bound by the Merchant's Contract. Most contract
/// rules and the resurrection system act only on entities with this marker.
#[derive(Debug, Clone, Copy, Component)]
pub struct Bound;

/// Tracks how the Merchant feels about a particular bound character. Drives
/// resurrection delay and post-return penalties (GDD Part 4).
///
/// `score` is the running performance metric. Counters are kept for diagnostics
/// and so future tuning can use raw counts instead of (or in addition to)
/// `score`.
#[derive(Debug, Clone, Component)]
pub struct ResurrectionStanding {
    pub score: i32,
    pub deaths: u32,
    pub hunts_completed: u32,
    pub hunts_failed: u32,
    pub contract_violations: u32,
}

impl Default for ResurrectionStanding {
    fn default() -> Self {
        Self {
            // Start at a comfortable baseline so the first death lands at
            // "Satisfactory" or better even with no hunts under the belt.
            score: 100,
            deaths: 0,
            hunts_completed: 0,
            hunts_failed: 0,
            contract_violations: 0,
        }
    }
}

/// The six performance tiers from GDD Part 4. Worse tiers map to longer
/// resurrection delays and harsher mental debuffs upon return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResurrectionRating {
    Exceptional,
    Satisfactory,
    Acceptable,
    Poor,
    Neglectful,
    Forfeited,
}

impl ResurrectionRating {
    pub fn from_score(score: i32) -> Self {
        match score {
            s if s >= 200 => Self::Exceptional,
            s if s >= 80 => Self::Satisfactory,
            s if s >= 0 => Self::Acceptable,
            s if s >= -80 => Self::Poor,
            s if s >= -200 => Self::Neglectful,
            _ => Self::Forfeited,
        }
    }
}

/// Component placed on a `Dead` bound character whose return is pending.
/// `process_resurrection_queue_system` watches the global Timestamp and, when
/// `ready_at_timestamp` is reached, restores the character.
#[derive(Debug, Clone, Component)]
pub struct AwaitingResurrection {
    pub ready_at_timestamp: u32,
    pub rating: ResurrectionRating,
}

#[derive(Debug, Clone, Message)]
pub struct ResurrectionRequestedEvent {
    pub who: Entity,
}

#[derive(Debug, Clone, Message)]
pub struct ResurrectedEvent {
    pub who: Entity,
    pub rating: ResurrectionRating,
}

// ---------------------------------------------------------------------------
// Reactions
// ---------------------------------------------------------------------------

/// What kind of event a reaction listens for. The trigger drives which
/// detection system fires the reaction. A character can hold multiple
/// reactions with different triggers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReactionTrigger {
    /// Fires when this character is the target of an `AttackIntentEvent`.
    /// Use cases: counters, dodges, retaliations.
    WhenAttacked,
    /// Fires when an ally within `range_meters` takes damage. Use cases:
    /// guardians, ward-bearers.
    WhenAllyDamaged { range_meters: f32 },
    /// Fires when an enemy steps inside `range_meters`. Use cases:
    /// opportunity attacks, alert zones.
    WhenEnemyEntersRange { range_meters: f32 },
}

/// A single reaction: trigger, the speed cost paid out of accumulated speed,
/// and an ability id whose effects resolve when the reaction fires. Reusing
/// `Ability` for the payload keeps reactions cheap to author — every existing
/// ability is potentially reaction-shaped.
#[derive(Debug, Clone)]
pub struct Reaction {
    pub trigger: ReactionTrigger,
    pub ability_id: u16,
    /// Portion of `AccumulatedSpeed.0` consumed when the reaction fires.
    /// Per the GDD: "Reactions consume a portion of the character's
    /// accumulated Speed, delaying their next turn."
    pub speed_cost: u32,
    /// Optional cooldown in turns. None = no cooldown.
    pub cooldown_turns: Option<u8>,
    /// Internal ticking state — turns remaining until the reaction can fire
    /// again. Authors leave at zero.
    pub cooldown_remaining: u8,
}

/// Per-character list of reactions the character can use.
#[derive(Component, Debug, Default)]
pub struct Reactions(pub Vec<Reaction>);

#[derive(Debug, Clone, Message)]
pub struct ReactionTriggeredEvent {
    pub reactor: Entity,
    pub trigger: ReactionTrigger,
    pub ability_id: u16,
    /// The entity whose action caused the trigger (attacker, mover, damaged
    /// ally). `None` when the trigger has no clear cause (rare).
    pub catalyst: Option<Entity>,
}

/// Ticks down `cooldown_remaining` on every reaction owned by the entity
/// whose turn just ended.
fn reaction_cooldown_tick_system(
    mut reader: MessageReader<TurnEndEvent>,
    mut q: Query<&mut Reactions>,
) {
    for ev in reader.read() {
        if let Ok(mut reactions) = q.get_mut(ev.who) {
            for r in reactions.0.iter_mut() {
                if r.cooldown_remaining > 0 {
                    r.cooldown_remaining -= 1;
                }
            }
        }
    }
}

/// `WhenAttacked` evaluator. Fires reactions on the *target* of every attack
/// intent. The reaction effect lands as a same-frame attack from the target
/// back at the attacker (counter pattern).
fn evaluate_when_attacked_reactions_system(
    mut reader: MessageReader<AttackIntentEvent>,
    mut q: Query<(&mut Reactions, &mut AccumulatedSpeed)>,
    mut writer: MessageWriter<ReactionTriggeredEvent>,
) {
    for ev in reader.read() {
        let Ok((mut reactions, mut acc)) = q.get_mut(ev.target) else {
            continue;
        };
        for r in reactions.0.iter_mut() {
            if !matches!(r.trigger, ReactionTrigger::WhenAttacked) {
                continue;
            }
            if r.cooldown_remaining > 0 {
                continue;
            }
            if (acc.0 as u32) < r.speed_cost {
                continue;
            }
            acc.0 = acc.0.saturating_sub(r.speed_cost);
            r.cooldown_remaining = r.cooldown_turns.unwrap_or(0);
            writer.write(ReactionTriggeredEvent {
                reactor: ev.target,
                trigger: r.trigger,
                ability_id: r.ability_id,
                catalyst: Some(ev.attacker),
            });
            // Only fire one reaction per inciting event per character.
            break;
        }
    }
}

/// `WhenAllyDamaged` evaluator. Listens to resolved damage events and fires
/// guardian-style reactions on allies within range. "Ally" today means
/// "shares a `BattleSide` (Ally)"; the side check is a thin proxy until a
/// fuller faction system lands.
fn evaluate_when_ally_damaged_reactions_system(
    mut reader: MessageReader<DamageEvent>,
    side_q: Query<&crate::battle::BattleSide>,
    transform_q: Query<&Transform>,
    mut q: Query<(Entity, &mut Reactions, &mut AccumulatedSpeed)>,
    mut writer: MessageWriter<ReactionTriggeredEvent>,
) {
    for ev in reader.read() {
        let Ok(victim_side) = side_q.get(ev.target) else {
            continue;
        };
        let Ok(victim_tf) = transform_q.get(ev.target) else {
            continue;
        };
        for (reactor, mut reactions, mut acc) in q.iter_mut() {
            if reactor == ev.target {
                continue;
            }
            // Same side check.
            let Ok(reactor_side) = side_q.get(reactor) else {
                continue;
            };
            if reactor_side != victim_side {
                continue;
            }
            let Ok(reactor_tf) = transform_q.get(reactor) else {
                continue;
            };
            for r in reactions.0.iter_mut() {
                let ReactionTrigger::WhenAllyDamaged { range_meters } = r.trigger else {
                    continue;
                };
                if r.cooldown_remaining > 0 {
                    continue;
                }
                if (acc.0 as u32) < r.speed_cost {
                    continue;
                }
                let distance = reactor_tf.translation.distance(victim_tf.translation);
                if distance > range_meters {
                    continue;
                }
                acc.0 = acc.0.saturating_sub(r.speed_cost);
                r.cooldown_remaining = r.cooldown_turns.unwrap_or(0);
                writer.write(ReactionTriggeredEvent {
                    reactor,
                    trigger: r.trigger,
                    ability_id: r.ability_id,
                    catalyst: Some(ev.attacker),
                });
                break;
            }
        }
    }
}

/// Resolves a fired reaction by looking up its ability and queuing its
/// effects against `catalyst`. Routes through the existing
/// `AttackIntentEvent` pipeline so reactions naturally feed into hit-rolls,
/// status modifiers, etc.
fn resolve_reaction_intent_system(
    mut reader: MessageReader<ReactionTriggeredEvent>,
    ability_tree: Option<Res<Ability_Tree>>,
    mut intent_writer: MessageWriter<AttackIntentEvent>,
) {
    let Some(tree) = ability_tree else {
        return;
    };
    for ev in reader.read() {
        let Some(ability) = tree.0.find(ev.ability_id) else {
            warn!(
                "Reaction by {:?} references unknown ability id {}",
                ev.reactor, ev.ability_id
            );
            continue;
        };
        let Some(catalyst) = ev.catalyst else {
            continue;
        };
        intent_writer.write(AttackIntentEvent {
            attacker: ev.reactor,
            target: catalyst,
            ability: Some(ability.clone()),
            context: AttackContext::default(),
            cause: ActionCause::Reaction { reactor: ev.reactor, ability_id: ability.id },
        });
    }
}


/// Context shared along the attack pipeline; systems may mutate `meta` or read values.
#[derive(Debug, Clone)]
pub struct AttackContext {
    pub base_lethality: i32,
    pub base_hit: i32,
    pub extra_flat_damage: i32,
    pub damage_type: Option<DamageType>,
    pub weapon: Option<Entity>,
    pub multipliers: Vec<StatModifier>, // trackers for multiplicative modifiers applied during flow
}

impl Default for AttackContext {
    fn default() -> Self {
        Self {
            base_lethality: 0,
            base_hit: 0,
            extra_flat_damage: 0,
            damage_type: None,
            weapon: None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemUseTrigger {
    Manual,
    PreDeath,
}

#[derive(Debug, Clone, Copy)]
pub enum ConsumableEffect {
    Heal { amount: u32 },
}

#[derive(Debug, Clone)]
pub enum InventoryItemKind {
    Consumable {
        effect: ConsumableEffect,
        usable_on_others: bool,
        usable_pre_death: bool,
    },
    Equipment(EquipmentType),
}

#[derive(Debug, Clone)]
pub struct InventoryItemDefinition {
    pub id: u16,
    pub name: String,
    pub kind: InventoryItemKind,
}

#[derive(Resource, Debug, Clone)]
pub struct InventoryItemCatalog(pub HashMap<u16, InventoryItemDefinition>);

impl Default for InventoryItemCatalog {
    fn default() -> Self {
        let mut items = HashMap::new();
        items.insert(
            1001,
            InventoryItemDefinition {
                id: 1001,
                name: "Field Medicine".to_string(),
                kind: InventoryItemKind::Consumable {
                    effect: ConsumableEffect::Heal { amount: 35 },
                    usable_on_others: true,
                    usable_pre_death: false,
                },
            },
        );
        items.insert(
            1002,
            InventoryItemDefinition {
                id: 1002,
                name: "Last Chance Tonic".to_string(),
                kind: InventoryItemKind::Consumable {
                    effect: ConsumableEffect::Heal { amount: 50 },
                    usable_on_others: false,
                    usable_pre_death: true,
                },
            },
        );
        Self(items)
    }
}

#[derive(Debug, Clone, Message)]
pub struct UseItemIntentEvent {
    pub user: Entity,
    pub item_id: u16,
    pub target: Option<Entity>,
    pub trigger: ItemUseTrigger,
}

#[derive(Debug, Clone, Message)]
pub struct GiveItemIntentEvent {
    pub giver: Entity,
    pub receiver: Entity,
    pub item_id: u16,
}

#[derive(Debug, Clone, Message)]
pub struct ItemTransferredEvent {
    pub giver: Entity,
    pub receiver: Entity,
    pub item_id: u16,
}

#[derive(Debug, Clone, Message)]
pub struct ItemUsedEvent {
    pub user: Entity,
    pub target: Entity,
    pub item_id: u16,
    pub trigger: ItemUseTrigger,
}

pub fn request_use_item(
    writer: &mut MessageWriter<UseItemIntentEvent>,
    user: Entity,
    item_id: u16,
    target: Option<Entity>,
    trigger: ItemUseTrigger,
) {
    writer.write(UseItemIntentEvent {
        user,
        item_id,
        target,
        trigger,
    });
}

pub fn request_give_item(
    writer: &mut MessageWriter<GiveItemIntentEvent>,
    giver: Entity,
    receiver: Entity,
    item_id: u16,
) {
    writer.write(GiveItemIntentEvent {
        giver,
        receiver,
        item_id,
    });
}

#[derive(Debug, Clone)]
pub struct LootItem {
    pub id: u16,
    pub quantity: u32,
}

#[derive(Debug, Clone, Resource)]
pub struct BattleLoot {
    pub items: Vec<LootItem>,
}

#[derive(Debug, Clone, Default, Resource)]
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

/// Request to bring a temporary combatant onto the field beside `summoner`.
/// Emitted by [`crate::combat_ability::handle_ability`] for `Summon` effects
/// and consumed by `crate::battle::resolve_summon_system`, which has the
/// `Commands` needed to spawn the unit and slot it into turn order.
#[derive(Debug, Clone, Message)]
pub struct SummonEvent {
    pub summoner: Entity,
    pub kind: SummonKind,
    pub lifetime_turns: u8,
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
        loot_writer.write(LootEvent {
            loot: self.loot_table.clone(),
            dropped_by: entity,
        });

        // Award XP to killer if exists
        if let Some(killer) = killer {
            xp_writer.write(AwardXpEvent {
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
            // Levels are capped at MAX_LEVEL (30); the high bits of `xp` encode
            // the raw level, so clamp before it ever leaves this system.
            let new_level = ((xp.0 >> 16) as u8).min(crate::combat_ability::MAX_LEVEL);
            events_level.write(LevelUpEvent {
                who: evt.recipient,
                old_level: lvl.0 as u8,
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
    mut q: Query<(&mut ExtraHp, &mut CombatStats), With<SpiritMediumBehavior>>,
    mut dmg_queue: ResMut<DamageQueue>,
) {
    for ev in incoming.read() {
        if let Ok((mut extra, mut stats)) = q.get_mut(ev.target) {

            let mut dmg = ev.amount;

            // absorb from extra hp
            let absorbed = dmg.min(extra.current);
            extra.current -= absorbed;
            dmg -= absorbed;

            if dmg == 0 {
                dmg_queue.0.push(QueuedDamage {
                    attacker: ev.attacker,
                    target: ev.target,
                    amount: 0,
                    damage_type: ev.damage_type,
                    scaled_with: vec![],
                    defended_with: vec![],
                    accuracy_override: None,
                    crit_multiplier: 1.0,
                    tags: vec![],
                    cause: ev.cause.clone(),
                });
                continue;
            }

            // apply remaining to normal HP
            let applied = stats.health.current.min(dmg as i32);
            stats.health.current -= applied;

            dmg_queue.0.push(QueuedDamage {
                attacker: ev.attacker,
                target: ev.target,
                amount: applied,
                damage_type: ev.damage_type,
                scaled_with: vec![],
                defended_with: vec![],
                accuracy_override: None,
                crit_multiplier: 1.0,
                tags: vec![],
                cause: ev.cause.clone(),
            });
        }
    }
}

fn paladin_before_attack_system(
    mut events: MessageMutator<BeforeAttackEvent>,
    mut paladins: Query<(), With<PaladinBehavior>>,
) {
    for ev in events.read() {
        if paladins.get(ev.attacker).is_ok() {
            ev.context.base_hit =
                (ev.context.base_hit as f32 * 1.10) as i32;
        }
    }
}

fn paladin_damage_reduction_system(
    mut incoming: MessageReader<IncomingDamageEvent>,
    paladins: Query<(), With<PaladinBehavior>>,
    mut dmg_queue: ResMut<DamageQueue>,
) {
    for ev in incoming.read() {
        if paladins.get(ev.target).is_ok() {
            let reduced = ev.amount.saturating_sub(1);
            dmg_queue.0.push(QueuedDamage {
                attacker: ev.attacker,
                target: ev.target,
                amount: reduced as i32,
                damage_type: ev.damage_type,
                scaled_with: vec![],
                defended_with: vec![],
                accuracy_override: None,
                crit_multiplier: 1.0,
                tags: vec![],
                cause: ev.cause.clone(),
            });
        }
    }
}

fn rogue_backstab_system(
    mut events: MessageMutator<BeforeAttackEvent>,
    rogues: Query<&Transform, With<RogueBehavior>>,
    targets: Query<&Transform>,
) {
    for ev in events.read() {
        if let Ok(rogue_tf) = rogues.get(ev.attacker) {
            if let Ok(target_tf) = targets.get(ev.target) {
                let dir = target_tf.translation - rogue_tf.translation;
                let back = target_tf.forward();

                if dir.length() < 2.0 && dir.dot(*back) > 0.8 {
                    ev.context.base_lethality += 20;
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
    for ev in incoming.read() {
        if let Ok(stats) = rogues.get(ev.target) {
            let chance = stats.evasion.current as f32 / 100.0;
            if rand::random::<f32>() < chance {
                // Dodged → send 0 damage
                dmg_queue.0.push(QueuedDamage {
                    attacker: ev.attacker,
                    target: ev.target,
                    amount: 0,
                    damage_type: ev.damage_type,
                    scaled_with: vec![],
                    defended_with: vec![],
                    accuracy_override: None,
                    crit_multiplier: 1.0,
                    tags: vec![],
                    cause: ev.cause.clone(),
                });
                continue;
            }
        }

        // not dodged → push normal damage
        dmg_queue.0.push(QueuedDamage {
            attacker: ev.attacker,
            target: ev.target,
            amount: ev.amount as i32,
            damage_type: ev.damage_type,
            scaled_with: vec![],
            defended_with: vec![],
            accuracy_override: None,
            crit_multiplier: 1.0,
            tags: vec![],
            cause: ev.cause.clone(),
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
    loadout_q: Query<&EquipmentLoadout>,
    mut commands: Commands,
    mut modifiers_q: Query<&mut StatModifiers>,
    timestamp: Res<Timestamp>,
) {
    for ev in befores.iter() {
        if let Ok(loadout) = loadout_q.get(ev.attacker) {
            for equipped_item in loadout.equipped_items() {
                if let Ok((equip_entity, _equip, hooks)) = equipment_q.get(equipped_item) {
                    for hook in &hooks.0 {
                        match hook {
                            EquipHook::BeforeAttackMultiplier {
                                stat,
                                multiplier,
                                duration_turns,
                            } => {
                                let modifier = StatModifier {
                                    stat: *stat,
                                    multiplier: *multiplier,
                                    expires_at_timestamp: Some(
                                        timestamp.0.saturating_add(*duration_turns),
                                    ),
                                    source: Some(equip_entity),
                                };

                                if let Ok(mut modifiers) = modifiers_q.get_mut(ev.attacker) {
                                    modifiers.0.push(modifier);
                                } else {
                                    commands.entity(ev.attacker).insert(StatModifiers(vec![modifier]));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn weapon_before_attack_effect_system(
    mut events: MessageMutator<BeforeAttackEvent>,
    loadout_q: Query<&EquipmentLoadout>,
    effects_q: Query<&WeaponBeforeAttackEffects>,
    sharpness_q: Query<&WeaponSharpness>,
    mut commands: Commands,
    mut modifiers_q: Query<&mut StatModifiers>,
    timestamp: Res<Timestamp>,
) {
    for ev in events.read() {
        let Some(weapon_entity) = loadout_q
            .get(ev.attacker)
            .ok()
            .and_then(|loadout| loadout.equipped_in_slot(EquipmentSlotType::Weapon))
        else {
            continue;
        };

        ev.context.weapon = Some(weapon_entity);

        let sharpness = sharpness_q
            .get(weapon_entity)
            .map(|s| s.current.min(100))
            .unwrap_or(100);

        if let Ok(effects) = effects_q.get(weapon_entity) {
            for effect in &effects.0 {
                match effect {
                    WeaponBeforeAttackEffect::AddFlatDamage { flat } => {
                        ev.context.extra_flat_damage += *flat;
                    }
                    WeaponBeforeAttackEffect::MultiplyBaseDamage { multiplier } => {
                        ev.context.base_lethality =
                            ((ev.context.base_lethality as f32) * *multiplier).round() as i32;
                    }
                    WeaponBeforeAttackEffect::AddHit { amount } => {
                        ev.context.base_hit += *amount;
                    }
                    WeaponBeforeAttackEffect::OverrideDamageType { damage_type } => {
                        ev.context.damage_type = Some(*damage_type);
                    }
                    WeaponBeforeAttackEffect::ApplyTemporaryStatModifier {
                        stat,
                        multiplier,
                        duration_turns,
                    } => {
                        let modifier = StatModifier {
                            stat: *stat,
                            multiplier: *multiplier,
                            expires_at_timestamp: Some(
                                timestamp.0.saturating_add(*duration_turns),
                            ),
                            source: Some(weapon_entity),
                        };

                        if let Some(existing) = ev
                            .context
                            .multipliers
                            .iter_mut()
                            .find(|m| m.stat == *stat && m.source == Some(weapon_entity))
                        {
                            existing.multiplier *= *multiplier;
                        } else {
                            ev.context.multipliers.push(modifier.clone());
                        }

                        if let Ok(mut modifiers) = modifiers_q.get_mut(ev.attacker) {
                            modifiers.0.push(modifier);
                        } else {
                            commands
                                .entity(ev.attacker)
                                .insert(StatModifiers(vec![modifier]));
                        }
                    }
                    WeaponBeforeAttackEffect::BonusWhenSharp {
                        minimum_sharpness,
                        flat_damage,
                    } => {
                        if sharpness >= *minimum_sharpness {
                            ev.context.extra_flat_damage += *flat_damage;
                        }
                    }
                }
            }
        }
    }
}

fn dull_weapon_on_attack_system(
    mut events: MessageReader<BeforeAttackEvent>,
    loadout_q: Query<&EquipmentLoadout>,
    mut sharpness_q: Query<&mut WeaponSharpness>,
) {
    for ev in events.iter() {
        let Some(weapon_entity) = loadout_q
            .get(ev.attacker)
            .ok()
            .and_then(|loadout| loadout.equipped_in_slot(EquipmentSlotType::Weapon))
        else {
            continue;
        };

        if let Ok(mut sharpness) = sharpness_q.get_mut(weapon_entity) {
            sharpness.dull_on_attack();
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
            ability: ev.ability.clone(),
            context: ev.context.clone(),
            cause: ev.cause.clone(),
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
            ability: ev.ability.clone(),
            context: ev.context.clone(),
            cause: ev.cause.clone(),
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
    mut intents: MessageReader<AttackIntentEvent>,
    mut before_attacks: MessageWriter<BeforeAttackEvent>,
) {
    for intent in intents.iter() {
        before_attacks.send(BeforeAttackEvent {
            attacker: intent.attacker,
            target: intent.target,
            ability: intent.ability.clone(),
            context: intent.context.clone(),
            cause: intent.cause.clone(),
        });
    }
}

/// At TurnStart, if the actor's `ActionGates` say their turn must be
/// forfeited (Terrified T3), zero their AP and end the turn immediately.
/// All status-driven action overrides flow through `action_gates`, so
/// other forfeit-causing statuses added later don't need a new system —
/// just a new field on `ActionGates`.
fn forfeit_turn_on_status_system(
    mut reader: MessageReader<TurnStartEvent>,
    mut stats_q: Query<&mut CombatStats>,
    status_q: Query<&crate::status_effects::StatusEffects>,
    mut turn_end_writer: MessageWriter<TurnEndEvent>,
    mut turn_in_progress: ResMut<TurnInProgress>,
) {
    for ev in reader.read() {
        let gates = crate::status_effects::action_gates(status_q.get(ev.who).ok());
        if gates.forfeit_turn {
            if let Ok(mut stats) = stats_q.get_mut(ev.who) {
                stats.action_points.current = 0;
            }
            turn_end_writer.write(TurnEndEvent { who: ev.who });
            turn_in_progress.0 = false;
            info!("ActionGates::forfeit_turn: {:?} loses turn.", ev.who);
        }
    }
}

/// `BeforeAttackEvent` mutator that applies any `ActionGates`-driven retarget
/// to the attack before damage is queued. Today only Confused triggers
/// retargeting (rolled probability picks the attacker's nearest ally as the
/// new target); future overrides that change *who* gets hit can plug into
/// `ActionGates` and reuse this system without forking it.
fn apply_retarget_overrides_system(
    mut events: MessageMutator<BeforeAttackEvent>,
    status_q: Query<&crate::status_effects::StatusEffects>,
    transform_q: Query<&Transform>,
    sides_iter_q: Query<(Entity, &crate::battle::BattleSide)>,
) {
    for ev in events.iter_mut() {
        let gates = crate::status_effects::action_gates(status_q.get(ev.attacker).ok());
        if gates.confused_retarget_chance <= 0.0
            || rand::random::<f32>() >= gates.confused_retarget_chance
        {
            continue;
        }

        let Ok(my_tf) = transform_q.get(ev.attacker) else {
            continue;
        };
        let Ok((_, my_side)) = sides_iter_q.get(ev.attacker) else {
            continue;
        };
        let mut best: Option<(Entity, f32)> = None;
        for (other, side) in sides_iter_q.iter() {
            if other == ev.attacker || side != my_side {
                continue;
            }
            let Ok(tf) = transform_q.get(other) else {
                continue;
            };
            let d = tf.translation.distance_squared(my_tf.translation);
            match best {
                None => best = Some((other, d)),
                Some((_, b)) if d < b => best = Some((other, d)),
                _ => {}
            }
        }
        if let Some((ally, _)) = best {
            info!(
                "Confused retarget ({}%): {:?} now attacks ally {:?} instead of {:?}",
                (gates.confused_retarget_chance * 100.0) as u8,
                ev.attacker,
                ally,
                ev.target,
            );
            ev.target = ally;
        }
    }
}

fn queue_damage_from_before_attack(
    mut dq: ResMut<DamageQueue>,
    mut befores: MessageReader<BeforeAttackEvent>,
    stats_q: Query<&CombatStats>,
    modifiers_q: Query<&StatModifiers>,
    targets_stats_q: Query<&CombatStats>,
    loadout_q: Query<&EquipmentLoadout>,
    equipment_q: Query<&Equipment>,
    sharpness_q: Query<&WeaponSharpness>,
    status_q: Query<&crate::status_effects::StatusEffects>,
    sides_q: Query<(Entity, &crate::battle::BattleSide)>,
) {
    for ev in befores.iter() {
        let attacker = ev.attacker;
        let target = ev.target;

        let att_stats = stats_q.get(attacker).ok();
        // Read `current` rather than `base`: status effects and (future) buffs
        // already feed into `current` via the recompute pass.
        let mut base_leth = ev.context.base_lethality;
        if base_leth == 0 {
            base_leth = att_stats.map(|s| s.lethality.current).unwrap_or(0);
        }
        let mut base_hit = ev.context.base_hit;
        if base_hit == 0 {
            base_hit = att_stats.map(|s| s.hit.current).unwrap_or(50);
        }
        let mut flat = ev.context.extra_flat_damage;

        if let Ok(mods) = modifiers_q.get(attacker) {
            for m in &mods.0 {
                match m.stat {
                    Stat::Lethality => {
                        base_leth = ((base_leth as f32) * m.multiplier).round() as i32;
                    }
                    Stat::Hit => {
                        base_hit = ((base_hit as f32) * m.multiplier).round() as i32;
                    }
                    _ => {}
                }
            }
        }

        // Hit-chance shifts from status (Unfocused on attacker; Unlucky and
        // Crippled Defense on target). Lethality/hit multipliers are already
        // baked into `*.current` via the recompute pass.
        let outgoing = crate::status_effects::outgoing_mods(status_q.get(attacker).ok());
        let incoming_for_hit = crate::status_effects::incoming_mods(
            status_q.get(target).ok(),
            ev.context.damage_type.unwrap_or(DamageType::Physical),
        );

        let mut scaled_with: Vec<(Stat, f32)> = Vec::new();
        let mut defended_with: Vec<(Stat, f32)> = Vec::new();

        if let Some(ability) = ev.ability.as_ref() {
            for eff in &ability.effects {
                match eff {
                    AbilityEffect::Damage {
                        scaled_with: sw,
                        defended_with: dw,
                        ..
                    } => {
                        scaled_with.push((*sw, 1.0));
                        defended_with.push((*dw, 1.0));
                    }
                    AbilityEffect::Heal { .. }
                    | AbilityEffect::Buff { .. }
                    | AbilityEffect::ApplyStatus { .. }
                    | AbilityEffect::RemoveStatus { .. }
                    | AbilityEffect::Summon { .. } => {}
                }
            }
        }

        if scaled_with.is_empty() {
            scaled_with.push((Stat::Lethality, 1.0));
        }
        if defended_with.is_empty() {
            defended_with.push((Stat::Armor, 1.0));
        }

        if let Some(weapon_entity) = ev
            .context
            .weapon
            .or_else(|| {
                loadout_q
                    .get(attacker)
                    .ok()
                    .and_then(|loadout| loadout.equipped_in_slot(EquipmentSlotType::Weapon))
            })
        {
            if let Ok(weapon) = equipment_q.get(weapon_entity) {
                base_leth += weapon.lethality;
                base_hit += weapon.hit;
                flat += weapon.agility.max(0) / 2;
            }
        }

        if let Some(a_stats) = att_stats {
            for (stat, mult) in &scaled_with {
                let val = get_stat_value(*stat, Some(a_stats));
                base_leth += (val as f32 * *mult / 10.0).round() as i32;
            }
        }

        let mut pre_def_damage = (base_leth + flat).max(0);

        if let Some(weapon_entity) = ev
            .context
            .weapon
            .or_else(|| {
                loadout_q
                    .get(attacker)
                    .ok()
                    .and_then(|loadout| loadout.equipped_in_slot(EquipmentSlotType::Weapon))
            })
        {
            if let Ok(sharpness) = sharpness_q.get(weapon_entity) {
                pre_def_damage =
                    ((pre_def_damage as f32) * sharpness.damage_multiplier()).round() as i32;
            }
        }

        let attacker_hit_f = base_hit as f32;
        let target_evasion_f = targets_stats_q
            .get(target)
            .map(|t| t.evasion.current as f32)
            .unwrap_or(0.0);
        let mut chance =
            1.0 / (1.0 + (-HIT_CHANCE_LOGISTIC_K * (attacker_hit_f - target_evasion_f)).exp());

        // Lucky (buff on attacker's allies) and Unlucky (debuff on target)
        // share one signed helper; both shift the attacker's hit chance up.
        let luck_shift =
            crate::status_effects::lucky_unlucky_shift(attacker, target, &sides_q, &status_q);

        chance = (chance
            + outgoing.hit_chance_shift
            + incoming_for_hit.attacker_hit_chance_shift
            + luck_shift)
            .clamp(0.0, 1.0);

        let roll = rand::random::<f32>();
        if roll > chance {
            dq.0.push(QueuedDamage {
                attacker,
                target,
                amount: DamageSignal::Miss as i32,
                damage_type: ev.context.damage_type.unwrap_or(DamageType::Physical),
                scaled_with: vec![],
                defended_with: vec![],
                accuracy_override: None,
                crit_multiplier: 1.0,
                tags: vec![],
                cause: ev.cause.clone(),
            });
            continue;
        }

        // Critical hit: roll landed in the top fraction of the hit window —
        // a "barely landed" lucky shot. Crit damage stacks multiplicatively
        // with weakness in `process_damage_queue_system`.
        let (crit_multiplier, tags) = if roll >= chance * (1.0 - CRITICAL_HIT_FRACTION) {
            (CRITICAL_HIT_DAMAGE_MULTIPLIER, vec![DamageTag::Critical])
        } else {
            (1.0, Vec::new())
        };

        dq.0.push(QueuedDamage {
            attacker,
            target,
            amount: pre_def_damage,
            damage_type: ev.context.damage_type.unwrap_or(DamageType::Physical),
            scaled_with: vec![],
            defended_with,
            accuracy_override: None,
            crit_multiplier,
            tags,
            cause: ev.cause.clone(),
        });
    }
}


fn process_damage_queue_system(
    mut dq: ResMut<DamageQueue>,
    stats_q: Query<&CombatStats>,
    mut status_q: Query<&mut crate::status_effects::StatusEffects>,
    weaknesses_q: Query<&DamageWeaknesses>,
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
                    amount: i32::MAX,
                    damage_type: entry.damage_type,
                    cause: entry.cause.clone(),
                });
                continue;
            }
            // If less than 0 but not one of the above, it's an error
            _ => {}
        }

        // FETCH STATS --------------------------------------------------------
        let atk = stats_q.get(entry.attacker).ok();
        let tgt = stats_q.get(entry.target).ok();

        // Target-side status modifiers (Fragile / Broken Body / Crippled
        // Defense armor mult / Haunted on mental damage).
        let target_status_view = status_q.get(entry.target).ok();
        let inc = crate::status_effects::incoming_mods(
            target_status_view.as_deref(),
            entry.damage_type,
        );

        // SCALING ------------------------------------------------------------
        if let Some(a) = atk {
            for (stat, mult) in &entry.scaled_with {
                entry.amount += (get_stat_value(*stat, Some(a)) as f32 * mult) as i32;
            }
        }

        // DEFENSE -------------------------------------------------------------
        if let Some(t) = tgt {
            for (stat, mult) in &entry.defended_with {
                let raw = get_stat_value(*stat, Some(t)) as f32 * mult;
                let scaled = if matches!(stat, Stat::Armor) {
                    raw * inc.armor_mult
                } else {
                    raw
                };
                entry.amount -= scaled as i32;
            }
        }

        // INCOMING MULTIPLIERS (Fragile, Broken Body, Haunted) ---------------
        entry.amount = ((entry.amount as f32) * inc.damage_mult).round() as i32;

        // EXPOSED is consumed on hit (one-shot multiplier, regardless of tier
        // duration which is only 1 turn anyway).
        if entry.amount > 0 {
            if let Ok(mut se) = status_q.get_mut(entry.target) {
                let exposed_mult = crate::status_effects::consume_exposed(&mut se);
                if (exposed_mult - 1.0).abs() > f32::EPSILON {
                    entry.amount = ((entry.amount as f32) * exposed_mult).round() as i32;
                }
            }
        }

        // CRIT × WEAKNESS — both are multiplicative final modifiers, so they
        // stack. Defaults are 1.0 (no-op) when neither applies.
        let weakness_mult = weaknesses_q
            .get(entry.target)
            .map(|w| w.multiplier_for(entry.damage_type))
            .unwrap_or(1.0);
        let final_mult = entry.crit_multiplier * weakness_mult;
        if (final_mult - 1.0).abs() > f32::EPSILON {
            entry.amount = ((entry.amount as f32) * final_mult).round() as i32;
        }

        entry.amount = entry.amount.max(0);

        // FINAL DAMAGE --------------------------------------------------------
        damage_writer.send(DamageEvent {
            attacker: entry.attacker,
            target: entry.target,
            amount: entry.amount,
            damage_type: entry.damage_type,
            cause: entry.cause.clone(),
        });
    }
}

fn apply_consumable_effect_to_health(
    target: Entity,
    effect: ConsumableEffect,
    stats_q: &mut Query<&mut CombatStats>,
) -> bool {
    let Ok(mut stats) = stats_q.get_mut(target) else {
        return false;
    };

    match effect {
        ConsumableEffect::Heal { amount } => {
            stats.health.restore_to_base(amount as i32);
            true
        }
    }
}

fn find_pre_death_item(inventory: &Inventory, item_catalog: &InventoryItemCatalog) -> Option<u16> {
    inventory.item_ids.iter().copied().find(|item_id| {
        matches!(
            item_catalog.0.get(item_id).map(|item| &item.kind),
            Some(InventoryItemKind::Consumable {
                usable_pre_death: true,
                ..
            })
        )
    })
}

fn try_use_pre_death_item(
    target: Entity,
    killer: Entity,
    inventory_q: &mut Query<&mut Inventory>,
    stats_q: &mut Query<&mut CombatStats>,
    item_catalog: &InventoryItemCatalog,
    item_used_writer: &mut MessageWriter<ItemUsedEvent>,
) -> bool {
    let Ok(mut inventory) = inventory_q.get_mut(target) else {
        return false;
    };

    let Some(item_id) = find_pre_death_item(&inventory, item_catalog) else {
        return false;
    };

    let Some(item_def) = item_catalog.0.get(&item_id) else {
        return false;
    };

    let InventoryItemKind::Consumable {
        effect,
        usable_pre_death: true,
        ..
    } = item_def.kind
    else {
        return false;
    };

    if !inventory.remove_item(item_id) {
        return false;
    }

    if apply_consumable_effect_to_health(target, effect, stats_q) {
        item_used_writer.write(ItemUsedEvent {
            user: target,
            target,
            item_id,
            trigger: ItemUseTrigger::PreDeath,
        });
        info!(
            "Entity {:?} used {} ({}) before death against attacker {:?}",
            target, item_def.name, item_def.id, killer
        );
        return true;
    }

    inventory.add_item(item_id);
    false
}


/// Apply damage to target's Health and emit AfterHitEvent
pub fn apply_damage_system(
    mut reader: MessageReader<DamageEvent>,
    mut stats_q: Query<&mut CombatStats>,
    mut inventory_q: Query<&mut Inventory>,
    item_catalog: Res<InventoryItemCatalog>,
    mut after_writer: MessageWriter<AfterHitEvent>,
    mut item_used_writer: MessageWriter<ItemUsedEvent>,
    mut death_writer: MessageWriter<DeathEvent>,
) {
    for ev in reader.iter() {
        if let Ok(mut stats) = stats_q.get_mut(ev.target) {
            let before = stats.health.current;
            stats.health.current = stats.health.current.saturating_sub(ev.amount);
            let applied = before - stats.health.current;
            let lethal = stats.health.current == 0;
            drop(stats);

            if lethal {
                let _ = try_use_pre_death_item(
                    ev.target,
                    ev.attacker,
                    &mut inventory_q,
                    &mut stats_q,
                    &item_catalog,
                    &mut item_used_writer,
                );
            }

            after_writer.send(AfterHitEvent {
                attacker: ev.attacker,
                target: ev.target,
                amount: applied,
                damage_type: ev.damage_type,
                cause: ev.cause.clone(),
            });

            if let Ok(stats) = stats_q.get(ev.target) {
                if stats.health.current == 0 {
                    death_writer.send(DeathEvent {
                        entity: ev.target,
                        killer: Some(ev.attacker),
                    });
                }
            }
        }
    }
}

fn apply_heal_system(
    mut reader: MessageReader<HealEvent>,
    mut stats_q: Query<&mut CombatStats>,
    status_q: Query<&crate::status_effects::StatusEffects>,
) {
    for ev in reader.iter() {
        if let Ok(mut stats) = stats_q.get_mut(ev.target) {
            let gate = crate::status_effects::heal_gate(status_q.get(ev.target).ok());
            let amount = ((ev.amount as f32) * gate.mult).round() as i32;
            if amount > 0 {
                stats.health.restore_to_base(amount);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Resurrection
// ---------------------------------------------------------------------------

/// When a `Bound` character dies, drop their performance score, schedule a
/// return at `current + delay` with mental-debuff payload determined by
/// performance tier. Non-bound characters are unaffected (they stay dead).
///
/// Subtle: the actual mental-debuff application happens at *return* time, in
/// `process_resurrection_queue_system`, so that the debuff durations are
/// counted from the moment the character is back, not from the moment they
/// fell.
fn enqueue_resurrection_on_death_system(
    mut commands: Commands,
    mut reader: MessageReader<DeathEvent>,
    mut q_standing: Query<(&Bound, &mut ResurrectionStanding)>,
    timestamp: Res<Timestamp>,
    mut writer: MessageWriter<ResurrectionRequestedEvent>,
) {
    for ev in reader.read() {
        let Ok((_bound, mut standing)) = q_standing.get_mut(ev.entity) else {
            continue;
        };
        standing.deaths = standing.deaths.saturating_add(1);
        // Each death drops score by 20. Hunt success/failure events tune it
        // further; this is the pure death penalty.
        standing.score = standing.score.saturating_sub(20);

        let rating = ResurrectionRating::from_score(standing.score);
        let delay_hours = match rating {
            ResurrectionRating::Exceptional => 0,
            ResurrectionRating::Satisfactory => 1,
            ResurrectionRating::Acceptable => 6,
            ResurrectionRating::Poor => 24,
            ResurrectionRating::Neglectful => 24 * 3,
            ResurrectionRating::Forfeited => 24 * 7,
        };
        let ready_at = timestamp
            .0
            .saturating_add(delay_hours * crate::constants::TIMESTAMP_TICKS_PER_HOUR);

        commands.entity(ev.entity).insert(AwaitingResurrection {
            ready_at_timestamp: ready_at,
            rating,
        });
        writer.write(ResurrectionRequestedEvent { who: ev.entity });
        info!(
            "Resurrection enqueued for {:?}: rating {:?}, ready in {}h",
            ev.entity, rating, delay_hours
        );
    }
}

/// Marker for any map location where a resurrected Bound character returns.
/// Spawn one (or more) at world setup; `teleport_on_resurrection` snaps the
/// resurrected entity to the closest one.
#[derive(Component)]
pub struct ResurrectionPoint;

/// On `ResurrectedEvent`, snap the resurrected entity's `Transform` to the
/// closest `ResurrectionPoint` on the map. Without this, restored
/// characters would wake up wherever they fell — which fights the design
/// intent that resurrection is a return to a fixed sanctuary on the map.
///
/// Works for any entity that received `ResurrectedEvent` (player, party
/// member, future bound NPC), not specifically the player.
pub fn teleport_on_resurrection(
    mut events: MessageReader<ResurrectedEvent>,
    shrine_q: Query<&Transform, With<ResurrectionPoint>>,
    mut transforms_q: Query<&mut Transform, Without<ResurrectionPoint>>,
) {
    for ev in events.read() {
        let Ok(mut tf) = transforms_q.get_mut(ev.who) else {
            continue;
        };
        let pos = tf.translation.truncate();
        let target = shrine_q
            .iter()
            .min_by(|a, b| {
                let da = a.translation.truncate().distance_squared(pos);
                let db = b.translation.truncate().distance_squared(pos);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|t| t.translation);
        let Some(target_pos) = target else {
            warn!("teleport_on_resurrection: no ResurrectionPoint exists in the world");
            continue;
        };
        tf.translation.x = target_pos.x;
        tf.translation.y = target_pos.y;
        info!("teleport_on_resurrection: {:?} → {:?}", ev.who, target_pos);
    }
}

/// Watches the global Timestamp and resurrects any `AwaitingResurrection`
/// entity whose deadline has passed. Restores Health to base and emits a
/// `ResurrectedEvent` so downstream systems (status effects, dialogue,
/// reputation) can react.
fn process_resurrection_queue_system(
    mut commands: Commands,
    timestamp: Res<Timestamp>,
    mut q: Query<(Entity, &AwaitingResurrection, &mut CombatStats)>,
    mut writer: MessageWriter<ResurrectedEvent>,
    mut status_writer: MessageWriter<crate::status_effects::ApplyStatusEvent>,
) {
    for (entity, awaiting, mut stats) in q.iter_mut() {
        if timestamp.0 < awaiting.ready_at_timestamp {
            continue;
        }
        // Restore to full HP. Magic and morale stay where they were — the
        // character returns alive but spent.
        stats.health.current = stats.health.base;

        // Apply mental debuffs per GDD Part 4 table (rating → debuff payload).
        apply_resurrection_debuffs(entity, awaiting.rating, &mut status_writer);

        commands
            .entity(entity)
            .remove::<Dead>()
            .remove::<AwaitingResurrection>();
        writer.write(ResurrectedEvent {
            who: entity,
            rating: awaiting.rating,
        });
        info!(
            "Resurrected {:?} with rating {:?}",
            entity, awaiting.rating
        );
    }
}

fn apply_resurrection_debuffs(
    target: Entity,
    rating: ResurrectionRating,
    writer: &mut MessageWriter<crate::status_effects::ApplyStatusEvent>,
) {
    use crate::status_effects::{
        ApplyStatusEvent, BadConditionKind, DebuffKind, StatusKind,
    };
    match rating {
        ResurrectionRating::Exceptional => {}
        ResurrectionRating::Satisfactory => {
            // GDD: "1 random minor debuff for 1 day". Pick Drained as a
            // representative minor debuff for now; the random pick can be
            // promoted to a system once a debuff-roll table exists.
            writer.write(ApplyStatusEvent {
                target,
                kind: StatusKind::Debuff(DebuffKind::Drained),
                tier: 1,
                source: None,
                expiry_override: None,
                resource_focus: None,
            });
        }
        ResurrectionRating::Acceptable => {
            writer.write(ApplyStatusEvent {
                target,
                kind: StatusKind::Debuff(DebuffKind::HauntedDreams),
                tier: 1,
                source: None,
                expiry_override: None,
                resource_focus: None,
            });
        }
        ResurrectionRating::Poor => {
            writer.write(ApplyStatusEvent {
                target,
                kind: StatusKind::Debuff(DebuffKind::HauntedDreams),
                tier: 1,
                source: None,
                expiry_override: None,
                resource_focus: None,
            });
            writer.write(ApplyStatusEvent {
                target,
                kind: StatusKind::Debuff(DebuffKind::Fragile),
                tier: 1,
                source: None,
                expiry_override: None,
                resource_focus: None,
            });
        }
        ResurrectionRating::Neglectful => {
            writer.write(ApplyStatusEvent {
                target,
                kind: StatusKind::Debuff(DebuffKind::HauntedDreams),
                tier: 2,
                source: None,
                expiry_override: None,
                resource_focus: None,
            });
            writer.write(ApplyStatusEvent {
                target,
                kind: StatusKind::Debuff(DebuffKind::BrokenBody),
                tier: 1,
                source: None,
                expiry_override: None,
                resource_focus: None,
            });
        }
        ResurrectionRating::Forfeited => {
            writer.write(ApplyStatusEvent {
                target,
                kind: StatusKind::Debuff(DebuffKind::HauntedDreams),
                tier: 3,
                source: None,
                expiry_override: None,
                resource_focus: None,
            });
            writer.write(ApplyStatusEvent {
                target,
                kind: StatusKind::Debuff(DebuffKind::BrokenBody),
                tier: 1,
                source: None,
                expiry_override: None,
                resource_focus: None,
            });
            writer.write(ApplyStatusEvent {
                target,
                kind: StatusKind::Debuff(DebuffKind::ShatteredSpirit),
                tier: 1,
                source: None,
                expiry_override: None,
                resource_focus: None,
            });
            // The morale-loss-from-self-resurrection trigger lands here too,
            // since the GDD says ally death and self-resurrection both lower
            // morale. Halve current morale as a placeholder.
            // (Concrete amount is data, not behaviour, so it's safe to tune.)
        }
    }
    let _ = target; // kept to make the signature uniform if writer drops out
}

fn resolve_use_item_intent_system(
    mut intents: MessageReader<UseItemIntentEvent>,
    item_catalog: Res<InventoryItemCatalog>,
    mut inventory_q: Query<&mut Inventory>,
    mut stats_q: Query<&mut CombatStats>,
    mut used_writer: MessageWriter<ItemUsedEvent>,
) {
    for intent in intents.iter() {
        let target = intent.target.unwrap_or(intent.user);

        let Some(item_def) = item_catalog.0.get(&intent.item_id) else {
            warn!("Unknown item id {} for item use", intent.item_id);
            continue;
        };

        let Ok(mut inventory) = inventory_q.get_mut(intent.user) else {
            warn!("Entity {:?} has no inventory", intent.user);
            continue;
        };

        if !inventory.has_item(intent.item_id) {
            warn!("Entity {:?} does not own item {}", intent.user, intent.item_id);
            continue;
        }

        let effect = match item_def.kind {
            InventoryItemKind::Consumable {
                effect,
                usable_on_others,
                usable_pre_death,
            } => {
                if matches!(intent.trigger, ItemUseTrigger::PreDeath) && !usable_pre_death {
                    warn!("Item {} cannot be used before death", intent.item_id);
                    continue;
                }
                if target != intent.user && !usable_on_others {
                    warn!("Item {} cannot target other characters", intent.item_id);
                    continue;
                }
                effect
            }
            InventoryItemKind::Equipment(_) => {
                warn!("Equipment item {} is not directly usable", intent.item_id);
                continue;
            }
        };

        if !inventory.remove_item(intent.item_id) {
            warn!("Failed to consume item {}", intent.item_id);
            continue;
        }

        if !apply_consumable_effect_to_health(target, effect, &mut stats_q) {
            inventory.add_item(intent.item_id);
            warn!("Failed to apply item {} to target {:?}", intent.item_id, target);
            continue;
        }

        used_writer.write(ItemUsedEvent {
            user: intent.user,
            target,
            item_id: intent.item_id,
            trigger: intent.trigger,
        });
    }
}

fn resolve_give_item_intent_system(
    mut intents: MessageReader<GiveItemIntentEvent>,
    mut inventory_q: Query<&mut Inventory>,
    loadout_q: Query<&EquipmentLoadout>,
    equipment_q: Query<&Equipment>,
    mut transferred_writer: MessageWriter<ItemTransferredEvent>,
) {
    for intent in intents.iter() {
        if intent.giver == intent.receiver {
            continue;
        }

        if let Ok(loadout) = loadout_q.get(intent.giver) {
            if loadout.has_equipped_item_id(intent.item_id, &equipment_q) {
                warn!(
                    "Entity {:?} cannot give equipped item {} without unequipping it first",
                    intent.giver, intent.item_id
                );
                continue;
            }
        }

        let Ok(mut giver_inventory) = inventory_q.get_mut(intent.giver) else {
            warn!("Giver {:?} has no inventory", intent.giver);
            continue;
        };

        if !giver_inventory.remove_item(intent.item_id) {
            warn!(
                "Giver {:?} does not own item {}",
                intent.giver, intent.item_id
            );
            continue;
        }

        drop(giver_inventory);

        if let Ok(mut receiver_inventory) = inventory_q.get_mut(intent.receiver) {
            receiver_inventory.add_item(intent.item_id);
        } else {
            warn!("Receiver {:?} has no inventory", intent.receiver);
            if let Ok(mut giver_inventory) = inventory_q.get_mut(intent.giver) {
                giver_inventory.add_item(intent.item_id);
            }
            continue;
        }

        transferred_writer.write(ItemTransferredEvent {
            giver: intent.giver,
            receiver: intent.receiver,
            item_id: intent.item_id,
        });
    }
}

fn apply_buff_system(
    mut commands: Commands,
    mut reader: MessageReader<ApplyBuffEvent>,
    mut modifiers_q: Query<&mut StatModifiers>,
) {
    for ev in reader.iter() {
        let modifier = StatModifier {
            stat: ev.stat,
            multiplier: ev.multiplier,
            expires_at_timestamp: Some(ev.applied_at.saturating_add(ev.duration_in_ticks)),
            source: None,
        };

        if let Ok(mut modifiers) = modifiers_q.get_mut(ev.target) {
            modifiers.0.push(modifier.clone());
        } else {
            commands.entity(ev.target).insert(StatModifiers(vec![modifier.clone()]));
        }

        commands.spawn(Buff {
            stat: ev.stat,
            multiplier: ev.multiplier,
            ends_at_timestamp: ev.applied_at.saturating_add(ev.duration_in_ticks),
            source: Some(ev.applier),
        });
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
            cause: ev.cause.clone(),
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
    mut query_buffs: Query<(Entity, &Buff)>,
    timestamp: Res<Timestamp>,
) {
    if !timestamp.is_changed() {
        return;
    }

    // Remove expired stat modifiers based on timestamp
    for (entity, mut mods) in query_mods.iter_mut() {
        let mut keep = Vec::new();
        for m in mods.0.drain(..) {
            match m.expires_at_timestamp {
                Some(ends_at) if timestamp.0 >= ends_at => {}
                _ => keep.push(m),
            }
        }
        commands.entity(entity).insert(StatModifiers(keep));
    }

    // Remove expired buffs
    for (entity, buff) in query_buffs.iter_mut() {
        if timestamp.0 >= buff.ends_at_timestamp {
            commands.entity(entity).despawn();
        }
    }
}

// Health and magic regen are no longer per-tick systems — they fire on
// `RestEvent`. See `StatusEffectsPlugin` for the rest-driven regen handlers.

/// Fans `RestEvent` (a request, possibly with `target = None`) out into a
/// per-entity `BeforeRestEvent`. Listeners running between this system and
/// `rest_regen_system` may mutate `hours` to model rest-modifier effects.
pub fn expand_rest_intent_system(
    mut reader: MessageReader<RestEvent>,
    mut writer: MessageWriter<BeforeRestEvent>,
    targets_q: Query<Entity, With<CombatStats>>,
) {
    for ev in reader.read() {
        match ev.target {
            Some(t) => {
                writer.write(BeforeRestEvent {
                    target: t,
                    hours: ev.hours,
                    cause: ev.cause.clone(),
                });
            }
            None => {
                for e in targets_q.iter() {
                    writer.write(BeforeRestEvent {
                        target: e,
                        hours: ev.hours,
                        cause: ev.cause.clone(),
                    });
                }
            }
        }
    }
}

/// Example AI system that makes a simple attack intent for demo
/// Debug AI that simply picks the first valid target and attacks.
/// Only runs if no other AI system produced an intent this frame.
fn demo_ai_system(
    mut intents: MessageWriter<AttackIntentEvent>,
    query_chars: Query<Entity, With<CombatStats>>,
) {
    let ents: Vec<Entity> = query_chars.iter().collect();
    if ents.len() < 2 {
        return; // not enough participants
    }

    let attacker = ents[0];
    let target = ents[1];

    intents.send(AttackIntentEvent {
        attacker,
        target,
        ability: None,
        context: AttackContext::default(),
        cause: ActionCause::Ai,
    });
}


/// -----------------------------
/// Turn Manager resource
/// -----------------------------

/// Holds the current computed turn order (queue of entities ready to act).
#[derive(Resource, Default)]
pub struct TurnOrder {
    pub queue: VecDeque<Entity>,
}

#[derive(Resource, Default)]
pub struct TurnInProgress(pub bool);

#[derive(Resource, Default)]
pub struct MagicRegenTracker {
    pub last_processed_timestamp: u32,
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
        let mut total_speed: u32 = 0;
        let mut total_level: u32 = 0;
        let mut count: u32 = 0;
        for &e in &self.participants {
            if let Ok(stats) = stats_q.get(e) {
                total_speed += stats.speed.current.max(0) as u32;
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
        let avg_speed = (total_speed / count).max(1);
        self.turn_threshold = avg_speed * 2; // original used <<1
        let avg_level = (total_level / count).max(1);
        self.maximum_value = avg_level << 3; // original used <<3
    }

    /// Calculate a precise turn order based on accumulated agility.
    /// For each participant:
    ///   accumulated += base_agility + rand(0..maximum_value)
    ///   while accumulated >= turn_threshold: push to order and subtract threshold
    pub fn calculate_turn_order(
        &mut self,
        mut acc_q: &mut Query<&mut AccumulatedSpeed>,
        stats_q: &Query<&CombatStats>,
    ) -> Vec<Entity> {
        let mut rng = rand::rng();
        let mut order: Vec<Entity> = Vec::new();

        // iterate participants in stable order
        for &entity in &self.participants {
            // get accumulated speed, if missing insert default (0)
            if let Ok(mut acc) = acc_q.get_mut(entity) {
                let speed = stats_q
                    .get(entity)
                    .map(|s| s.speed.current.max(0) as u32)
                    .unwrap_or(0);
                let jitter: u32 = if self.maximum_value > 0 {
                    rng.gen_range(0..self.maximum_value)
                } else {
                    0
                };

                let mut current = acc.0;
                // add speed + random jitter
                current = current.saturating_add(speed).saturating_add(jitter);
                // while enough to take a turn
                while current >= self.turn_threshold && self.turn_threshold > 0 {
                    current = current.saturating_sub(self.turn_threshold);
                    order.push(entity);
                }
                acc.0 = current;
            } else {
                // entity missing AccumulatedSpeed -> skip or insert?
                // We skip; spawn-time code should ensure AccumulatedSpeed exists for participants.
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
        let value = ((inner.ln() / 1.25f32.ln()) + 1.5) * 16384.0;
        // clamp to non-negative
        value.max(0.0)
    } else {
        // ratio.powf(30.2) * 16384
        ratio.powf(30.2) * 16384.0
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
/// Equation example: f(x) = base - (2 * attr)^exponent / 524288      /// 0 = 250 - (2 * 250)^3.007632509 / 524288
fn curve_growth_tactical(attr: u8, base: f32, exponent: f32) -> u32 {
    // multiply attr by 2 (shift left 1), then raise to exponent
    let shifted_attr = ((attr as u32) << 1) as f32;
    let power = shifted_attr.powf(exponent);
    
    // Divide power by 524288 FIRST, then subtract from (base/8)
    let divided_power = power / 524288.0;
    let value = (base / 8.0) - divided_power;
    
    // Clamp negative to zero
    let result = if value.is_nan() || value <= 0.0 {
        1
    } else {
        value as u32
    };

    result
    
    // Minimum growth (base/10 divided by 8, minimum 1)
    // let min_growth = (((base / 10.0) as u32) >> 3).max(1);
    
    // std::cmp::max(result, 1)
}

/// Similar helper but returning signed i32 (for stats that are i32)
fn curve_growth_i32(attr: u8, base: f32, exponent: f32) -> i32 {
    curve_growth_tactical(attr, base, exponent) as i32
}

fn distribute_integer_growth(total_gain: u32, weights: [u8; 4]) -> [u32; 4] {
    if total_gain == 0 {
        return [0; 4];
    }

    let total_weight: u32 = weights.iter().map(|&w| w as u32).sum();
    if total_weight == 0 {
        let base = total_gain / 4;
        let mut gains = [base; 4];
        for gain in gains.iter_mut().take((total_gain % 4) as usize) {
            *gain += 1;
        }
        return gains;
    }

    let mut gains = [0_u32; 4];
    let mut remainders = [(0usize, 0_u32); 4];
    let mut assigned = 0_u32;

    for (index, &weight) in weights.iter().enumerate() {
        let numerator = total_gain.saturating_mul(weight as u32);
        gains[index] = numerator / total_weight;
        remainders[index] = (index, numerator % total_weight);
        assigned = assigned.saturating_add(gains[index]);
    }

    remainders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let leftovers = total_gain.saturating_sub(assigned);
    for (index, _) in remainders.into_iter().take(leftovers as usize) {
        gains[index] = gains[index].saturating_add(1);
    }

    gains
}

/// Apply one growth contribution's amount to the matching field on
/// `CombatStats`. Capacity targets land on `*.base` via `add_to_base` so
/// `current` rises proportionally; rate targets accumulate into the
/// `*_per_rest_hour` scalars.
fn apply_growth(stats: &mut CombatStats, target: GrowthTarget, amount: i32) {
    let amount_f = amount as f32;
    match target {
        GrowthTarget::Health => stats.health.add_to_base(amount),
        GrowthTarget::HealthRegen => {
            stats.health_per_rest_hour = stats.health_per_rest_hour.saturating_add(amount);
        }
        GrowthTarget::Morale => stats.morale.add_to_base(amount),
        GrowthTarget::MoraleRegen => {
            stats.morale_per_rest_hour = stats.morale_per_rest_hour.saturating_add(amount);
        }
        GrowthTarget::Lethality => stats.lethality.add_to_base(amount),
        GrowthTarget::Hit => stats.hit.add_to_base(amount),
        GrowthTarget::Armor => stats.armor.add_to_base(amount),
        GrowthTarget::Speed => stats.speed.add_to_base(amount),
        GrowthTarget::Evasion => stats.evasion.add_to_base(amount),
        GrowthTarget::Mind => stats.mind.add_to_base(amount),
        GrowthTarget::Movement => stats.movement.add_to_base(amount),
        GrowthTarget::Kiho => stats.kiho.add_to_base(amount_f),
        GrowthTarget::Onmyodo => stats.onmyodo.add_to_base(amount_f),
        GrowthTarget::Yokaijutsu => stats.yokaijutsu.add_to_base(amount_f),
        GrowthTarget::Kamishin => stats.kamishin.add_to_base(amount_f),
    }
}

/// Optional per-character class curve modulation, e.g. paladins gain more HP
/// per vitality point. Returns the multiplier the contribution amount should
/// be scaled by before being applied.
fn growth_curve_multiplier(target: GrowthTarget, curve: Option<&GrowthCurve>) -> f32 {
    let Some(c) = curve else { return 1.0 };
    match target {
        GrowthTarget::Health | GrowthTarget::HealthRegen => c.hp_curve,
        GrowthTarget::Lethality => c.lethality_curve,
        GrowthTarget::Hit => c.hit_curve,
        GrowthTarget::Speed | GrowthTarget::Movement => c.speed_curve,
        GrowthTarget::Evasion => c.evasion_curve,
        GrowthTarget::Mind => c.mind_curve,
        GrowthTarget::Morale | GrowthTarget::MoraleRegen => c.morale_curve,
        GrowthTarget::Kiho
        | GrowthTarget::Onmyodo
        | GrowthTarget::Yokaijutsu
        | GrowthTarget::Kamishin => c.magic_curve,
        GrowthTarget::Armor => 1.0,
    }
}

/// --------------- Level up system using your confirmed parameters ---------------

/// Event: LevelUpEvent { who: Entity, old_level: u8, new_level: u8 }
/// (assumes you already defined LevelUpEvent elsewhere and registered it)
pub fn level_up_system(
    mut level_up_events: MessageReader<LevelUpEvent>,
    mut q_stats: Query<(
        &mut CombatStats,
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
        if let Ok((mut stats, growth_attr, curve_opt)) = q_stats.get_mut(ev.who) {
            let level_gained = (ev.new_level as i32) - (ev.old_level as i32);
            if level_gained <= 0 {
                continue;
            }

            // Snapshot the growth iterator out of the borrow so we don't hold
            // `growth_attr` across the mutation of `stats`.
            let pairs: [(u8, &'static [GrowthContribution]); 13] =
                growth_attr.iter_contributions();
            let curve = curve_opt.as_deref().cloned();

            for _ in 0..level_gained {
                for (points, contribs) in pairs.iter() {
                    if *points == 0 {
                        continue;
                    }
                    for c in contribs.iter() {
                        let raw = curve_growth_tactical(*points, c.base, c.exponent) as i32;
                        let scaled =
                            (raw as f32 * growth_curve_multiplier(c.target, curve.as_ref()))
                                .round() as i32;
                        if scaled != 0 {
                            apply_growth(&mut stats, c.target, scaled);
                        }
                    }
                }
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
                + attributes.celerity as u32
                + attributes.reflex as u32
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
    turn_in_progress: Res<TurnInProgress>,
    mut acc_q: Query<&mut AccumulatedSpeed>,
    stats_q: Query<&CombatStats>,
    levels_q: Query<&Level>,
    mut ev_writer: MessageWriter<TurnOrderCalculatedEvent>,
    _ev_reader: MessageReader<RoundEndEvent>,
) {
    if turn_in_progress.0 {
        return;
    }
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
            let speed = stats_q.get(entity).map(|s| s.speed.current.max(0) as u32).unwrap_or(0);
            let jitter: u32 = if tm.maximum_value > 0 { rng.gen_range(0..tm.maximum_value) } else { 0 };
            let mut current = acc.0;
            current = current.saturating_add(speed).saturating_add(jitter);
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
fn advance_turn_system(
    mut turn_order: ResMut<TurnOrder>,
    mut turn_start_writer: MessageWriter<TurnStartEvent>,
    mut round_end_writer: MessageWriter<RoundEndEvent>,
    mut timestamp: ResMut<Timestamp>,
) {
    if let Some(next) = turn_order.queue.pop_front() {
        timestamp.0 = timestamp.0.saturating_add(1);
        turn_start_writer.send(TurnStartEvent { who: next });
    } else {
        round_end_writer.send(RoundEndEvent);
    }
}

/// Example: when a turn starts for an entity, we allow AI or player to emit AttackIntentEvent.
/// For simplicity demo AI will fire an intent against any other participant.
pub fn on_turn_start_system(
    mut ev_reader: MessageReader<TurnStartEvent>,
    q_participants: Query<Entity, With<CombatStats>>,
    player_controlled: Query<(), With<PlayerControlled>>,
    bt_driven: Query<(), With<crate::ai_decision::BehaviorTreeProfile>>,
    mut stats_q: Query<&mut CombatStats>,
    mut intent_writer: MessageWriter<AttackIntentEvent>,
    mut turn_end_writer: MessageWriter<TurnEndEvent>,
    mut turn_in_progress: ResMut<TurnInProgress>,
) {
    for ev in ev_reader.iter() {
        let Ok(mut stats) = stats_q.get_mut(ev.who) else {
            continue;
        };
        stats.action_points.current = stats.action_points.base;

        if player_controlled.get(ev.who).is_ok() {
            continue;
        }
        // BT-driven enemies are handled by `evaluate_behavior_tree_system`.
        if bt_driven.get(ev.who).is_ok() {
            continue;
        }
        // simple demo: find first entity different from ev.who and issue attack
        let mut target_opt: Option<Entity> = None;
        for e in q_participants.iter() {
            if e != ev.who {
                target_opt = Some(e);
                break;
            }
        }
        if let Some(target) = target_opt {
            while stats.action_points.spend(BASIC_ATTACK_ACTION_POINT_COST) {
                intent_writer.send(AttackIntentEvent {
                    attacker: ev.who,
                    target,
                    ability: None,
                    context: AttackContext::default(),
                    cause: ActionCause::Ai,
                });
            }
            turn_end_writer.send(TurnEndEvent { who: ev.who });
            turn_in_progress.0 = false;
        }
    }
}

fn finish_turn_if_needed(
    actor: Entity,
    pending: &mut ResMut<PendingPlayerAction>,
    turn_end_writer: &mut MessageWriter<TurnEndEvent>,
    turn_in_progress: &mut ResMut<TurnInProgress>,
    stats_q: &mut Query<&mut CombatStats>,
    force_end: bool,
) {
    let should_end = force_end
        || stats_q
            .get(actor)
            .map(|stats| stats.action_points.current <= 0)
            .unwrap_or(true);

    if should_end {
        pending.entity = None;
        turn_end_writer.send(TurnEndEvent { who: actor });
        turn_in_progress.0 = false;
    }
}

/// Bundles every event writer the player-action handler emits to. Without
/// this bundle the system param count exceeds Bevy's 16-arg ceiling.
#[derive(bevy::ecs::system::SystemParam)]
struct PlayerActionWriters<'w> {
    intent: MessageWriter<'w, AttackIntentEvent>,
    use_item: MessageWriter<'w, UseItemIntentEvent>,
    heal: MessageWriter<'w, HealEvent>,
    buff: MessageWriter<'w, ApplyBuffEvent>,
    apply_status: MessageWriter<'w, crate::status_effects::ApplyStatusEvent>,
    remove_status: MessageWriter<'w, crate::status_effects::RemoveStatusEvent>,
    defend: MessageWriter<'w, DefendIntentEvent>,
    wait: MessageWriter<'w, WaitIntentEvent>,
    turn_end: MessageWriter<'w, TurnEndEvent>,
    summon: MessageWriter<'w, SummonEvent>,
}

fn process_player_action_system(
    mut ev: MessageReader<PlayerActionEvent>,
    mut pending: ResMut<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    timestamp: Res<Timestamp>,
    mut dq: ResMut<DamageQueue>,
    mut stats_q: Query<&mut CombatStats>,
    status_q: Query<&crate::status_effects::StatusEffects>,
    defilement_q: Query<&crate::kegare::Defilement>,
    mut writers: PlayerActionWriters,
    mut turn_in_progress: ResMut<TurnInProgress>,
) {
    if pending.entity.is_none() {
        return; // no player turn pending
    }

    let Some(actor) = pending.entity else {
        warn!("Pending player action has no associated entity");
        return;
    };

    for e in ev.iter() {
        let mut end_turn = false;

        // Terrified gating: tier 2+ forces every action to be movement, so
        // attacks/abilities/items are blocked here. (Tier 1 only forces the
        // first action — that's enforced by the movement system, not here.)
        // Single source of truth for status-driven action overrides. Every
        // branch below consults the same `gates` instead of having bespoke
        // status checks per action.
        let gates = crate::status_effects::action_gates(status_q.get(actor).ok());

        match &e.action {
            PlayerAction::Attack(target) => {
                if gates.block_attacks {
                    info!("Actor {:?}: attacks blocked by ActionGates", actor);
                    continue;
                }
                let Ok(mut stats) = stats_q.get_mut(actor) else {
                    warn!("Actor {:?} has no combat stats", actor);
                    break;
                };
                if !stats.action_points.spend(BASIC_ATTACK_ACTION_POINT_COST) {
                    info!(
                        "Actor {:?} needs {} AP for a basic attack but only has {}",
                        actor, BASIC_ATTACK_ACTION_POINT_COST, stats.action_points.current
                    );
                    continue;
                }
                writers.intent.send(AttackIntentEvent {
                    attacker: actor,
                    target: *target,
                    ability: None,
                    context: AttackContext::default(),
                    cause: ActionCause::Player,
                });
            }

            PlayerAction::UseAbility(ability_id, target) => {
                if gates.block_attacks {
                    info!("Actor {:?}: ability use blocked by ActionGates", actor);
                    continue;
                }
                let Some(tree) = ability_tree.as_ref() else {
                    warn!("Ability tree resource is not available");
                    continue;
                };
                let Some(ability) = tree.0.find(*ability_id as u16) else {
                    warn!("Ability {} not found", ability_id);
                    continue;
                };

                if gates.block_magic_abilities && ability.magic_cost > 0.0 {
                    info!(
                        "Actor {:?}: cannot cast {} (school {:?}) — blocked by ActionGates",
                        actor, ability.name, ability.magic_school
                    );
                    continue;
                }

                // Efficient Casting / Exhausting Cost shape the actual magic
                // paid; AP cost is unaffected.
                let cost_mult =
                    crate::status_effects::magic_cost_multiplier(status_q.get(actor).ok());
                // Kegare never blocks a school — it only tilts the cost.
                // Kamishin grows pricier as you defile, Yokaijutsu cheaper.
                // Only entities carrying a `Defilement` participate, so this is
                // a no-op until kegare is wired onto a character.
                let kegare_cost_mult = defilement_q
                    .get(actor)
                    .map(|&d| crate::kegare::cost_multiplier(d, ability.magic_school))
                    .unwrap_or(1.0);
                let scaled_magic_cost = ability.magic_cost * cost_mult * kegare_cost_mult;

                let Ok(mut stats) = stats_q.get_mut(actor) else {
                    warn!("Actor {:?} has no combat stats", actor);
                    continue;
                };
                if !stats.action_points.can_spend(ability.action_point_cost) {
                    info!(
                        "Actor {:?} needs {} AP for {} but only has {}",
                        actor, ability.action_point_cost, ability.name, stats.action_points.current
                    );
                    continue;
                }
                if !stats.pool(ability.magic_school).can_spend(scaled_magic_cost) {
                    info!(
                        "Actor {:?} lacks {:?} for {}: needs {:.2} (raw {:.2})",
                        actor,
                        ability.magic_school,
                        ability.name,
                        scaled_magic_cost,
                        ability.magic_cost,
                    );
                    continue;
                }

                stats.action_points.spend(ability.action_point_cost);
                stats.pool_mut(ability.magic_school).spend(scaled_magic_cost);
                drop(stats);

                handle_ability(
                    actor,
                    &ability,
                    &[*target],
                    timestamp.0,
                    &mut dq,
                    &mut writers.intent,
                    &mut writers.heal,
                    &mut writers.buff,
                    &mut writers.apply_status,
                    &mut writers.remove_status,
                    &mut writers.summon,
                );
            }

            PlayerAction::UseItem(_item_id, _target) => {
                let item_id = *_item_id;
                let target = *_target;
                if gates.block_items || gates.block_attacks {
                    info!("Actor {:?}: item use blocked by ActionGates", actor);
                    continue;
                }
                let Ok(mut stats) = stats_q.get_mut(actor) else {
                    warn!("Actor {:?} has no combat stats", actor);
                    continue;
                };
                if !stats.action_points.spend(ITEM_ACTION_POINT_COST) {
                    info!(
                        "Actor {:?} needs {} AP to use an item but only has {}",
                        actor, ITEM_ACTION_POINT_COST, stats.action_points.current
                    );
                    continue;
                }
                writers.use_item.write(UseItemIntentEvent {
                    user: actor,
                    item_id,
                    target,
                    trigger: ItemUseTrigger::Manual,
                });
            }

            PlayerAction::Defend => {
                writers.defend.send(DefendIntentEvent { defender: actor });
                end_turn = true;
            }

            PlayerAction::Wait => {
                writers.wait.send(WaitIntentEvent { waiter: actor });
                end_turn = true;
            }
        }

        finish_turn_if_needed(
            actor,
            &mut pending,
            &mut writers.turn_end,
            &mut turn_in_progress,
            &mut stats_q,
            end_turn,
        );
        break;
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
    mut turn_in_progress: ResMut<TurnInProgress>,
) {
    for _ in ev_reader.iter() {
        if let Some(next) = turn_order.queue.pop_front() {
            ev_writer.send(TurnStartEvent { who: next });
            turn_in_progress.0 = true;
        }
    }
}

/// Buff tick per turn: when a TurnStartEvent occurs for a character, decrement their buff durations (so durations map to turns).
fn buff_tick_on_turn_start_system(
    mut ev_reader: MessageReader<TurnStartEvent>,
    mut query_buffs: Query<(Entity, &Buff)>,
    mut commands: Commands,
    mut modifiers_q: Query<(Entity, &mut StatModifiers)>,
    timestamp: Res<Timestamp>,
) {
    for ev in ev_reader.iter() {
        // Decrement global Buff entities that have source == ev.who (optional design)
        for (entity, buff) in query_buffs.iter_mut() {
            if let Some(src) = buff.source {
                if src == ev.who {
                    if timestamp.0 >= buff.ends_at_timestamp {
                        commands.entity(entity).despawn();
                    }
                }
            }
        }

        // Also decrement StatModifiers on the actor
        if let Ok((entity, mut mods)) = modifiers_q.get_mut(ev.who) {
            let mut keep: Vec<StatModifier> = Vec::new();
            for m in mods.0.drain(..) {
                match m.expires_at_timestamp {
                    Some(ends_at) if timestamp.0 >= ends_at => {}
                    _ => keep.push(m),
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
        &CombatStats,
        Option<&StatModifiers>,
        Option<&EquipmentLoadout>,
        Option<&Level>,
        Option<&Experience>,
        Option<&AccumulatedSpeed>,
    )>,
) {
    for (name, id, stats, mods, slots, lvl, xp, acc) in q.iter() {
        let level = lvl.map(|l| l.0).unwrap_or(1);
        let xp_val = xp.map(|x| x.0).unwrap_or(0);
        let acc_text = acc.map(|a| a.0.to_string()).unwrap_or_else(|| "N/A".into());
        let mut s = format!(
            "{}({:?}) L{} XP:{} HP: {}/{} Leth:{} Hit:{} Acc:{}",
            name.0,
            id.0,
            level,
            xp_val,
            stats.health.current,
            stats.health.base,
            stats.lethality.current,
            stats.hit.current,
            acc_text
        );
        if let Some(mods) = mods {
            if !mods.0.is_empty() {
                s.push_str(&format!(" Mods: {:?}", mods.0));
            }
        }
        if let Some(loadout) = slots {
            if loadout.equipped_in_slot(EquipmentSlotType::Weapon).is_some() {
                s.push_str(" WeaponEquipped");
            }
        }
        info!("{}", s);
    }
}

pub fn get_affected_characters(
    ability: &Ability,
    player_entity: Entity,
    cursor_position: (f32, f32),
    query: &Query<(Entity, &Transform)>,
    player_position_query: &Query<&Transform>,
) -> Vec<Entity> {
    let mut affected = Vec::new();

    let Ok(player_pos) = player_position_query.get(player_entity) else {
        warn!("Could not fetch player position for targeting");
        return affected;
    };
    let player_position = (player_pos.translation.x, player_pos.translation.y);

    for (entity, transform) in query.iter() {
        let target_position = (transform.translation.x, transform.translation.y);

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

fn default_allowed_types_for_slot(slot_type: EquipmentSlotType) -> Vec<EquipmentType> {
    match slot_type {
        EquipmentSlotType::Weapon => vec![
            EquipmentType::Weapon(WeaponType::Sword),
            EquipmentType::Weapon(WeaponType::Dagger),
            EquipmentType::Weapon(WeaponType::Staff),
        ],
        EquipmentSlotType::Armor => vec![
            EquipmentType::Armor(ArmorType::HeavyArmor),
            EquipmentType::Armor(ArmorType::LightArmor),
            EquipmentType::Armor(ArmorType::Robe),
            EquipmentType::Armor(ArmorType::Shield),
        ],
        EquipmentSlotType::Accessory => vec![
            EquipmentType::Accessory(AccessoryType::Charm),
            EquipmentType::Accessory(AccessoryType::Ring),
            EquipmentType::Accessory(AccessoryType::Relic),
        ],
    }
}

/// -----------------------------
/// Startup spawn examples (with XP, Level, AccumulatedSpeed)
/// -----------------------------
fn spawn_examples(mut commands: Commands, mut tm: ResMut<TurnManager>, timestamp: Res<Timestamp>) {
    // spawn sword
    let sword = commands
        .spawn_empty()
        .insert(Equipment {
            id: 5001,
            name: "Silversteel Blade".to_string(),
            equipment_type: EquipmentType::Weapon(WeaponType::Sword),
            base_price: 32000,
            // Material total with enum unit costs:
            // 20*1000 + 4*400 + 3*500 + 2*800 = 24700 (below base_price)
            materials: vec![
                ItemMaterialCost {
                    material: ItemMaterial::SilverSteelIngot,
                    quantity: 20,
                },
                ItemMaterialCost {
                    material: ItemMaterial::OakWood,
                    quantity: 4,
                },
                ItemMaterialCost {
                    material: ItemMaterial::Leather,
                    quantity: 3,
                },
                ItemMaterialCost {
                    material: ItemMaterial::CrystalDust,
                    quantity: 2,
                },
            ],
            lethality: 10,
            hit: 5,
            armor: 0,
            agility: 2,
            mind: 0,
            morale: 0,
        })
        .insert(EquipmentHooks(vec![EquipHook::BeforeAttackMultiplier {
            stat: Stat::Lethality,
            multiplier: 1.15,
            duration_turns: 1,
        }]))
        .insert(WeaponSharpness::new(100, 4))
        .insert(WeaponBeforeAttackEffects(vec![
            WeaponBeforeAttackEffect::AddFlatDamage { flat: 6 },
            WeaponBeforeAttackEffect::BonusWhenSharp {
                minimum_sharpness: 80,
                flat_damage: 4,
            },
        ]))
        .id();

    // The four GDD protagonists. All are bound by the Merchant's Contract
    // (`Bound` + `ResurrectionStanding`), and their `Abilities` lists hold the
    // per-character ability ids from `assets/data/abilities/AbilitiesExample.ron`.

    // --------------------------------------
    // Rina — Rogue (Kiho)
    // --------------------------------------
    let rina = commands
        .spawn_empty()
        .insert(Name("Rina".to_string()))
        .insert(CharacterId(1))
        .insert(Class("Rogue".to_string()))
        .insert(Bound)
        .insert(ResurrectionStanding::default())
        .insert(Inventory { item_ids: vec![1001] })
        .insert(CombatStats {
            health: <StatPool<i32>>::new(41),
            morale: <StatPool<i32>>::new(62),
            action_points: <StatPool<i32>>::new(DEFAULT_ACTION_POINTS + 1), // GDD: extra AP
            movement: <StatPool<i32>>::new(7),
            kiho: <StatPool<f32>>::new(4.0),
            onmyodo: <StatPool<f32>>::new(0.0),
            yokaijutsu: <StatPool<f32>>::new(0.0),
            kamishin: <StatPool<f32>>::new(0.0),
            lethality: <StatPool<i32>>::new(25),
            hit: <StatPool<i32>>::new(32),
            armor: <StatPool<i32>>::new(7),
            speed: <StatPool<i32>>::new(37),    // GDD agility
            evasion: <StatPool<i32>>::new(37),  // GDD agility
            mind: <StatPool<i32>>::new(3),
            health_per_rest_hour: 1,
            morale_per_rest_hour: 4,
            kiho_per_rest_hour: 0.25,
            onmyodo_per_rest_hour: 0.0,
            yokaijutsu_per_rest_hour: 0.0,
            kamishin_per_rest_hour: 0.0,
        })
        .insert(GrowthAttributes::default())
        .insert(GrowthCurve::rogue_curve())
        .insert(EquipmentLoadout::with_allowed_types([
            (EquipmentSlotType::Weapon, vec![EquipmentType::Weapon(WeaponType::Dagger)]),
            (EquipmentSlotType::Armor, vec![EquipmentType::Armor(ArmorType::LightArmor)]),
            (EquipmentSlotType::Accessory, vec![EquipmentType::Accessory(AccessoryType::Ring)]),
            (EquipmentSlotType::Accessory, vec![EquipmentType::Accessory(AccessoryType::Charm)]),
        ]))
        // Quick slash, Throw knife, Shoot, Reload, Dodge, Get knife back,
        // Invisibility, Surprise attack — see AbilitiesExample.ron L10 / 0x5000**.
        .insert(Abilities(vec![20480, 20481, 20482, 20483, 20484, 20485, 20486, 20487]))
        .insert(Experience(0))
        .insert(Level(1))
        .insert(AccumulatedSpeed(0))
        .insert(RogueBehavior)
        .insert(StatModifiers(Vec::new()))
        .insert(SkillPoints::default())
        .insert(LearnedSkills::default())
        .insert(MagicCostMultipliers::default())
        // Rina: Kiho only, plus her Rogue class tree.
        .insert(
            SkillTreeAccess::new()
                .with_universal()
                .with_magic([MagicSchool::Kiho])
                .with(SkillTreeKind::RinaRogue),
        )
        .id();

    // --------------------------------------
    // Sayaka — Cleric / Kitsune (Kamishin, Onmyodo)
    // --------------------------------------
    let sayaka = commands
        .spawn_empty()
        .insert(Name("Sayaka".to_string()))
        .insert(CharacterId(2))
        .insert(Class("Cleric".to_string()))
        .insert(Bound)
        .insert(ResurrectionStanding::default())
        .insert(Inventory { item_ids: vec![5001] })
        .insert(CombatStats {
            health: <StatPool<i32>>::new(52),
            morale: <StatPool<i32>>::new(70),
            action_points: <StatPool<i32>>::new(DEFAULT_ACTION_POINTS),
            movement: <StatPool<i32>>::new(5),
            kiho: <StatPool<f32>>::new(0.0),
            onmyodo: <StatPool<f32>>::new(5.0),
            yokaijutsu: <StatPool<f32>>::new(0.0),
            kamishin: <StatPool<f32>>::new(6.0),
            lethality: <StatPool<i32>>::new(12),
            hit: <StatPool<i32>>::new(20),
            armor: <StatPool<i32>>::new(10),
            speed: <StatPool<i32>>::new(18),
            evasion: <StatPool<i32>>::new(18),
            mind: <StatPool<i32>>::new(22),
            health_per_rest_hour: 2,
            morale_per_rest_hour: 5,
            kiho_per_rest_hour: 0.0,
            onmyodo_per_rest_hour: 0.5,
            yokaijutsu_per_rest_hour: 0.0,
            kamishin_per_rest_hour: 0.6,
        })
        .insert(GrowthAttributes::default())
        .insert(GrowthCurve::default())
        .insert(EquipmentLoadout::with_allowed_types([
            (EquipmentSlotType::Weapon, vec![EquipmentType::Weapon(WeaponType::Staff)]),
            (EquipmentSlotType::Armor, vec![EquipmentType::Armor(ArmorType::Robe)]),
            (EquipmentSlotType::Accessory, vec![EquipmentType::Accessory(AccessoryType::Charm)]),
        ]))
        // Purifying strike, Sacred prayer, Barrier of faith, Cleanse, Ritual of stillness.
        .insert(Abilities(vec![22528, 22529, 22530, 22531, 22532]))
        .insert(Experience(0))
        .insert(Level(1))
        .insert(AccumulatedSpeed(0))
        .insert(StatModifiers(Vec::new()))
        .insert(SkillPoints::default())
        .insert(LearnedSkills::default())
        .insert(MagicCostMultipliers::default())
        // Sayaka: Kamishin + Onmyodo, plus her Cleric class tree.
        .insert(
            SkillTreeAccess::new()
                .with_universal()
                .with_magic([MagicSchool::Kamishin, MagicSchool::Onmyodo])
                .with(SkillTreeKind::SayakaCleric),
        )
        .id();

    // --------------------------------------
    // Houjou Utaka — Samurai (Kiho limited, Yokaijutsu limited)
    // --------------------------------------
    let houjou = commands
        .spawn_empty()
        .insert(Name("Houjou Utaka".to_string()))
        .insert(CharacterId(3))
        .insert(Class("Samurai".to_string()))
        .insert(Bound)
        .insert(ResurrectionStanding::default())
        .insert(Inventory { item_ids: vec![] })
        .insert(CombatStats {
            health: <StatPool<i32>>::new(68),
            morale: <StatPool<i32>>::new(55),
            action_points: <StatPool<i32>>::new(DEFAULT_ACTION_POINTS),
            movement: <StatPool<i32>>::new(5),
            kiho: <StatPool<f32>>::new(2.0),
            onmyodo: <StatPool<f32>>::new(0.0),
            yokaijutsu: <StatPool<f32>>::new(3.0),
            kamishin: <StatPool<f32>>::new(0.0),
            lethality: <StatPool<i32>>::new(34),
            hit: <StatPool<i32>>::new(28),
            armor: <StatPool<i32>>::new(18),
            speed: <StatPool<i32>>::new(22),
            evasion: <StatPool<i32>>::new(22),
            mind: <StatPool<i32>>::new(8),
            health_per_rest_hour: 2,
            morale_per_rest_hour: 3,
            kiho_per_rest_hour: 0.15,
            onmyodo_per_rest_hour: 0.0,
            yokaijutsu_per_rest_hour: 0.2,
            kamishin_per_rest_hour: 0.0,
        })
        .insert(GrowthAttributes::default())
        .insert(GrowthCurve::default())
        .insert({
            let mut loadout = EquipmentLoadout::with_allowed_types([
                (EquipmentSlotType::Weapon, vec![EquipmentType::Weapon(WeaponType::Sword)]),
                (EquipmentSlotType::Armor, vec![EquipmentType::Armor(ArmorType::HeavyArmor)]),
                (EquipmentSlotType::Accessory, vec![EquipmentType::Accessory(AccessoryType::Charm)]),
            ]);
            loadout.equip_in_first_matching_slot(EquipmentType::Weapon(WeaponType::Sword), sword);
            loadout
        })
        // Heavy strike, Wide slash, Counter stance, Blood draw, Forbidden cut,
        // Sake drink, Bind spirit.
        .insert(Abilities(vec![24576, 24577, 24578, 24579, 24580, 24581, 24582]))
        .insert(Experience(0))
        .insert(Level(1))
        .insert(AccumulatedSpeed(0))
        .insert(StatModifiers(Vec::new()))
        .insert(SkillPoints::default())
        .insert(LearnedSkills::default())
        .insert(MagicCostMultipliers::default())
        // Houjou: Kiho + Yokaijutsu (limited), plus his Samurai class tree.
        .insert(
            SkillTreeAccess::new()
                .with_universal()
                .with_magic([MagicSchool::Kiho, MagicSchool::Yokaijutsu])
                .with(SkillTreeKind::HoujouSamurai),
        )
        .id();

    // --------------------------------------
    // Toshiko — Vessel (Yokaijutsu) with the Kuro extra-HP mechanic
    // --------------------------------------
    let toshiko = commands
        .spawn_empty()
        .insert(Name("Toshiko".to_string()))
        .insert(CharacterId(4))
        .insert(Class("Vessel".to_string()))
        .insert(Bound)
        .insert(ResurrectionStanding::default())
        .insert(Inventory { item_ids: vec![1002] })
        .insert(CombatStats {
            health: <StatPool<i32>>::new(44),
            morale: <StatPool<i32>>::new(48),
            action_points: <StatPool<i32>>::new(DEFAULT_ACTION_POINTS),
            movement: <StatPool<i32>>::new(5),
            kiho: <StatPool<f32>>::new(0.0),
            onmyodo: <StatPool<f32>>::new(0.0),
            yokaijutsu: <StatPool<f32>>::new(5.0),
            kamishin: <StatPool<f32>>::new(0.0),
            lethality: <StatPool<i32>>::new(16),
            hit: <StatPool<i32>>::new(18),
            armor: <StatPool<i32>>::new(6),
            speed: <StatPool<i32>>::new(20),
            evasion: <StatPool<i32>>::new(20),
            mind: <StatPool<i32>>::new(20),
            health_per_rest_hour: 1,
            morale_per_rest_hour: 3,
            kiho_per_rest_hour: 0.0,
            onmyodo_per_rest_hour: 0.0,
            yokaijutsu_per_rest_hour: 0.5,
            kamishin_per_rest_hour: 0.0,
        })
        .insert(GrowthAttributes::default())
        .insert(GrowthCurve::spirit_mage_curve())
        .insert(ExtraHp { current: 40, max: 40 })
        .insert(EquipmentLoadout::with_allowed_types([
            (EquipmentSlotType::Weapon, vec![EquipmentType::Weapon(WeaponType::Staff)]),
            (EquipmentSlotType::Armor, vec![EquipmentType::Armor(ArmorType::Robe)]),
            (
                EquipmentSlotType::Accessory,
                vec![
                    EquipmentType::Accessory(AccessoryType::Charm),
                    EquipmentType::Accessory(AccessoryType::Relic),
                ],
            ),
        ]))
        // Shadow touch, Whisper, See unseen, Kuro's grasp, Night veil, Shared pain.
        .insert(Abilities(vec![26624, 26625, 26626, 26627, 26628, 26629]))
        .insert(Experience(0))
        .insert(Level(1))
        .insert(AccumulatedSpeed(0))
        .insert(SpiritMediumBehavior)
        .insert(StatModifiers(Vec::new()))
        .insert(SkillPoints::default())
        .insert(LearnedSkills::default())
        .insert(MagicCostMultipliers::default())
        // Toshiko: Yokaijutsu only, plus her Vessel class tree.
        .insert(
            SkillTreeAccess::new()
                .with_universal()
                .with_magic([MagicSchool::Yokaijutsu])
                .with(SkillTreeKind::ToshikoVessel),
        )
        .id();

    // register participants in turn manager
    tm.participants.push(rina);
    tm.participants.push(sayaka);
    tm.participants.push(houjou);
    tm.participants.push(toshiko);

    info!(
        "Spawned the four bound: Rina {:?}, Sayaka {:?}, Houjou {:?}, Toshiko {:?} (sword {:?})",
        rina, sayaka, houjou, toshiko, sword
    );
}

/// -----------------------------
/// App Setup
/// -----------------------------
pub struct CombatPlugin;

fn init_messages(mut commands: Commands) {
    commands.init_resource::<Messages<DeathEvent>>();
}

fn load_ability_tree_system(mut ability_tree: ResMut<Ability_Tree>) {
    let Ok(contents) = std::fs::read_to_string("assets/data/abilities/AbilitiesExample.ron") else {
        warn!("Unable to load abilities from assets/data/abilities/AbilitiesExample.ron");
        return;
    };

    match ron::de::from_str::<Vec<Ability>>(&contents) {
        Ok(abilities) => {
            ability_tree.0 = AbilityTree::new();
            for ability in abilities {
                ability_tree.0.insert(ability);
            }
        }
        Err(err) => warn!("Failed to parse abilities file: {err}"),
    }
}

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        // TO DO: insert all systems correctly
        app.insert_resource(TurnOrder::default())
            .insert_resource(TurnManager::default())
            .insert_resource(TurnInProgress::default())
            .insert_resource(InventoryItemCatalog::default())
            .insert_resource(Ability_Tree(AbilityTree::new()))
            .insert_resource(PendingPlayerAction::default())
            // events
            .add_message::<RestEvent>()
            .add_message::<BeforeRestEvent>()
            .add_message::<AfterRestEvent>()
            .add_message::<AwardXpEvent>()
            .add_message::<AttackIntentEvent>()
            .add_message::<AbilityIntentEvent>()
            .add_message::<DefendIntentEvent>()
            .add_message::<WaitIntentEvent>()
            .add_message::<PlayerActionEvent>()
            .add_message::<BeforeAttackEvent>()
            .add_message::<AttackExecuteEvent>()
            .add_message::<BeforeHitEvent>()
            .add_message::<HealEvent>()
            .add_message::<ApplyBuffEvent>()
            .add_message::<DamageEvent>()
            .add_message::<UseItemIntentEvent>()
            .add_message::<GiveItemIntentEvent>()
            .add_message::<ItemTransferredEvent>()
            .add_message::<ItemUsedEvent>()
            .add_message::<AfterHitEvent>()
            .add_message::<AfterAttackEvent>()
            .add_message::<DeathEvent>()
            .add_message::<SummonEvent>()
            .add_message::<ResurrectionRequestedEvent>()
            .add_message::<ResurrectedEvent>()
            .add_message::<ReactionTriggeredEvent>()
            .add_message::<LevelUpEvent>()
            .add_message::<TurnOrderCalculatedEvent>()
            .add_message::<TurnStartEvent>()
            .add_message::<TurnEndEvent>()
            .add_message::<RoundEndEvent>()
            // startup
            // Disable the demo auto-battle spawns so the game starts in exploration without combat noise.
            .add_systems(Startup, init_messages)
            .add_systems(Startup, load_ability_tree_system.after(init_messages))
            // xp / leveling systems
            .add_systems(Update, award_xp_system)
            .add_systems(Update, level_up_system.after(award_xp_system))
            // turn systems
            .add_systems(Update, register_participants_system)
            .add_systems(Update, compute_turn_order_system.after(register_participants_system))
            .add_systems(Update, auto_advance_after_order.after(compute_turn_order_system))
            .add_systems(Update, on_turn_start_system.after(auto_advance_after_order))
            .add_systems(Update, buff_tick_on_turn_start_system.after(on_turn_start_system))
            .add_systems(Update, advance_turn_system.after(compute_turn_order_system))
            .add_systems(Update, buff_tick_system)
            .add_systems(Update, process_player_action_system)
            // combat pipeline (core)
            .add_systems(Update, process_attack_intent)
            .add_systems(Update, resolve_give_item_intent_system)
            .add_systems(
                Update,
                resolve_use_item_intent_system.after(process_player_action_system),
            )
            .add_systems(
                Update,
                (
                    paladin_before_attack_system,
                    rogue_backstab_system,
                    equipment_before_attack_listener,
                    weapon_before_attack_effect_system,
                    apply_retarget_overrides_system,
                    queue_damage_from_before_attack,
                    before_to_execute,
                    dull_weapon_on_attack_system,
                )
                    .chain()
                    .after(process_attack_intent),
            )
            .add_systems(Update, before_hit_listeners.after(before_to_execute))
            .add_systems(Update, apply_heal_system)
            .add_systems(Update, apply_buff_system)
            .add_systems(Update, process_damage_queue_system.after(queue_damage_from_before_attack))
            .add_systems(Update, apply_damage_system.after(process_damage_queue_system))
            .add_systems(Update, after_hit_listeners.after(apply_damage_system))
            .add_systems(Update, after_attack_finalizers.after(after_hit_listeners))
            // supporting
            // Health/magic regen are owned by StatusEffectsPlugin so regen
            // multipliers (Slow/Minimal Regeneration, Crippled Spirit, Starved)
            // can scale them without double-ticking.
            // Rest pipeline first stage: fan RestEvent out to per-target
            // BeforeRestEvent. `rest_regen_system` (in StatusEffectsPlugin)
            // chains after this and emits AfterRestEvent.
            .add_systems(Update, expand_rest_intent_system)
            .add_systems(Update, enqueue_resurrection_on_death_system)
            .add_systems(Update, process_resurrection_queue_system)
            .add_systems(Update, teleport_on_resurrection.after(process_resurrection_queue_system))
            .add_systems(
                Update,
                forfeit_turn_on_status_system.after(on_turn_start_system),
            )
            .add_systems(Update, reaction_cooldown_tick_system)
            .add_systems(
                Update,
                evaluate_when_attacked_reactions_system.before(process_attack_intent),
            )
            .add_systems(Update, evaluate_when_ally_damaged_reactions_system)
            .add_systems(Update, resolve_reaction_intent_system)
            .add_systems(Update, debug_print_system);
    }
}

// fn main() {
//     App::new()
//         .add_plugins(DefaultPlugins)
//         .add_plugin(CombatPlugin)
//         .run();
// }
