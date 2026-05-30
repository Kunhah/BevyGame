//! Kegare — ritual impurity as an access/contagion force, not a morality score.
//!
//! See `docs/gdd.md` (## Kegare) for the design. This module owns the first
//! slice: the per-character data model, the state derivation, and the
//! magic-school gating consumed by the combat action pipeline.
//!
//! Deliberately *not* implemented here yet (later slices, in build order):
//! purification actions, the diegetic signal layer (shader/props/audio),
//! contagion + per-zone `ZoneKegare`, and yokai-encounter weighting.
//!
//! Design invariant: kegare never makes a character weaker. It gates *which*
//! magic source a character can draw on and (later) how the world treats them.
//! Nothing here touches lethality, HP, or resource caps.

use bevy::prelude::*;

use crate::combat_ability::MagicSchool;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// Hidden per-character accumulator in `0.0..=1.0`. The player never sees this
/// value (visibility is diegetic-only); it exists purely to drive
/// [`Defilement`]. Accrual hooks (killing the undead/yokai, blighted tiles,
/// rot) and purification will write here in later slices.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component)]
pub struct Kegare {
    pub level: f32,
}

impl Kegare {
    pub const MIN: f32 = 0.0;
    pub const MAX: f32 = 1.0;

    pub fn new(level: f32) -> Self {
        Self {
            level: level.clamp(Self::MIN, Self::MAX),
        }
    }

    /// Add (or, with a negative delta, purify) and clamp to `[MIN, MAX]`.
    pub fn add(&mut self, delta: f32) {
        self.level = (self.level + delta).clamp(Self::MIN, Self::MAX);
    }
}

/// The defilement state the player actually perceives — a short ladder of named
/// states rather than a number, because a continuous value can't be read
/// diegetically. Derived from [`Kegare`] every frame by
/// [`derive_defilement_system`] with hysteresis so it doesn't flicker at a
/// threshold.
///
/// Variant order is meaningful: it defines the `Ord` used by gating
/// (`>= Defiled`, etc.). Keep Clean..Steeped in ascending severity.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[reflect(Component)]
pub enum Defilement {
    /// 清 Kiyoshi — full Kamishin; the Other is inert.
    #[default]
    Clean,
    /// 薄穢 Usugare — kegare creeping in; the Other stirs.
    Creeping,
    /// 穢 Kegare — Kamishin mostly locked; Yokaijutsu empowered.
    Defiled,
    /// 黒穢 Kurogare — Kamishin fully locked; Yokaijutsu at peak; taint spreads.
    Steeped,
}

// ---------------------------------------------------------------------------
// State derivation (with hysteresis)
// ---------------------------------------------------------------------------

// Rising thresholds: the level must reach these to step *up* a state.
const CREEPING_ENTER: f32 = 0.15;
const DEFILED_ENTER: f32 = 0.45;
const STEEPED_ENTER: f32 = 0.80;

// Falling thresholds: the level must drop *below* these to step back down.
// The gap between each EXIT and the matching ENTER is the hysteresis band, in
// which the current state is held — this is what stops single-point flicker.
const CREEPING_EXIT: f32 = 0.10;
const DEFILED_EXIT: f32 = 0.38;
const STEEPED_EXIT: f32 = 0.72;

impl Defilement {
    /// Pure derivation used by the system and the tests. Going *up* requires
    /// crossing the higher ENTER threshold; going *down* requires dropping
    /// below the lower EXIT threshold; in between, `current` is held.
    pub fn derive(level: f32, current: Defilement) -> Defilement {
        use Defilement::*;
        let l = level.clamp(Kegare::MIN, Kegare::MAX);

        let rising = if l >= STEEPED_ENTER {
            Steeped
        } else if l >= DEFILED_ENTER {
            Defiled
        } else if l >= CREEPING_ENTER {
            Creeping
        } else {
            Clean
        };

        let falling = if l >= STEEPED_EXIT {
            Steeped
        } else if l >= DEFILED_EXIT {
            Defiled
        } else if l >= CREEPING_EXIT {
            Creeping
        } else {
            Clean
        };

        if rising > current {
            rising
        } else if falling < current {
            falling
        } else {
            current
        }
    }
}

// ---------------------------------------------------------------------------
// Magic-school modulation
// ---------------------------------------------------------------------------
//
// Kegare never *blocks* a school — every ability stays castable at every state.
// It only tilts the cost/benefit, expressing pillar 1 as a soft tension rather
// than a hard lock:
//
//   - Kamishin: the kami withdraw favor as you defile. It restores more slowly
//     and costs more to channel, but is never denied.
//   - Yokaijutsu: the Other feeds on impurity. It restores faster and costs
//     less the more steeped you are (and is slightly sluggish/expensive while
//     perfectly Clean — the pure have weak ties to it — but still usable).
//   - Kiho / Onmyodo: untouched (Onmyodo is the purification lever).

/// Multiplier on how much `school` magic a character in `defilement` recovers,
/// applied to both rest regen and activity restoration. `1.0` is neutral.
pub fn regen_multiplier(defilement: Defilement, school: MagicSchool) -> f32 {
    use Defilement::*;
    match school {
        // Favor withers as defilement rises — slower to pray it back.
        MagicSchool::Kamishin => match defilement {
            Clean => 1.0,
            Creeping => 0.8,
            Defiled => 0.55,
            Steeped => 0.3,
        },
        // The Other answers the defiled more readily.
        MagicSchool::Yokaijutsu => match defilement {
            Clean => 0.85,
            Creeping => 1.0,
            Defiled => 1.25,
            Steeped => 1.5,
        },
        MagicSchool::Kiho | MagicSchool::Onmyodo => 1.0,
    }
}

/// Multiplier on the magic cost a character in `defilement` pays to cast
/// `school`. `1.0` is neutral; >1 makes it pricier, <1 cheaper.
pub fn cost_multiplier(defilement: Defilement, school: MagicSchool) -> f32 {
    use Defilement::*;
    match school {
        // Harder to channel the kami when unclean.
        MagicSchool::Kamishin => match defilement {
            Clean => 1.0,
            Creeping => 1.1,
            Defiled => 1.35,
            Steeped => 1.75,
        },
        // The Other strikes its bargains cheaply with the steeped.
        MagicSchool::Yokaijutsu => match defilement {
            Clean => 1.15,
            Creeping => 1.0,
            Defiled => 0.85,
            Steeped => 0.7,
        },
        MagicSchool::Kiho | MagicSchool::Onmyodo => 1.0,
    }
}

// ---------------------------------------------------------------------------
// Purification (harae / misogi) — the only way kegare comes back down
// ---------------------------------------------------------------------------

/// A rite of purification. Each strips a fixed amount off the [`Kegare`]
/// accumulator (subtractive, clamped at `MIN`). Two tiers for now:
///
/// - **Misogi** — water ablution / salt. Cheap and repeatable, modest strip.
///   Maps to riverside/waterfall purification and the GDD's everyday cleansing.
/// - **Harae** — a full shrine rite or Onmyodo cleansing. Costly, strong strip;
///   a single one walks a steeped character back to the lower states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purification {
    Misogi,
    Harae,
}

impl Purification {
    /// How much of the accumulator this strips. Tuned against the state
    /// thresholds: one Misogi steps Defiled down to Creeping; one Harae brings
    /// Steeped down to Creeping (or Defiled all the way to Clean).
    pub fn potency(self) -> f32 {
        match self {
            Purification::Misogi => 0.25,
            Purification::Harae => 0.60,
        }
    }
}

/// Fire to purify `target` by `method`. Applied by [`purify_system`], which
/// reduces the target's [`Kegare`]; [`derive_defilement_system`] then re-derives
/// the perceived state the same frame (the two are chained in [`KegarePlugin`]).
/// A no-op for entities that don't carry a [`Kegare`] component.
#[derive(Message, Debug, Clone)]
pub struct PurifyEvent {
    pub target: Entity,
    pub method: Purification,
}

/// Apply queued purifications: strip `method.potency()` off each target's
/// [`Kegare`]. `Kegare::add` clamps at `MIN`, so over-purifying just lands at
/// perfectly clean.
pub fn purify_system(mut reader: MessageReader<PurifyEvent>, mut q: Query<&mut Kegare>) {
    for ev in reader.read() {
        let Ok(mut kegare) = q.get_mut(ev.target) else {
            continue;
        };
        kegare.add(-ev.method.potency());
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Re-derive each kegare-bearing entity's [`Defilement`] from its [`Kegare`]
/// accumulator. Only entities that carry *both* components participate in the
/// system — this lets kegare roll out per-character without disturbing entities
/// that haven't been opted in yet.
pub fn derive_defilement_system(mut q: Query<(&Kegare, &mut Defilement)>) {
    for (kegare, mut state) in q.iter_mut() {
        let next = Defilement::derive(kegare.level, *state);
        // Guard the write so change-detection only fires on real transitions —
        // the signal layer (later) keys off `Changed<Defilement>`.
        if next != *state {
            *state = next;
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct KegarePlugin;

impl Plugin for KegarePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Kegare>()
            .register_type::<Defilement>()
            .add_message::<PurifyEvent>()
            // Purify first so the strip is visible in the derived state the
            // same frame.
            .add_systems(Update, (purify_system, derive_defilement_system).chain());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_rises_through_states() {
        use Defilement::*;
        assert_eq!(Defilement::derive(0.0, Clean), Clean);
        assert_eq!(Defilement::derive(0.15, Clean), Creeping);
        assert_eq!(Defilement::derive(0.45, Creeping), Defiled);
        assert_eq!(Defilement::derive(0.80, Defiled), Steeped);
    }

    #[test]
    fn hysteresis_holds_state_in_the_band() {
        use Defilement::*;
        // Climbed to Defiled at 0.46, then drifted down into the band.
        assert_eq!(Defilement::derive(0.46, Creeping), Defiled);
        // 0.40 is below DEFILED_ENTER (0.45) but above DEFILED_EXIT (0.38):
        // hold Defiled instead of flickering back to Creeping.
        assert_eq!(Defilement::derive(0.40, Defiled), Defiled);
        // Drop below the EXIT threshold and it finally steps down.
        assert_eq!(Defilement::derive(0.37, Defiled), Creeping);
    }

    #[test]
    fn derive_can_drop_multiple_states_at_once() {
        // A hard purification straight back to clean.
        assert_eq!(Defilement::derive(0.0, Defilement::Steeped), Defilement::Clean);
    }

    #[test]
    fn kamishin_withers_with_defilement_but_is_never_blocked() {
        use Defilement::*;
        // Regen falls monotonically, cost rises monotonically — and both stay
        // strictly positive, so the school is always castable.
        let regen: Vec<f32> = [Clean, Creeping, Defiled, Steeped]
            .iter()
            .map(|d| regen_multiplier(*d, MagicSchool::Kamishin))
            .collect();
        let cost: Vec<f32> = [Clean, Creeping, Defiled, Steeped]
            .iter()
            .map(|d| cost_multiplier(*d, MagicSchool::Kamishin))
            .collect();
        assert!(regen.windows(2).all(|w| w[0] > w[1]));
        assert!(regen.iter().all(|&m| m > 0.0));
        assert!(cost.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn yokaijutsu_strengthens_with_defilement_and_works_when_clean() {
        use Defilement::*;
        // Usable even at Clean (no zero/lock), and improves as you steep:
        // regen rises, cost falls.
        assert!(regen_multiplier(Clean, MagicSchool::Yokaijutsu) > 0.0);
        let regen: Vec<f32> = [Clean, Creeping, Defiled, Steeped]
            .iter()
            .map(|d| regen_multiplier(*d, MagicSchool::Yokaijutsu))
            .collect();
        let cost: Vec<f32> = [Clean, Creeping, Defiled, Steeped]
            .iter()
            .map(|d| cost_multiplier(*d, MagicSchool::Yokaijutsu))
            .collect();
        assert!(regen.windows(2).all(|w| w[0] < w[1]));
        assert!(cost.windows(2).all(|w| w[0] > w[1]));
    }

    #[test]
    fn purification_strips_and_clamps_at_clean() {
        // Misogi steps a Defiled-level accumulator down past the Defiled exit.
        let mut k = Kegare::new(0.45);
        k.add(-Purification::Misogi.potency()); // 0.45 - 0.25 = 0.20
        assert!((k.level - 0.20).abs() < 1e-6);
        assert_eq!(Defilement::derive(k.level, Defilement::Defiled), Defilement::Creeping);

        // Harae over-purifies a steeped character; the strip clamps at MIN.
        let mut k = Kegare::new(0.80);
        k.add(-Purification::Harae.potency()); // 0.80 - 0.60 = 0.20
        assert!((k.level - 0.20).abs() < 1e-6);

        let mut k = Kegare::new(0.10);
        k.add(-Purification::Harae.potency()); // clamps, no underflow
        assert_eq!(k.level, Kegare::MIN);
        assert_eq!(Defilement::derive(k.level, Defilement::Creeping), Defilement::Clean);
    }

    #[test]
    fn harae_is_stronger_than_misogi() {
        assert!(Purification::Harae.potency() > Purification::Misogi.potency());
    }

    #[test]
    fn kiho_and_onmyodo_are_never_modulated() {
        for d in [
            Defilement::Clean,
            Defilement::Creeping,
            Defilement::Defiled,
            Defilement::Steeped,
        ] {
            for school in [MagicSchool::Kiho, MagicSchool::Onmyodo] {
                assert_eq!(regen_multiplier(d, school), 1.0);
                assert_eq!(cost_multiplier(d, school), 1.0);
            }
        }
    }
}
