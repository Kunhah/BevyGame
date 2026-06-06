//! 五行 Gogyō — the Five-Phase elemental system.
//!
//! This module is the *pure* core of the elemental layer: the five phases, the
//! two interlocking cycles (相生 generating / 相剋 overcoming), yin-yang
//! polarity, and the math that turns an attacker/defender element pair into a
//! damage multiplier. It has **no Bevy dependencies and no game state** — it is
//! all value types and pure functions so it can be unit-tested in isolation and
//! reused from the combat pipeline, the AI, and the UI alike.
//!
//! The wider design (how this hooks the damage queue, the 生 support channel,
//! phase-triggered statuses, the overload/reversal mutator, and the per-school
//! "verbs") is layered on top of this core in later steps; nothing here knows
//! about any of that.
//!
//! ## The two cycles
//!
//! Both cycles run over the same fixed ordering of phases. We store the order
//! once (the `Phase` discriminants) and derive *both* cycles by index, so they
//! can never drift apart:
//!
//! ```text
//!         Wood
//!        /    \
//!    Water      Fire
//!      |    ⨯     |
//!    Metal ---- Earth
//!
//!  outer ring = 相生 (generating): Wood → Fire → Earth → Metal → Water → Wood
//!  inner star = 相剋 (overcoming): Wood → Earth → Water → Fire → Metal → Wood
//! ```

use serde::{Deserialize, Serialize};

/// The five phases (五行). The discriminant order **is** the generating ring:
/// each phase generates the next one round the circle. Do not reorder without
/// updating the cycle reasoning in [`Phase::generates`] / [`Phase::overcomes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    Wood,  // 木
    Fire,  // 火
    Earth, // 土
    Metal, // 金
    Water, // 水
}

/// Yin / Yang (陰陽). The polarity axis is orthogonal to the phase wheel: it
/// modulates how strongly a 剋 (overcoming) matchup lands and which kind of
/// status a phase tends to apply (offensive vs control), but it does not change
/// *who* beats *whom*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Polarity {
    /// 陰 — yin: receptive, control/drain-leaning.
    In,
    /// 陽 — yang: active, offence-leaning.
    Yo,
}

/// A full elemental state: one of the 5 × 2 = 10 combinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Element {
    pub phase: Phase,
    pub polarity: Polarity,
}

/// The relationship of an **attacker's** phase to a **defender's** phase. Every
/// ordered pair of phases resolves to exactly one of these five — this is the
/// whole strategic surface of the wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseRel {
    /// Identical phase.
    Same,
    /// Attacker nourishes the defender (相生, outgoing). Support synergy; no
    /// combat advantage on the 剋 damage channel.
    Generates,
    /// Defender nourishes the attacker (相生, incoming).
    GeneratedBy,
    /// Attacker overcomes the defender (相剋 advantage) — bonus damage.
    Overcomes,
    /// Defender overcomes the attacker (相剋 disadvantage) — reduced damage.
    OvercomeBy,
}

const PHASE_ORDER: [Phase; 5] =
    [Phase::Wood, Phase::Fire, Phase::Earth, Phase::Metal, Phase::Water];

impl Phase {
    /// Position of this phase on the generating ring (0..=4).
    #[inline]
    fn index(self) -> usize {
        match self {
            Phase::Wood => 0,
            Phase::Fire => 1,
            Phase::Earth => 2,
            Phase::Metal => 3,
            Phase::Water => 4,
        }
    }

    /// All five phases in canonical (generating-ring) order.
    #[inline]
    pub fn all() -> [Phase; 5] {
        PHASE_ORDER
    }

    /// 相生 — the phase this one **nourishes** (the next step round the ring):
    /// Wood→Fire→Earth→Metal→Water→Wood.
    #[inline]
    pub fn generates(self) -> Phase {
        PHASE_ORDER[(self.index() + 1) % 5]
    }

    /// 相生 (inverse) — the phase that **nourishes** this one.
    #[inline]
    pub fn generated_by(self) -> Phase {
        PHASE_ORDER[(self.index() + 4) % 5]
    }

    /// 相剋 — the phase this one **overcomes** (skip one round the ring):
    /// Wood→Earth→Water→Fire→Metal→Wood.
    #[inline]
    pub fn overcomes(self) -> Phase {
        PHASE_ORDER[(self.index() + 2) % 5]
    }

    /// 相剋 (inverse) — the phase that **overcomes** this one.
    #[inline]
    pub fn overcome_by(self) -> Phase {
        PHASE_ORDER[(self.index() + 3) % 5]
    }

    /// Resolve this phase (as attacker) against a defender's phase.
    pub fn relationship(self, defender: Phase) -> PhaseRel {
        if self == defender {
            PhaseRel::Same
        } else if self.overcomes() == defender {
            PhaseRel::Overcomes
        } else if self.overcome_by() == defender {
            PhaseRel::OvercomeBy
        } else if self.generates() == defender {
            PhaseRel::Generates
        } else {
            // The only remaining case round a 5-cycle is "defender generates me".
            debug_assert_eq!(self.generated_by(), defender);
            PhaseRel::GeneratedBy
        }
    }
}

impl Polarity {
    /// The opposing polarity.
    #[inline]
    pub fn opposite(self) -> Polarity {
        match self {
            Polarity::In => Polarity::Yo,
            Polarity::Yo => Polarity::In,
        }
    }
}

// ---------------------------------------------------------------------------
// Damage multiplier tuning
// ---------------------------------------------------------------------------
//
// These live here (next to the math that consumes them) rather than in
// `tuning.rs` for now; they can be promoted to a runtime-tunable resource once
// the system is wired into the damage pipeline (step 3) and we want to balance
// without recompiling.

/// Multiplier applied when the attacker **overcomes** the defender (剋
/// advantage), at full polarity contrast.
pub const OVERCOME_BONUS: f32 = 1.5;
/// Multiplier applied when the attacker is **overcome by** the defender (剋
/// disadvantage).
pub const OVERCOME_PENALTY: f32 = 0.66;
/// How much of the 剋 swing survives when attacker and defender share the same
/// polarity (same In/In or Yō/Yō). Opposite polarity → full swing (`1.0`).
pub const SAME_POLARITY_DAMPEN: f32 = 0.5;

/// Amplification applied to a 生 support effect (heal / buff) when the caster's
/// element **generates** the target's, at full polarity contrast.
pub const GENERATE_BONUS: f32 = 1.3;

/// The raw 剋-channel damage multiplier for an attacker `Element` striking a
/// defender `Element`, **before** any per-combatant resist is applied.
///
/// * 剋 advantage / disadvantage scale toward [`OVERCOME_BONUS`] /
///   [`OVERCOME_PENALTY`]; the distance from neutral `1.0` is reduced by
///   [`SAME_POLARITY_DAMPEN`] when the two share a polarity.
/// * `Same` and both 生 relationships are neutral (`1.0`) on this channel — the
///   生 cycle pays out on the *support* channel, not here.
pub fn damage_multiplier(attacker: Element, defender: Element) -> f32 {
    let rel = attacker.phase.relationship(defender.phase);
    let base = match rel {
        PhaseRel::Overcomes => OVERCOME_BONUS,
        PhaseRel::OvercomeBy => OVERCOME_PENALTY,
        PhaseRel::Same | PhaseRel::Generates | PhaseRel::GeneratedBy => 1.0,
    };
    if base == 1.0 {
        return 1.0;
    }
    // Soften the swing toward 1.0 when polarities match.
    let contrast = if attacker.polarity == defender.polarity {
        SAME_POLARITY_DAMPEN
    } else {
        1.0
    };
    1.0 + (base - 1.0) * contrast
}

/// The 生-channel **support** multiplier for a `caster` element empowering a
/// `target` element (heals, buffs, resource gen). Mirrors [`damage_multiplier`]
/// but on the generating cycle:
///
/// * When the caster's phase **generates** the target's, the effect scales
///   toward [`GENERATE_BONUS`], reduced by [`SAME_POLARITY_DAMPEN`] if the two
///   share a polarity.
/// * Every other relationship (including 剋 ones) is neutral (`1.0`) here — the
///   overcoming cycle pays out on the *damage* channel, not on support.
pub fn support_multiplier(caster: Element, target: Element) -> f32 {
    if caster.phase.generates() != target.phase {
        return 1.0;
    }
    let contrast = if caster.polarity == target.polarity {
        SAME_POLARITY_DAMPEN
    } else {
        1.0
    };
    1.0 + (GENERATE_BONUS - 1.0) * contrast
}

/// Whether a normally-losing matchup should **invert** (相乘 overwhelm): an
/// attacker that would be `OvercomeBy` the defender flips to `Overcomes` when
/// its elemental power exceeds the defender's by at least `threshold`.
///
/// Returns `true` only for the `OvercomeBy` case — every other relationship is
/// left untouched. This is the pure predicate; the combat layer decides what
/// "elemental power" means and supplies the two scalars.
pub fn overload_inverts(
    attacker: Phase,
    defender: Phase,
    attacker_power: f32,
    defender_power: f32,
    threshold: f32,
) -> bool {
    matches!(attacker.relationship(defender), PhaseRel::OvercomeBy)
        && attacker_power - defender_power >= threshold
}

/// [`damage_multiplier`] with the 相乘 **overload** rule folded in: when the
/// attacker would normally lose the matchup but [`overload_inverts`] fires, the
/// matchup flips and the attacker instead *overcomes* — paid out at
/// [`OVERCOME_BONUS`] (softened by [`SAME_POLARITY_DAMPEN`] on matching
/// polarity). Otherwise identical to [`damage_multiplier`].
pub fn damage_multiplier_overloaded(
    attacker: Element,
    defender: Element,
    attacker_power: f32,
    defender_power: f32,
    threshold: f32,
) -> f32 {
    if overload_inverts(attacker.phase, defender.phase, attacker_power, defender_power, threshold) {
        let contrast = if attacker.polarity == defender.polarity {
            SAME_POLARITY_DAMPEN
        } else {
            1.0
        };
        return 1.0 + (OVERCOME_BONUS - 1.0) * contrast;
    }
    damage_multiplier(attacker, defender)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Phase; 5] = PHASE_ORDER;

    #[test]
    fn generating_ring_is_a_single_5_cycle() {
        // Walking `generates` from any start visits all five and returns home.
        let mut seen = Vec::new();
        let mut p = Phase::Wood;
        for _ in 0..5 {
            seen.push(p);
            p = p.generates();
        }
        assert_eq!(p, Phase::Wood, "ring must close after 5 steps");
        for phase in ALL {
            assert!(seen.contains(&phase), "{phase:?} missing from ring");
        }
        // Canonical edges.
        assert_eq!(Phase::Wood.generates(), Phase::Fire);
        assert_eq!(Phase::Fire.generates(), Phase::Earth);
        assert_eq!(Phase::Water.generates(), Phase::Wood);
    }

    #[test]
    fn overcoming_star_is_a_single_5_cycle() {
        let mut seen = Vec::new();
        let mut p = Phase::Wood;
        for _ in 0..5 {
            seen.push(p);
            p = p.overcomes();
        }
        assert_eq!(p, Phase::Wood, "star must close after 5 steps");
        for phase in ALL {
            assert!(seen.contains(&phase), "{phase:?} missing from star");
        }
        // Canonical edges: Wood breaks Earth, Water douses Fire, Fire melts Metal.
        assert_eq!(Phase::Wood.overcomes(), Phase::Earth);
        assert_eq!(Phase::Water.overcomes(), Phase::Fire);
        assert_eq!(Phase::Fire.overcomes(), Phase::Metal);
    }

    #[test]
    fn inverses_agree_with_forward_cycles() {
        for p in ALL {
            assert_eq!(p.generates().generated_by(), p);
            assert_eq!(p.overcomes().overcome_by(), p);
            assert_eq!(p.generated_by().generates(), p);
            assert_eq!(p.overcome_by().overcomes(), p);
        }
    }

    #[test]
    fn relationship_partitions_every_pair() {
        for a in ALL {
            // Exactly one Same, one each of the four directed relations.
            let rels: Vec<PhaseRel> = ALL.iter().map(|&d| a.relationship(d)).collect();
            assert_eq!(rels.iter().filter(|r| **r == PhaseRel::Same).count(), 1);
            assert_eq!(rels.iter().filter(|r| **r == PhaseRel::Generates).count(), 1);
            assert_eq!(rels.iter().filter(|r| **r == PhaseRel::GeneratedBy).count(), 1);
            assert_eq!(rels.iter().filter(|r| **r == PhaseRel::Overcomes).count(), 1);
            assert_eq!(rels.iter().filter(|r| **r == PhaseRel::OvercomeBy).count(), 1);
        }
    }

    #[test]
    fn relationship_is_antisymmetric() {
        for a in ALL {
            for d in ALL {
                match a.relationship(d) {
                    PhaseRel::Overcomes => assert_eq!(d.relationship(a), PhaseRel::OvercomeBy),
                    PhaseRel::OvercomeBy => assert_eq!(d.relationship(a), PhaseRel::Overcomes),
                    PhaseRel::Generates => assert_eq!(d.relationship(a), PhaseRel::GeneratedBy),
                    PhaseRel::GeneratedBy => assert_eq!(d.relationship(a), PhaseRel::Generates),
                    PhaseRel::Same => assert_eq!(d.relationship(a), PhaseRel::Same),
                }
            }
        }
    }

    fn el(phase: Phase, polarity: Polarity) -> Element {
        Element { phase, polarity }
    }

    #[test]
    fn damage_multiplier_advantage_and_disadvantage() {
        // Water-yang douses Fire-yin: opposite polarity → full bonus.
        let m = damage_multiplier(el(Phase::Water, Polarity::Yo), el(Phase::Fire, Polarity::In));
        assert!((m - OVERCOME_BONUS).abs() < 1e-6);

        // Reverse is the disadvantage case, full strength.
        let m = damage_multiplier(el(Phase::Fire, Polarity::In), el(Phase::Water, Polarity::Yo));
        assert!((m - OVERCOME_PENALTY).abs() < 1e-6);
    }

    #[test]
    fn same_polarity_softens_the_swing() {
        // Same phase-matchup, same polarity → swing halved toward 1.0.
        let full =
            damage_multiplier(el(Phase::Water, Polarity::Yo), el(Phase::Fire, Polarity::In));
        let soft =
            damage_multiplier(el(Phase::Water, Polarity::Yo), el(Phase::Fire, Polarity::Yo));
        let expected = 1.0 + (full - 1.0) * SAME_POLARITY_DAMPEN;
        assert!((soft - expected).abs() < 1e-6);
        assert!(soft < full && soft > 1.0);
    }

    #[test]
    fn neutral_relationships_are_unity() {
        for pol_a in [Polarity::In, Polarity::Yo] {
            for pol_d in [Polarity::In, Polarity::Yo] {
                // Same phase.
                assert_eq!(
                    damage_multiplier(el(Phase::Fire, pol_a), el(Phase::Fire, pol_d)),
                    1.0
                );
                // Generating relationship (Wood feeds Fire) — neutral on damage.
                assert_eq!(
                    damage_multiplier(el(Phase::Wood, pol_a), el(Phase::Fire, pol_d)),
                    1.0
                );
            }
        }
    }

    #[test]
    fn support_multiplier_amplifies_only_the_generating_target() {
        // Earth generates Metal (ore born in earth). Opposite polarity → full ×1.3.
        let m = support_multiplier(
            el(Phase::Earth, Polarity::Yo),
            el(Phase::Metal, Polarity::In),
        );
        assert!((m - GENERATE_BONUS).abs() < 1e-6);

        // Same polarity softens to ×1.15.
        let soft = support_multiplier(
            el(Phase::Earth, Polarity::Yo),
            el(Phase::Metal, Polarity::Yo),
        );
        assert!((soft - (1.0 + (GENERATE_BONUS - 1.0) * SAME_POLARITY_DAMPEN)).abs() < 1e-6);
        assert!(soft < m && soft > 1.0);

        // A non-generating relationship (Earth → Fire is the wrong direction) is neutral.
        assert_eq!(
            support_multiplier(el(Phase::Earth, Polarity::Yo), el(Phase::Fire, Polarity::In)),
            1.0
        );
        // A 剋 relationship is also neutral on the support channel.
        assert_eq!(
            support_multiplier(el(Phase::Water, Polarity::Yo), el(Phase::Fire, Polarity::In)),
            1.0
        );
    }

    #[test]
    fn overload_only_inverts_losing_matchups() {
        // Fire is overcome by Water; with enough power it overwhelms (相乘).
        assert!(overload_inverts(Phase::Fire, Phase::Water, 100.0, 50.0, 30.0));
        // Not enough margin → no inversion.
        assert!(!overload_inverts(Phase::Fire, Phase::Water, 60.0, 50.0, 30.0));
        // A winning or neutral matchup never inverts regardless of power.
        assert!(!overload_inverts(Phase::Water, Phase::Fire, 999.0, 0.0, 30.0));
        assert!(!overload_inverts(Phase::Fire, Phase::Fire, 999.0, 0.0, 30.0));
    }

    #[test]
    fn overloaded_multiplier_flips_a_losing_matchup_when_powered() {
        let fire = el(Phase::Fire, Polarity::Yo);
        let water = el(Phase::Water, Polarity::In);
        // Underpowered: still the ×0.66 disadvantage (opposite polarity, full).
        let weak = damage_multiplier_overloaded(fire, water, 10.0, 10.0, 30.0);
        assert!((weak - OVERCOME_PENALTY).abs() < 1e-6);
        // Overpowered: inverts to the ×1.5 advantage.
        let strong = damage_multiplier_overloaded(fire, water, 100.0, 10.0, 30.0);
        assert!((strong - OVERCOME_BONUS).abs() < 1e-6);
        // A non-losing matchup is unaffected (Water overcomes Fire → still ×1.5).
        let normal = damage_multiplier_overloaded(water, fire, 100.0, 0.0, 30.0);
        assert!((normal - OVERCOME_BONUS).abs() < 1e-6);
    }
}
