//! Bad Conditions, Debuffs, and Contract Debuffs.
//!
//! Mirrors GDD Parts 1–3. The taxonomy is fixed by the GDD; this module owns the
//! data model, application/expiration, and a small set of read-side helpers that
//! the combat pipeline consults when computing damage, healing, AP, etc.
//!
//! Three things deliberately differ from the existing `Buff` / `StatModifiers`
//! used by abilities:
//! - Stacking rule (no stack past highest tier; reapplying refreshes duration).
//! - Multiple expiry semantics (turns, hours, end of combat, until cleansed,
//!   until atonement).
//! - Effects can be non-multiplicative (DoT, AP penalties, heal gates,
//!   incoming-damage multipliers conditional on damage type, etc.).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat_plugin::{
    AfterRestEvent, BeforeRestEvent, CombatStats, DamageType, MagicSchool, RoundEndEvent, StatPool,
    TurnEndEvent,
};
use crate::constants::TIMESTAMP_TICKS_PER_HOUR;
use crate::core::Timestamp;

// ---------------------------------------------------------------------------
// Kind taxonomy
// ---------------------------------------------------------------------------

/// Bad Conditions — short-term, GDD Part 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BadConditionKind {
    Bleeding,
    Staggered,
    Slowed,
    Confused,
    Terrified,
    Exposed,
    Silenced,
    Blinded,
    ShatteredResolve,
    Crippled,
    Broken,
    Haunted,
    MagicalWound,
    CursedWound,
}

/// Debuffs — Minor and Severe, GDD Part 2. Folded into one enum because they
/// share the same lifecycle and removal machinery; severity is implicit in the
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DebuffKind {
    // Minor
    SlowRegeneration,
    CrippledSpirit,
    Drained,
    Depleted,
    LingeringWounds,
    Fragile,
    Sluggish,
    Unlucky,
    Unfocused,
    // Severe
    CrippledDefense,
    ExhaustingCost,
    MinimalRegeneration,
    Starved,
    ShatteredSpirit,
    EmptyVessel,
    EternalWounds,
    BrokenBody,
    Paralyzed,
    HauntedDreams,
}

/// Buffs — GDD Part 3. Mirror image of debuffs: tier-based, same stacking rule
/// ("don't stack past highest tier; reapply refreshes"). Folded into a single
/// enum because Minor and Severe share the same lifecycle and machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuffKind {
    // Minor
    FastRegeneration,
    FocusedSpirit,       // single-resource regen boost (uses resource_focus)
    BolsteredMorale,     // max morale increase
    OverflowingVessel,   // max magic increase
    SteadyRecovery,      // bad-condition duration shortening
    Fortified,           // incoming damage reduction
    Swift,               // +AP at turn start
    Lucky,               // allies +hit chance (party-wide; deferred — see below)
    SharpenedFocus,      // attackers -hit chance against you
    // Severe
    BlessedDefense,      // attackers -hit chance + armor multiplier
    EfficientCasting,    // magic costs less
    AbundantRegeneration,// broad regen multiplier
    OverflowingRenewal,  // single-resource regen boost (uses resource_focus)
    UnbreakableSpirit,   // max morale (severe)
    SacredReserve,       // max magic (severe)
    RapidRecovery,       // bad-condition duration shortening (severe; tier 3 clears)
    IronBody,            // incoming damage reduction (severe)
    Overclocked,         // +AP at turn start (severe)
    PropheticCalm,       // morale per rest hour bonus
}

/// Contract Debuffs — GDD Part 3. Removed only by atonement, Merchant favors,
/// or Contract completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContractDebuffKind {
    /// Rule II — Veil. Cannot speak for 3 days; spells still work.
    TongueOfAsh,
    /// Rule III — Bound Blade. Damage to ally also damages you (unhealable).
    MirrorWound,
    /// Rule IV — Calling. Refused hunt becomes forced; loss = game over.
    DenyTheContract,
    /// Rule VI — Sunrises. Rest restoration penalty per delayed hour, max -50%.
    StolenHours,
    /// Rule VIII — Shared Path. Half of coins gained go to the wronged bound.
    LoneShadow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StatusKind {
    BadCondition(BadConditionKind),
    Debuff(DebuffKind),
    Buff(BuffKind),
    Contract(ContractDebuffKind),
}

/// The signature status an on-wheel hit applies, by its phase + polarity
/// (§7 of `docs/gogyo_elemental_system.md`). Yō phases lean *offensive*, In
/// phases lean *control / drain*.
///
/// **Interim mapping:** these reuse the closest *existing* status kinds rather
/// than introducing dedicated Burn / Soak / Bloom / etc. (which would each need
/// their own tick/effect wiring). The flavour is approximate — e.g. Fire-Yō
/// "Burn" rides the Bleeding DoT engine, Water-Yō "Soak" uses Exposed
/// (next-hit-amplified). Dedicated statuses are future content.
pub fn phase_proc_status(
    phase: crate::gogyo::Phase,
    polarity: crate::gogyo::Polarity,
) -> Option<(StatusKind, Tier)> {
    use crate::gogyo::{Phase, Polarity};
    let kind = match (phase, polarity) {
        // Fire — Yō burns (DoT), In smoulders (impaired recovery).
        (Phase::Fire, Polarity::Yo) => StatusKind::BadCondition(BadConditionKind::Bleeding),
        (Phase::Fire, Polarity::In) => StatusKind::Debuff(DebuffKind::SlowRegeneration),
        // Water — Yō soaks (next hit amplified), In chills (slow).
        (Phase::Water, Polarity::Yo) => StatusKind::BadCondition(BadConditionKind::Exposed),
        (Phase::Water, Polarity::In) => StatusKind::BadCondition(BadConditionKind::Slowed),
        // Metal — Yō severs (DoT), In dulls (accuracy down).
        (Phase::Metal, Polarity::Yo) => StatusKind::BadCondition(BadConditionKind::Bleeding),
        (Phase::Metal, Polarity::In) => StatusKind::BadCondition(BadConditionKind::Blinded),
        // Wood — Yō blooms (vulnerability), In entangles (root ≈ slow).
        (Phase::Wood, Polarity::Yo) => StatusKind::Debuff(DebuffKind::Fragile),
        (Phase::Wood, Polarity::In) => StatusKind::BadCondition(BadConditionKind::Slowed),
        // Earth — Yō staggers, In weighs down.
        (Phase::Earth, Polarity::Yo) => StatusKind::BadCondition(BadConditionKind::Staggered),
        (Phase::Earth, Polarity::In) => StatusKind::Debuff(DebuffKind::Sluggish),
    };
    Some((kind, 1))
}

/// Single-resource selector for effects like Crippled Spirit and Starved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    Health,
    Magic,
    Morale,
}

/// Tier 1 / 2 / 3 from the GDD tables.
pub type Tier = u8;

/// Rules by which an effect ends.
///
/// All time-bounded effects use `AtTimestamp` against the global `Timestamp`
/// resource, which advances by 1 per combat turn (~9 seconds in-world per
/// [`crate::constants::TIMESTAMP_SECONDS_PER_TICK`]) and by larger jumps
/// during travel / inn rest. So a "4 turn" Stagger and a "6 hour" Crippled
/// share one expiry path: their end timestamp.
#[derive(Debug, Clone, Copy)]
pub enum Expiry {
    AtTimestamp(u32),
    EndOfCombat,
    UntilCleansed,
    UntilAtonement,
}

#[derive(Debug, Clone)]
pub struct StatusInstance {
    pub kind: StatusKind,
    pub tier: Tier,
    pub expiry: Expiry,
    pub source: Option<Entity>,
    /// Per-instance scratch for DoT cadence (e.g. Bleeding ticks every 2 turns).
    pub dot_counter: u8,
    /// Which resource this instance targets, when the kind is single-resource
    /// (Crippled Spirit, Starved). Ignored for other kinds.
    pub resource_focus: Option<ResourceKind>,
}

#[derive(Component, Debug, Default)]
pub struct StatusEffects(pub Vec<StatusInstance>);

impl StatusEffects {
    /// GDD: "Effects of the same type do not stack beyond their highest tier,
    /// and reapplying them refreshes their duration."
    ///
    /// "Same type" is keyed on `(kind, resource_focus)` so that, e.g., a
    /// Crippled Spirit on Health and another on Magic coexist, but two
    /// Crippled Spirits on Health collapse to the highest tier.
    pub fn apply(&mut self, new: StatusInstance) {
        if let Some(existing) = self
            .0
            .iter_mut()
            .find(|s| s.kind == new.kind && s.resource_focus == new.resource_focus)
        {
            if new.tier > existing.tier {
                existing.tier = new.tier;
            }
            existing.expiry = new.expiry;
            if new.source.is_some() {
                existing.source = new.source;
            }
            return;
        }
        self.0.push(new);
    }

    pub fn remove_kind(&mut self, kind: StatusKind) {
        self.0.retain(|s| s.kind != kind);
    }

    pub fn has(&self, kind: StatusKind) -> bool {
        self.0.iter().any(|s| s.kind == kind)
    }

    pub fn tier_of(&self, kind: StatusKind) -> Option<Tier> {
        self.0.iter().find(|s| s.kind == kind).map(|s| s.tier)
    }

    /// Highest tier of `kind` whose `resource_focus` matches `resource`.
    /// Used by single-resource regen debuffs.
    pub fn tier_of_focused(&self, kind: StatusKind, resource: ResourceKind) -> Option<Tier> {
        self.0
            .iter()
            .filter(|s| s.kind == kind && s.resource_focus == Some(resource))
            .map(|s| s.tier)
            .max()
    }
}

// ---------------------------------------------------------------------------
// Default duration table (GDD Part 1 + Part 2)
// ---------------------------------------------------------------------------

/// Returns the GDD-specified expiry for a freshly applied effect at `tier`,
/// using `now` as the current Timestamp for hour-based effects.
pub fn default_expiry(kind: StatusKind, _tier: Tier, now: u32) -> Expiry {
    use BadConditionKind::*;

    // Helper: shorthand for "N combat turns from now" — 1 turn = 1 timestamp tick.
    let in_turns = |n: u32| Expiry::AtTimestamp(now.saturating_add(n));
    let in_hours = |n: u32| Expiry::AtTimestamp(now.saturating_add(n * TIMESTAMP_TICKS_PER_HOUR));

    match kind {
        StatusKind::BadCondition(bc) => match bc {
            Bleeding => in_turns(6),
            Staggered => in_turns(4),
            Slowed => in_turns(4),
            Confused => in_turns(3),
            Terrified => in_turns(3),
            Exposed => in_turns(1),
            Silenced => in_turns(4),
            Blinded => Expiry::EndOfCombat,
            Haunted => in_turns(5),
            MagicalWound | CursedWound => Expiry::EndOfCombat,
            ShatteredResolve | Crippled => in_hours(6),
            Broken => in_hours(12),
        },
        // Debuffs persist out of combat; cleansed by ritual / medicine / shrine.
        StatusKind::Debuff(_) => Expiry::UntilCleansed,
        StatusKind::Buff(b) => match b {
            // Resource-cap and rest-hour bonuses feel like blessings — keep
            // them around for a few in-game hours by default.
            BuffKind::BolsteredMorale
            | BuffKind::OverflowingVessel
            | BuffKind::UnbreakableSpirit
            | BuffKind::SacredReserve
            | BuffKind::PropheticCalm => in_hours(6),
            // Combat-time buffs default to 5 turns; specific abilities can
            // override via `expiry_override`.
            _ => in_turns(5),
        },
        StatusKind::Contract(_) => Expiry::UntilAtonement,
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Message)]
pub struct ApplyStatusEvent {
    pub target: Entity,
    pub kind: StatusKind,
    /// Clamped to 1..=3 on apply.
    pub tier: Tier,
    pub source: Option<Entity>,
    /// Override the default GDD duration. Useful for `LingeringWounds` /
    /// `EternalWounds` extensions and for atonement-locked effects with
    /// already-known expiry.
    pub expiry_override: Option<Expiry>,
    /// Required for single-resource debuffs (Crippled Spirit, Starved).
    /// Ignored otherwise.
    pub resource_focus: Option<ResourceKind>,
}

#[derive(Debug, Clone, Message)]
pub struct RemoveStatusEvent {
    pub target: Entity,
    pub kind: StatusKind,
}

// ---------------------------------------------------------------------------
// Apply / remove systems
// ---------------------------------------------------------------------------

pub fn apply_status_system(
    mut commands: Commands,
    mut reader: MessageReader<ApplyStatusEvent>,
    mut q: Query<&mut StatusEffects>,
    timestamp: Res<Timestamp>,
) {
    for ev in reader.read() {
        let tier = ev.tier.clamp(1, 3);

        // Read existing status (for Lingering/Eternal Wounds extension) through
        // the same query — `Query::get` on a `Query<&mut T>` returns a shared
        // borrow, so we can compute the extension factor without spawning a
        // second StatusEffects query that would conflict with this one.
        let extension_factor = if matches!(ev.kind, StatusKind::BadCondition(_)) {
            let existing = q.get(ev.target).ok();
            bad_condition_duration_multiplier(existing.as_deref())
        } else {
            1.0
        };

        let base_expiry = ev
            .expiry_override
            .unwrap_or_else(|| default_expiry(ev.kind, tier, timestamp.0));
        let expiry = scale_expiry(base_expiry, extension_factor, timestamp.0);

        let inst = StatusInstance {
            kind: ev.kind,
            tier,
            expiry,
            source: ev.source,
            dot_counter: 0,
            resource_focus: ev.resource_focus,
        };

        if let Ok(mut se) = q.get_mut(ev.target) {
            se.apply(inst);
        } else {
            let mut se = StatusEffects::default();
            se.apply(inst);
            commands.entity(ev.target).insert(se);
        }
    }
}

pub fn remove_status_system(
    mut reader: MessageReader<RemoveStatusEvent>,
    mut q: Query<&mut StatusEffects>,
) {
    for ev in reader.read() {
        if let Ok(mut se) = q.get_mut(ev.target) {
            se.remove_kind(ev.kind);
        }
    }
}

fn scale_expiry(expiry: Expiry, factor: f32, now: u32) -> Expiry {
    if (factor - 1.0).abs() < f32::EPSILON {
        return expiry;
    }
    match expiry {
        Expiry::AtTimestamp(end) => {
            let remaining = end.saturating_sub(now) as f32;
            let scaled = (remaining * factor).round() as u32;
            Expiry::AtTimestamp(now.saturating_add(scaled))
        }
        other => other,
    }
}

fn bad_condition_duration_multiplier(existing: Option<&StatusEffects>) -> f32 {
    let Some(se) = existing else { return 1.0 };
    use BuffKind::*;
    use DebuffKind::*;

    // Debuff side (extends durations). Severe (Eternal Wounds) dominates
    // Minor (Lingering Wounds).
    let extend = if let Some(t) = se.tier_of(StatusKind::Debuff(EternalWounds)) {
        match t {
            1 => 6.0,
            2 => 8.0,
            _ => 10.0,
        }
    } else if let Some(t) = se.tier_of(StatusKind::Debuff(LingeringWounds)) {
        match t {
            1 => 2.0,
            2 => 3.5,
            _ => 5.0,
        }
    } else {
        1.0
    };

    // Buff side (shortens durations). Rapid Recovery dominates Steady
    // Recovery; tier 3 of Rapid Recovery clears the bad condition outright
    // (returned as 0.0 so the apply path can detect and skip).
    let shorten = if let Some(t) = se.tier_of(StatusKind::Buff(RapidRecovery)) {
        match t {
            1 => 0.50,
            2 => 0.25,
            _ => 0.0,
        }
    } else if let Some(t) = se.tier_of(StatusKind::Buff(SteadyRecovery)) {
        match t {
            1 => 0.75,
            2 => 0.60,
            _ => 0.40,
        }
    } else {
        1.0
    };

    extend * shorten
}

// ---------------------------------------------------------------------------
// Tick systems
// ---------------------------------------------------------------------------

/// Per-character DoT (Bleeding) ticks. Fires on the affected entity's
/// `TurnEndEvent` so DoT damage lands on *their* turn (their action triggers
/// the cadence). Duration / expiry is timestamp-based and handled by
/// `status_expiry_tick_system`, not here.
pub fn status_turn_end_tick_system(
    mut reader: MessageReader<TurnEndEvent>,
    mut status_q: Query<(&mut StatusEffects, &CombatStats)>,
    mut damage_writer: MessageWriter<crate::combat_plugin::DamageEvent>,
) {
    for ev in reader.read() {
        let Ok((mut se, stats)) = status_q.get_mut(ev.who) else {
            continue;
        };

        for s in se.0.iter_mut() {
            if let StatusKind::BadCondition(BadConditionKind::Bleeding) = s.kind {
                s.dot_counter = s.dot_counter.saturating_add(1);
                if s.dot_counter >= 2 {
                    s.dot_counter = 0;
                    let pct = match s.tier {
                        1 => 0.03,
                        2 => 0.05,
                        _ => 0.07,
                    };
                    let dmg = ((stats.health.base as f32) * pct).round() as i32;
                    if dmg > 0 {
                        damage_writer.write(crate::combat_plugin::DamageEvent {
                            attacker: s.source.unwrap_or(ev.who),
                            target: ev.who,
                            amount: dmg,
                            damage_type: DamageType::True,
                            cause: crate::combat_plugin::ActionCause::StatusEffect {
                                source: ev.who,
                            },
                        });
                    }
                }
            }
        }
    }
}

/// Universal expiry sweep: drops every effect whose `AtTimestamp` deadline
/// has passed. Runs whenever `Timestamp` changes, which covers both combat
/// (each turn ticks +1) and world (travel / inn jumps by hours).
pub fn status_expiry_tick_system(
    timestamp: Res<Timestamp>,
    mut q: Query<&mut StatusEffects>,
) {
    if !timestamp.is_changed() {
        return;
    }
    let now = timestamp.0;
    for mut se in q.iter_mut() {
        se.0.retain(|s| match s.expiry {
            Expiry::AtTimestamp(end) => now < end,
            _ => true,
        });
    }
}

/// On RoundEnd we treat the encounter as ending (combat module's lifecycle
/// will be tightened later — this is the cleanest hook available right now)
/// and drop `EndOfCombat` statuses.
pub fn status_end_of_combat_system(
    mut reader: MessageReader<RoundEndEvent>,
    mut q: Query<&mut StatusEffects>,
) {
    let mut fired = false;
    for _ in reader.read() {
        fired = true;
    }
    if !fired {
        return;
    }
    for mut se in q.iter_mut() {
        se.0.retain(|s| !matches!(s.expiry, Expiry::EndOfCombat));
    }
}

// ---------------------------------------------------------------------------
// Read-side helpers — consumed by the combat pipeline
// ---------------------------------------------------------------------------

/// How an outgoing attack should be modulated by the *attacker's* status.
/// Multiplicative on lethality/hit; flat penalty on hit-percentage applied later.
#[derive(Debug, Clone, Copy, Default)]
pub struct OutgoingMods {
    pub lethality_mult: f32,
    pub hit_mult: f32,
    /// Flat shift to final hit chance in 0..=1, sourced from Unfocused.
    pub hit_chance_shift: f32,
}

impl OutgoingMods {
    pub fn identity() -> Self {
        Self {
            lethality_mult: 1.0,
            hit_mult: 1.0,
            hit_chance_shift: 0.0,
        }
    }
}

pub fn outgoing_mods(se: Option<&StatusEffects>) -> OutgoingMods {
    let mut m = OutgoingMods::identity();
    let Some(se) = se else { return m };
    use BadConditionKind::*;
    use DebuffKind::*;

    if let Some(t) = se.tier_of(StatusKind::BadCondition(Staggered)) {
        let (leth, hit) = match t {
            1 => (0.85, 0.75),
            2 => (0.75, 0.65),
            _ => (0.65, 0.55),
        };
        m.lethality_mult *= leth;
        m.hit_mult *= hit;
    }
    if let Some(t) = se.tier_of(StatusKind::Debuff(Unfocused)) {
        m.hit_chance_shift -= match t {
            1 => 0.10,
            2 => 0.20,
            _ => 0.30,
        };
    }
    m
}

/// How an incoming attack should be modulated by the *target's* status.
#[derive(Debug, Clone, Copy, Default)]
pub struct IncomingMods {
    pub damage_mult: f32,
    pub armor_mult: f32,
    /// Flat hit-chance bonus *for the attacker*, sourced from Unlucky and
    /// Crippled Defense.
    pub attacker_hit_chance_shift: f32,
}

impl IncomingMods {
    pub fn identity() -> Self {
        Self {
            damage_mult: 1.0,
            armor_mult: 1.0,
            attacker_hit_chance_shift: 0.0,
        }
    }
}

pub fn incoming_mods(se: Option<&StatusEffects>, damage_type: DamageType) -> IncomingMods {
    let mut m = IncomingMods::identity();
    let Some(se) = se else { return m };
    use BadConditionKind::*;
    use BuffKind::*;
    use DebuffKind::*;

    // --- Damage multipliers -------------------------------------------------
    // Debuff side: Fragile + Broken Body stack on top of one another (different
    // families). Buff side: Iron Body dominates Fortified.
    if let Some(t) = se.tier_of(StatusKind::Debuff(Fragile)) {
        m.damage_mult *= match t {
            1 => 1.33,
            2 => 1.5,
            _ => 2.0,
        };
    }
    if let Some(t) = se.tier_of(StatusKind::Debuff(BrokenBody)) {
        m.damage_mult *= match t {
            1 => 2.0,
            2 => 2.5,
            _ => 3.0,
        };
    }
    if let Some(t) = se.tier_of(StatusKind::Buff(IronBody)) {
        m.damage_mult *= match t {
            1 => 0.66,
            2 => 0.5,
            _ => 0.33,
        };
    } else if let Some(t) = se.tier_of(StatusKind::Buff(Fortified)) {
        m.damage_mult *= match t {
            1 => 0.85,
            2 => 0.7,
            _ => 0.5,
        };
    }

    // --- Armor multipliers --------------------------------------------------
    if let Some(t) = se.tier_of(StatusKind::Debuff(CrippledDefense)) {
        m.armor_mult *= match t {
            1 => 0.70,
            2 => 0.55,
            _ => 0.40,
        };
    }
    if let Some(t) = se.tier_of(StatusKind::Buff(BlessedDefense)) {
        m.armor_mult *= match t {
            1 => 1.3,
            2 => 1.5,
            _ => 2.0,
        };
    }

    // --- Attacker hit-chance shifts ----------------------------------------
    // Debuffs raise it (easier to hit you); buffs lower it (harder to hit).
    if let Some(t) = se.tier_of(StatusKind::Debuff(CrippledDefense)) {
        m.attacker_hit_chance_shift += match t {
            1 => 0.20,
            2 => 0.30,
            _ => 0.40,
        };
    }
    // Unlucky lives in `lucky_unlucky_shift` so it shares one signed helper
    // with its mirror buff (Lucky on an ally → +, Unlucky on the target → +).
    // Both hit the same `+ tier * 0.10` curve.
    if let Some(t) = se.tier_of(StatusKind::Buff(BlessedDefense)) {
        m.attacker_hit_chance_shift -= match t {
            1 => 0.20,
            2 => 0.30,
            _ => 0.40,
        };
    }
    if let Some(t) = se.tier_of(StatusKind::Buff(SharpenedFocus)) {
        m.attacker_hit_chance_shift -= match t {
            1 => 0.10,
            2 => 0.20,
            _ => 0.30,
        };
    }

    if matches!(damage_type, DamageType::True) {
        // True damage mostly represents internal/spiritual damage in this codebase
        // (e.g. Bleeding ticks). Haunted boosts mental damage; we treat True as
        // mental-coded for now and refine when DamageType::Mental is introduced.
        if let Some(t) = se.tier_of(StatusKind::BadCondition(Haunted)) {
            m.damage_mult *= match t {
                1 => 1.5,
                2 => 2.0,
                _ => 3.0,
            };
        }
    }
    m
}

/// Returns the post-Exposed multiplier and consumes the Exposed instance if
/// present. Call this exactly once per resolved hit.
pub fn consume_exposed(se: &mut StatusEffects) -> f32 {
    use BadConditionKind::*;
    let mut mult = 1.0;
    se.0.retain(|s| {
        if matches!(s.kind, StatusKind::BadCondition(Exposed)) {
            mult *= match s.tier {
                1 => 1.5,
                2 => 2.0,
                _ => 2.5,
            };
            false
        } else {
            true
        }
    });
    mult
}

/// How healing should be gated by the target's status.
#[derive(Debug, Clone, Copy)]
pub struct HealGate {
    /// 0.0..=1.0 multiplier to the heal amount.
    pub mult: f32,
}

impl HealGate {
    pub fn full() -> Self {
        Self { mult: 1.0 }
    }
}

pub fn heal_gate(se: Option<&StatusEffects>) -> HealGate {
    let Some(se) = se else { return HealGate::full() };
    use BadConditionKind::*;

    if se.has(StatusKind::BadCondition(CursedWound)) {
        return HealGate { mult: 0.0 };
    }
    if let Some(t) = se.tier_of(StatusKind::BadCondition(MagicalWound)) {
        return HealGate {
            mult: match t {
                1 => 0.5,
                2 => 0.25,
                _ => 0.0,
            },
        };
    }
    HealGate::full()
}

/// Flat AP modifier applied at TurnStart after the base refill. Negative for
/// debuffs (Sluggish, Paralyzed), positive for buffs (Swift, Overclocked).
pub fn ap_modifier(se: Option<&StatusEffects>) -> i32 {
    let Some(se) = se else { return 0 };
    use BuffKind::*;
    use DebuffKind::*;

    let mut total = 0_i32;

    if let Some(t) = se.tier_of(StatusKind::Debuff(Sluggish)) {
        total -= match t {
            1 => 1,
            2 => 2,
            _ => 3,
        };
    }
    if let Some(t) = se.tier_of(StatusKind::Debuff(Paralyzed)) {
        total -= match t {
            1 => 2,
            2 => 3,
            _ => 4,
        };
    }
    if let Some(t) = se.tier_of(StatusKind::Buff(Swift)) {
        total += match t {
            1 => 1,
            2 => 2,
            _ => 3,
        };
    }
    if let Some(t) = se.tier_of(StatusKind::Buff(Overclocked)) {
        total += match t {
            1 => 2,
            2 => 3,
            _ => 4,
        };
    }
    total
}

// ---------------------------------------------------------------------------
// Regen multipliers
// ---------------------------------------------------------------------------

/// Combined regen multiplier for `resource`, given the target's status set.
///
/// Stacking model:
/// - Within the broad-regen group (`SlowRegeneration` + `MinimalRegeneration`)
///   the *worst* multiplier wins — Severe overrides Minor of the same family.
/// - Within the focused-regen group (`CrippledSpirit` + `Starved`) only
///   instances whose `resource_focus` matches `resource` apply, and again the
///   worst multiplier wins.
/// - The two groups are independent and multiply together: a character with
///   broad Slow Regeneration *and* a Magic-targeted Starved gets both hits to
///   their Magic regen.
pub fn regen_multiplier(se: Option<&StatusEffects>, resource: ResourceKind) -> f32 {
    let Some(se) = se else { return 1.0 };
    use BuffKind::*;
    use DebuffKind::*;

    // Debuff side: pick worst (smallest multiplier) within each family.
    let broad_debuff = {
        let minimal = se
            .tier_of(StatusKind::Debuff(MinimalRegeneration))
            .map(|t| match t {
                1 => 1.0 / 3.0,
                2 => 0.25,
                _ => 0.20,
            });
        let slow = se
            .tier_of(StatusKind::Debuff(SlowRegeneration))
            .map(|t| match t {
                1 => 2.0 / 3.0,
                2 => 0.5,
                _ => 1.0 / 3.0,
            });
        match (minimal, slow) {
            (Some(a), Some(b)) => f32::min(a, b),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => 1.0,
        }
    };

    let focused_debuff = {
        let starved = se
            .tier_of_focused(StatusKind::Debuff(Starved), resource)
            .map(|t| match t {
                1 => 0.20,
                2 => 1.0 / 6.0,
                _ => 1.0 / 7.0,
            });
        let crippled_spirit = se
            .tier_of_focused(StatusKind::Debuff(CrippledSpirit), resource)
            .map(|t| match t {
                1 => 1.0 / 3.0,
                2 => 0.25,
                _ => 0.20,
            });
        match (starved, crippled_spirit) {
            (Some(a), Some(b)) => f32::min(a, b),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => 1.0,
        }
    };

    // Buff side: pick best (largest multiplier) within each family.
    let broad_buff = {
        let abundant = se
            .tier_of(StatusKind::Buff(AbundantRegeneration))
            .map(|t| match t {
                1 => 2.0,
                2 => 2.5,
                _ => 3.0,
            });
        let fast = se
            .tier_of(StatusKind::Buff(FastRegeneration))
            .map(|t| match t {
                1 => 1.33,
                2 => 1.5,
                _ => 2.0,
            });
        match (abundant, fast) {
            (Some(a), Some(b)) => f32::max(a, b),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => 1.0,
        }
    };

    let focused_buff = {
        let renewal = se
            .tier_of_focused(StatusKind::Buff(OverflowingRenewal), resource)
            .map(|t| match t {
                1 => 3.0,
                2 => 4.0,
                _ => 5.0,
            });
        let focused_spirit = se
            .tier_of_focused(StatusKind::Buff(FocusedSpirit), resource)
            .map(|t| match t {
                1 => 1.5,
                2 => 2.0,
                _ => 3.0,
            });
        match (renewal, focused_spirit) {
            (Some(a), Some(b)) => f32::max(a, b),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => 1.0,
        }
    };

    (broad_debuff * focused_debuff * broad_buff * focused_buff).max(0.0)
}

// ---------------------------------------------------------------------------
// Max-cap multipliers
// ---------------------------------------------------------------------------

/// Multiplier applied to *all* magic pools' max capacity.
/// Within each family (debuff / buff), severe dominates minor; the two
/// families multiply together.
///
/// GDD Note: maximum values are "rounded up". The recompute system applies the
/// ceil(); this function returns the raw factor.
pub fn magic_max_multiplier(se: Option<&StatusEffects>) -> f32 {
    let Some(se) = se else { return 1.0 };
    use BuffKind::*;
    use DebuffKind::*;

    let debuff_mult = if let Some(t) = se.tier_of(StatusKind::Debuff(EmptyVessel)) {
        match t {
            1 => 1.0 / 3.0,
            2 => 0.25,
            _ => 0.20,
        }
    } else if let Some(t) = se.tier_of(StatusKind::Debuff(Depleted)) {
        match t {
            1 => 2.0 / 3.0,
            2 => 0.5,
            _ => 1.0 / 3.0,
        }
    } else {
        1.0
    };

    let buff_mult = if let Some(t) = se.tier_of(StatusKind::Buff(SacredReserve)) {
        match t {
            1 => 1.5,
            2 => 2.0,
            _ => 3.0,
        }
    } else if let Some(t) = se.tier_of(StatusKind::Buff(OverflowingVessel)) {
        match t {
            1 => 1.25,
            2 => 1.5,
            _ => 2.0,
        }
    } else {
        1.0
    };

    debuff_mult * buff_mult
}

/// Crippled/Broken affect both Health and Morale max simultaneously per the
/// GDD's bad-condition table. This helper returns the multiplier of the more
/// punishing of the two if both are active (Broken dominates Crippled at
/// equal tiers).
fn crippled_broken_multiplier(se: &StatusEffects) -> f32 {
    use BadConditionKind::*;

    let crippled = se.tier_of(StatusKind::BadCondition(Crippled)).map(|t| match t {
        1 => 0.75,
        2 => 2.0 / 3.0,
        _ => 0.5,
    });
    let broken = se.tier_of(StatusKind::BadCondition(Broken)).map(|t| match t {
        1 => 2.0 / 3.0,
        2 => 0.5,
        _ => 1.0 / 3.0,
    });
    match (crippled, broken) {
        (Some(a), Some(b)) => f32::min(a, b),
        (Some(x), None) | (None, Some(x)) => x,
        (None, None) => 1.0,
    }
}

/// Multiplier applied to health's max capacity. Driven by Crippled / Broken
/// from the bad-condition table; no debuffs or buffs flex max-HP yet, but the
/// recompute pass goes through this helper so future ones can plug in.
pub fn health_max_multiplier(se: Option<&StatusEffects>) -> f32 {
    let Some(se) = se else { return 1.0 };
    crippled_broken_multiplier(se)
}

/// Multiplier applied to morale's max capacity. Same family-of-two pattern as
/// `magic_max_multiplier`, with Crippled/Broken folded in (they hit both
/// health and morale per the GDD).
pub fn morale_max_multiplier(se: Option<&StatusEffects>) -> f32 {
    let Some(se) = se else { return 1.0 };
    use BadConditionKind::*;
    use BuffKind::*;
    use DebuffKind::*;

    // Bad-condition side: Shattered Resolve (very harsh, in-combat) plus
    // Crippled/Broken which clip both health and morale.
    let bc_mult = {
        let shattered = se
            .tier_of(StatusKind::BadCondition(ShatteredResolve))
            .map(|t| match t {
                1 => 1.0 / 3.0,
                2 => 0.25,
                _ => 0.0,
            })
            .unwrap_or(1.0);
        let crippled_or_broken = crippled_broken_multiplier(se);
        f32::min(shattered, crippled_or_broken)
    };

    // Debuff side: Shattered Spirit (severe) > Drained (minor).
    let debuff_mult = if let Some(t) = se.tier_of(StatusKind::Debuff(ShatteredSpirit)) {
        match t {
            1 => 1.0 / 3.0,
            2 => 0.25,
            _ => 0.20,
        }
    } else if let Some(t) = se.tier_of(StatusKind::Debuff(Drained)) {
        match t {
            1 => 2.0 / 3.0,
            2 => 0.5,
            _ => 1.0 / 3.0,
        }
    } else {
        1.0
    };

    // Buff side: Unbreakable Spirit (severe) > Bolstered Morale (minor).
    let buff_mult = if let Some(t) = se.tier_of(StatusKind::Buff(UnbreakableSpirit)) {
        match t {
            1 => 1.5,
            2 => 2.0,
            _ => 3.0,
        }
    } else if let Some(t) = se.tier_of(StatusKind::Buff(BolsteredMorale)) {
        match t {
            1 => 1.25,
            2 => 1.5,
            _ => 2.0,
        }
    } else {
        1.0
    };

    bc_mult * debuff_mult * buff_mult
}

/// Single source of truth for status-driven action overrides — Silenced,
/// Terrified, and Confused all flow through here. Consumers (player action
/// handler, BeforeAttackEvent mutator, TurnStart hooks, AI emitters) read
/// only the fields they care about, instead of each consulting bespoke
/// helpers.
///
/// Keep this struct flat. If a future status needs a new gate, add a field
/// rather than spinning up a parallel helper — that's the whole point of
/// having one entry point for action overrides.
#[derive(Debug, Clone, Copy)]
pub struct ActionGates {
    /// Magic-cost abilities are blocked (Silenced T1+).
    pub block_magic_abilities: bool,
    /// Item use is blocked (Silenced T2+).
    pub block_items: bool,
    /// Dialogue / speech-bound actions are blocked (Silenced T3).
    /// Combat doesn't read this — dialogue and scripted events do.
    pub block_speech: bool,
    /// Every non-move action is blocked this turn (Terrified T2+).
    pub block_attacks: bool,
    /// First action of the turn must be a move away from the nearest enemy
    /// (Terrified T1). Honored by the movement system.
    pub force_first_action_move: bool,
    /// The actor's turn must end immediately (Terrified T3).
    pub forfeit_turn: bool,
    /// Probability in 0.0..=1.0 that the actor's intended attack target
    /// gets re-pointed to their nearest ally (Confused).
    pub confused_retarget_chance: f32,
}

impl ActionGates {
    pub const fn open() -> Self {
        Self {
            block_magic_abilities: false,
            block_items: false,
            block_speech: false,
            block_attacks: false,
            force_first_action_move: false,
            forfeit_turn: false,
            confused_retarget_chance: 0.0,
        }
    }
}

/// Read all action-time gates a status set imposes on its bearer.
pub fn action_gates(se: Option<&StatusEffects>) -> ActionGates {
    let Some(se) = se else { return ActionGates::open() };
    use BadConditionKind::*;

    let mut gates = ActionGates::open();

    // Silenced — higher tiers stack everything earlier tiers blocked.
    match se.tier_of(StatusKind::BadCondition(Silenced)) {
        Some(1) => {
            gates.block_magic_abilities = true;
        }
        Some(2) => {
            gates.block_magic_abilities = true;
            gates.block_items = true;
        }
        Some(_) => {
            gates.block_magic_abilities = true;
            gates.block_items = true;
            gates.block_speech = true;
        }
        None => {}
    }

    // Terrified.
    match se.tier_of(StatusKind::BadCondition(Terrified)) {
        Some(1) => gates.force_first_action_move = true,
        Some(2) => gates.block_attacks = true,
        Some(_) => {
            gates.block_attacks = true;
            gates.forfeit_turn = true;
        }
        None => {}
    }

    // Confused.
    if let Some(t) = se.tier_of(StatusKind::BadCondition(Confused)) {
        gates.confused_retarget_chance = match t {
            1 => 0.30,
            2 => 0.50,
            _ => 0.70,
        };
    }

    gates
}

/// Signed hit-chance shift from Lucky (buff on attacker's allies) and Unlucky
/// (debuff on target). Same `tier × 0.10` curve, single function — both
/// contribute *positively* to the attacker's hit chance ("luck" makes you hit
/// more, "unluck" on the target makes them get hit more), differing only in
/// where the source status sits.
///
/// Reads from the same battle-side / status queries the rest of the combat
/// pipeline already has, so callers don't need to pre-collect anything.
pub fn lucky_unlucky_shift(
    attacker: Entity,
    target: Entity,
    sides_q: &Query<(Entity, &crate::battle::BattleSide)>,
    status_q: &Query<&StatusEffects>,
) -> f32 {
    let scale = |tier: u8| -> f32 {
        match tier {
            1 => 0.10,
            2 => 0.20,
            _ => 0.30,
        }
    };
    let mut shift = 0.0;

    // Lucky on attacker's allies (excluding the attacker themselves).
    let attacker_side = sides_q.get(attacker).ok().map(|(_, s)| *s);
    for (other, side) in sides_q.iter() {
        if other == attacker || Some(*side) != attacker_side {
            continue;
        }
        if let Ok(se) = status_q.get(other) {
            if let Some(t) = se.tier_of(StatusKind::Buff(BuffKind::Lucky)) {
                shift += scale(t);
            }
        }
    }

    // Unlucky on the target.
    if let Ok(se) = status_q.get(target) {
        if let Some(t) = se.tier_of(StatusKind::Debuff(DebuffKind::Unlucky)) {
            shift += scale(t);
        }
    }
    shift
}

/// Multiplier on magic cost paid when casting an ability.
/// Buff side (Efficient Casting) and the future Exhausting Cost debuff
/// multiply together.
pub fn magic_cost_multiplier(se: Option<&StatusEffects>) -> f32 {
    let Some(se) = se else { return 1.0 };
    use BuffKind::*;
    use DebuffKind::*;

    let debuff_mult = if let Some(t) = se.tier_of(StatusKind::Debuff(ExhaustingCost)) {
        match t {
            1 => 2.0,
            2 => 2.5,
            _ => 3.0,
        }
    } else {
        1.0
    };

    let buff_mult = if let Some(t) = se.tier_of(StatusKind::Buff(EfficientCasting)) {
        match t {
            1 => 0.75,
            2 => 0.50,
            _ => 0.33,
        }
    } else {
        1.0
    };

    debuff_mult * buff_mult
}

// ---------------------------------------------------------------------------
// Regen / max-cap systems
// ---------------------------------------------------------------------------

/// Re-derive capability stats' `current` from `base` plus active status
/// modifiers. Runs every frame; cheap because there are only a handful of
/// stats and effects per character.
///
/// Buffs aren't modeled yet — when they land, the formula becomes
/// `current = base * (1 + buff_mults) * (1 - debuff_mults)`; until then,
/// only the debuff side applies.
pub fn recompute_combat_capability_system(
    mut q: Query<(&mut CombatStats, Option<&StatusEffects>)>,
) {
    for (mut stats, se) in q.iter_mut() {
        let outgoing = outgoing_mods(se);
        // Note: `incoming_mods` depends on damage type (Haunted boosts mental).
        // The damage-type-dependent piece stays at the read site in
        // `process_damage_queue_system`; we only fold the type-agnostic armor
        // multiplier here.
        let inc = incoming_mods(se, DamageType::Physical);

        stats.lethality.current = ((stats.lethality.base as f32) * outgoing.lethality_mult)
            .round() as i32;
        stats.hit.current = ((stats.hit.base as f32) * outgoing.hit_mult).round() as i32;
        stats.armor.current = ((stats.armor.base as f32) * inc.armor_mult).round() as i32;

        // Speed: Slowed cuts accumulation. Tier 1 → ½, Tier 2 → ¼, Tier 3 → 0.
        let speed_mult = if let Some(t) =
            se.and_then(|s| s.tier_of(StatusKind::BadCondition(BadConditionKind::Slowed)))
        {
            match t {
                1 => 0.5,
                2 => 0.25,
                _ => 0.0,
            }
        } else {
            1.0
        };
        // Staggered also drops speed by a flat percentage.
        let staggered_speed = if let Some(t) =
            se.and_then(|s| s.tier_of(StatusKind::BadCondition(BadConditionKind::Staggered)))
        {
            match t {
                1 => 0.70,
                2 => 0.60,
                _ => 0.50,
            }
        } else {
            1.0
        };
        stats.speed.current =
            ((stats.speed.base as f32) * speed_mult * staggered_speed).round() as i32;

        // Evasion isn't directly listed in the GDD's bad-condition table as a
        // multiplier; leave at base for now.
        stats.evasion.current = stats.evasion.base;

        // Mind: Staggered cuts mental defense by 20/30/40%.
        let mind_mult = if let Some(t) =
            se.and_then(|s| s.tier_of(StatusKind::BadCondition(BadConditionKind::Staggered)))
        {
            match t {
                1 => 0.80,
                2 => 0.70,
                _ => 0.60,
            }
        } else {
            1.0
        };
        stats.mind.current = ((stats.mind.base as f32) * mind_mult).round() as i32;
    }
}

/// Recompute the soft caps for resource pools whose ceiling can flex with
/// status effects (max-magic via Depleted/Empty Vessel/Overflowing Vessel/
/// Sacred Reserve, max-morale via Drained/Shattered Spirit/Bolstered Morale/
/// Unbreakable Spirit/Shattered Resolve).
///
/// `base` is left untouched (level-up writes there); we only clamp `current`
/// down when an active debuff drops the ceiling below it. Buffs raise the
/// ceiling but don't auto-fill — `current` stays where gameplay left it.
pub fn recompute_resource_caps_system(
    mut q: Query<(&mut CombatStats, Option<&StatusEffects>)>,
) {
    for (mut stats, se) in q.iter_mut() {
        // Magic schools.
        let mag_mult = magic_max_multiplier(se);
        for school in [
            MagicSchool::Kiho,
            MagicSchool::Onmyodo,
            MagicSchool::Yokaijutsu,
            MagicSchool::Kamishin,
        ] {
            let pool: &mut StatPool<f32> = stats.pool_mut(school);
            let cap = (pool.base * mag_mult).max(0.0).ceil();
            if pool.current > cap {
                pool.current = cap;
            }
        }

        // Morale.
        let mor_mult = morale_max_multiplier(se);
        let cap = ((stats.morale.base as f32) * mor_mult).max(0.0).ceil() as i32;
        if stats.morale.current > cap {
            stats.morale.current = cap;
        }

        // Health (Crippled / Broken).
        let hp_mult = health_max_multiplier(se);
        let hp_cap = ((stats.health.base as f32) * hp_mult).max(0.0).ceil() as i32;
        if stats.health.current > hp_cap {
            stats.health.current = hp_cap;
        }
    }
}

/// Apply rest regen on `BeforeRestEvent` (per-target, after any listeners
/// mutated `hours`). Multiplies the entity's `*_per_rest_hour` rates by status
/// regen multipliers and adds them to `current`, clamped to `base`. Emits
/// `AfterRestEvent` for post-rest reactions.
pub fn rest_regen_system(
    mut reader: MessageReader<BeforeRestEvent>,
    mut writer: MessageWriter<AfterRestEvent>,
    mut q: Query<(
        &mut CombatStats,
        Option<&StatusEffects>,
        Option<&crate::kegare::Defilement>,
    )>,
) {
    for ev in reader.read() {
        // Derive fractional hours from elapsed ticks. A 4-minute rest is 30
        // ticks ≈ 0.067 h.
        let hours = ev.ticks as f32 / TIMESTAMP_TICKS_PER_HOUR as f32;
        if hours <= 0.0 {
            continue;
        }
        let Ok((mut stats, se, defilement)) = q.get_mut(ev.target) else {
            continue;
        };

        let h_mult = regen_multiplier(se, ResourceKind::Health);
        let m_mult = regen_multiplier(se, ResourceKind::Magic);
        let mor_mult = regen_multiplier(se, ResourceKind::Morale);

        // The location's per-stat rates are added on top of the entity's own.
        let loc = ev.location;

        let h_gain =
            ((stats.health_per_rest_hour as f32 + loc.health) * h_mult * hours).round() as i32;
        stats.health.restore_to_base(h_gain);

        // Prophetic Calm adds +1/2/3 morale per rest hour on top of the
        // character's base rate, then is multiplied by the same
        // morale-regen multiplier.
        let prophetic_bonus = se
            .and_then(|s| s.tier_of(StatusKind::Buff(BuffKind::PropheticCalm)))
            .map(|t| t as i32)
            .unwrap_or(0);
        let base_morale_rate =
            (stats.morale_per_rest_hour + prophetic_bonus) as f32 + loc.morale;
        let mor_gain = (base_morale_rate * mor_mult * hours).round() as i32;
        stats.morale.restore_to_base(mor_gain);

        // Haunted Dreams: lose morale per rest hour by tier (1/2/3 per
        // GDD severe-debuff table). Applied AFTER regen so the net
        // effect is "rest gives you N back, nightmares take T*hours
        // away," letting morale net-decrease for low base regen rates.
        if let Some(tier) =
            se.and_then(|s| s.tier_of(StatusKind::Debuff(DebuffKind::HauntedDreams)))
        {
            let loss = (tier as i32) * ((ev.ticks / TIMESTAMP_TICKS_PER_HOUR) as i32);
            stats.morale.current = (stats.morale.current - loss).max(0);
        }

        // Kegare tilts per-school magic recovery: Kamishin restores slowly when
        // defiled, Yokaijutsu quickly. No-op when the entity isn't in the
        // kegare system or for Kiho/Onmyodo (both return 1.0).
        let kegare_mult = |school| {
            defilement
                .map(|d| crate::kegare::regen_multiplier(*d, school))
                .unwrap_or(1.0)
        };
        let kiho_gain = (stats.kiho_per_rest_hour + loc.kiho)
            * m_mult
            * hours
            * kegare_mult(MagicSchool::Kiho);
        stats.kiho.restore_to_base(kiho_gain);
        let chi_gain = (stats.onmyodo_per_rest_hour + loc.onmyodo)
            * m_mult
            * hours
            * kegare_mult(MagicSchool::Onmyodo);
        stats.onmyodo.restore_to_base(chi_gain);
        let yo_gain = (stats.yokaijutsu_per_rest_hour + loc.yokaijutsu)
            * m_mult
            * hours
            * kegare_mult(MagicSchool::Yokaijutsu);
        stats.yokaijutsu.restore_to_base(yo_gain);
        let kami_gain = (stats.kamishin_per_rest_hour + loc.kamishin)
            * m_mult
            * hours
            * kegare_mult(MagicSchool::Kamishin);
        stats.kamishin.restore_to_base(kami_gain);

        writer.write(AfterRestEvent {
            target: ev.target,
            ticks: ev.ticks,
            location: ev.location,
            cause: ev.cause.clone(),
        });
    }
}

// ---------------------------------------------------------------------------
// AP penalty system — reduces `current` after the existing refill
// ---------------------------------------------------------------------------

pub fn apply_ap_modifier_system(
    mut reader: MessageReader<crate::combat_plugin::TurnStartEvent>,
    mut q: Query<(&mut CombatStats, Option<&StatusEffects>)>,
) {
    for ev in reader.read() {
        if let Ok((mut stats, se)) = q.get_mut(ev.who) {
            // The combat plugin's on_turn_start_system sets current = base
            // earlier in the same Update; this runs after and applies the
            // modifier on top. Negative for debuffs, positive for buffs.
            let delta = ap_modifier(se);
            if delta != 0 {
                stats.action_points.current = (stats.action_points.current + delta).max(0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reactive status listeners (proof of concept for the cause-tagged event bus).
//
// These two systems demonstrate the architecture's two halves:
//  - `bleeding_aggravation_on_hit_system` reads `AfterHitEvent` and writes back
//    `DamageEvent`s with `cause = ActionCause::StatusEffect { ... }`, proving
//    that status effects can be implemented as listeners on combat events.
//    It filters out hits whose own cause is a status effect, breaking the
//    feedback loop that would otherwise occur if every status-driven hit
//    re-triggered the same status reaction.
//  - `starved_rest_interruption_system` reads `BeforeRestEvent` mutably and
//    shortens `hours` based on the target's `Starved` tier, proving that
//    status effects can mutate Before-events before the canonical resolver
//    (`rest_regen_system`) reads them.
// ---------------------------------------------------------------------------

/// On hit, if the *target* is Bleeding, queue a small extra true-damage proc
/// against them with `cause = StatusEffect`. Hits already caused by a status
/// effect are skipped, so this does not chain on itself.
pub fn bleeding_aggravation_on_hit_system(
    mut reader: MessageReader<crate::combat_plugin::AfterHitEvent>,
    mut writer: MessageWriter<crate::combat_plugin::DamageEvent>,
    status_q: Query<&StatusEffects>,
) {
    for ev in reader.read() {
        if matches!(
            ev.cause,
            crate::combat_plugin::ActionCause::StatusEffect { .. },
        ) {
            continue;
        }
        if ev.amount <= 0 {
            continue;
        }
        let Ok(se) = status_q.get(ev.target) else {
            continue;
        };
        let Some(tier) = se.tier_of(StatusKind::BadCondition(BadConditionKind::Bleeding)) else {
            continue;
        };
        writer.write(crate::combat_plugin::DamageEvent {
            attacker: ev.attacker,
            target: ev.target,
            amount: tier as i32,
            damage_type: DamageType::True,
            cause: crate::combat_plugin::ActionCause::StatusEffect { source: ev.target },
        });
    }
}

/// Hunger interrupts rest. If the resting entity is `Starved` (tier 1/2/3),
/// reduce the actual rested hours before regen sees them. Mutates
/// `BeforeRestEvent`, so it must run after `expand_rest_intent_system` and
/// before `rest_regen_system`.
pub fn starved_rest_interruption_system(
    mut events: MessageMutator<crate::combat_plugin::BeforeRestEvent>,
    status_q: Query<&StatusEffects>,
) {
    for ev in events.read() {
        let Ok(se) = status_q.get(ev.target) else {
            continue;
        };
        let Some(tier) = se.tier_of(StatusKind::Debuff(DebuffKind::Starved)) else {
            continue;
        };
        // Starvation costs `tier` hours of effective rest.
        let lost = tier as u32 * TIMESTAMP_TICKS_PER_HOUR;
        ev.ticks = ev.ticks.saturating_sub(lost);
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct StatusEffectsPlugin;

impl Plugin for StatusEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ApplyStatusEvent>()
            .add_message::<RemoveStatusEvent>()
            .add_systems(
                Update,
                (
                    apply_status_system,
                    remove_status_system,
                    status_turn_end_tick_system,
                    status_expiry_tick_system,
                    status_end_of_combat_system,
                    apply_ap_modifier_system
                        .after(crate::combat_plugin::on_turn_start_system),
                    // Reactive status: turns Bleeding into an on-hit proc by
                    // listening to AfterHitEvent. Filters its own cause to
                    // avoid feedback loops.
                    bleeding_aggravation_on_hit_system
                        .after(crate::combat_plugin::apply_damage_system),
                    // Rest pipeline: RestEvent -> BeforeRestEvent (per target,
                    // mutable; Starved listener can shorten hours) ->
                    // rest_regen_system -> AfterRestEvent.
                    starved_rest_interruption_system
                        .after(crate::combat_plugin::expand_rest_intent_system),
                    rest_regen_system
                        .after(crate::combat_plugin::expand_rest_intent_system)
                        .after(starved_rest_interruption_system),
                    // Clamp magic and morale `current` to status-derived caps
                    // each frame; cheap and avoids stale values after a cap
                    // status applies or expires.
                    recompute_resource_caps_system,
                    // Bake status modifiers into capability stats' `current`.
                    recompute_combat_capability_system,
                ),
            );
    }
}
