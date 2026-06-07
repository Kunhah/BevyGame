//! Magic-source restoration activities (GDD Part 2 — "Restoration Tasks").
//!
//! Each magic school in the GDD has a list of in-fiction activities that
//! restore it: meditation for Kiho, foraging for Onmyodo, night rituals
//! for Yokaijutsu, prayer for Kamishin. This module turns that taxonomy into
//! a single event (`PerformActivityEvent`) with an enum of activity kinds, a
//! mapping from each kind to the school it restores, and a per-hour
//! restoration rate.
//!
//! Game systems (dialogue, world interactions, debug console) fire the event;
//! `apply_activity_restoration_system` consumes it and adds magic to the
//! performer's pool. Health-cost activities (`BloodPact`) also drain HP.

use bevy::prelude::*;

use crate::combat_plugin::{CombatStats, MagicSchool};

/// One specific activity a character can perform between hunts. Variants
/// match the GDD's per-school task lists at docs/gdd.md:391-450.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    // Kiho — martial focus.
    Meditation,
    BreathExercises,
    KataPractice,
    SparringDrills,
    ShrineFocus,
    // Onmyodo — earth, place-bound practice.
    NatureSpiritInteraction,
    TendSacredGrove,
    Forage,
    CraftTalisman,
    RestFertileTerrain,
    // Yokaijutsu — occult, taboo knowledge.
    NightRitual,
    SpiritOffering,
    BloodPact,
    BindingCircle,
    HauntedLocation,
    // Kamishin — divine invocation.
    Prayer,
    ShrineOffering,
    FormalRite,
    Pilgrimage,
    TempleBlessing,
    // Purification (harae / misogi) — does not restore magic; strips kegare.
    Misogi,
    Harae,
}

impl ActivityKind {
    pub fn school(self) -> MagicSchool {
        match self {
            ActivityKind::Meditation
            | ActivityKind::BreathExercises
            | ActivityKind::KataPractice
            | ActivityKind::SparringDrills
            | ActivityKind::ShrineFocus => MagicSchool::Kiho,

            ActivityKind::NatureSpiritInteraction
            | ActivityKind::TendSacredGrove
            | ActivityKind::Forage
            | ActivityKind::CraftTalisman
            | ActivityKind::RestFertileTerrain => MagicSchool::Onmyodo,

            ActivityKind::NightRitual
            | ActivityKind::SpiritOffering
            | ActivityKind::BloodPact
            | ActivityKind::BindingCircle
            | ActivityKind::HauntedLocation => MagicSchool::Yokaijutsu,

            ActivityKind::Prayer
            | ActivityKind::ShrineOffering
            | ActivityKind::FormalRite
            | ActivityKind::Pilgrimage
            | ActivityKind::TempleBlessing => MagicSchool::Kamishin,

            // Purification is the onmyoji's craft; categorize under Onmyodo
            // even though it restores no magic (see `restoration_per_hour`).
            ActivityKind::Misogi | ActivityKind::Harae => MagicSchool::Onmyodo,
        }
    }

    /// Whether this ritual can only be performed at a consecrated place (a
    /// shrine, temple, or sacred site / service NPC) rather than at a roadside
    /// camp. Place-bound rites count as resting but are never interrupted by
    /// random events; camp-able rites can be interrupted. Defaults here are a
    /// starting point — tune freely.
    pub fn requires_place(self) -> bool {
        matches!(
            self,
            ActivityKind::Harae
                | ActivityKind::Misogi
                | ActivityKind::ShrineFocus
                | ActivityKind::ShrineOffering
                | ActivityKind::FormalRite
                | ActivityKind::TempleBlessing
                | ActivityKind::Pilgrimage
                | ActivityKind::TendSacredGrove
                | ActivityKind::BindingCircle
                | ActivityKind::HauntedLocation
                | ActivityKind::RestFertileTerrain
        )
    }

    /// The purification rite this activity performs, if any. Purification
    /// activities reduce the performer's kegare instead of restoring magic.
    pub fn purification(self) -> Option<crate::kegare::Purification> {
        match self {
            ActivityKind::Misogi => Some(crate::kegare::Purification::Misogi),
            ActivityKind::Harae => Some(crate::kegare::Purification::Harae),
            _ => None,
        }
    }

    /// Magic restored per hour spent on this activity. Tuned conservatively
    /// so a long meditation session matches roughly the magnitude of the
    /// per-rest-hour passive regen, and "ritual" activities give larger
    /// bursts. These can move into a data file later.
    pub fn restoration_per_hour(self) -> f32 {
        match self {
            ActivityKind::Meditation => 1.5,
            ActivityKind::BreathExercises => 0.8,
            ActivityKind::KataPractice => 0.6,
            ActivityKind::SparringDrills => 0.4,
            ActivityKind::ShrineFocus => 1.2,

            ActivityKind::NatureSpiritInteraction => 1.4,
            ActivityKind::TendSacredGrove => 1.0,
            ActivityKind::Forage => 0.7,
            ActivityKind::CraftTalisman => 0.5,
            ActivityKind::RestFertileTerrain => 1.0,

            ActivityKind::NightRitual => 1.8,
            ActivityKind::SpiritOffering => 1.2,
            // Higher rate but pays in HP via `health_cost_per_hour`.
            ActivityKind::BloodPact => 2.5,
            ActivityKind::BindingCircle => 0.9,
            ActivityKind::HauntedLocation => 1.0,

            ActivityKind::Prayer => 1.5,
            ActivityKind::ShrineOffering => 1.2,
            ActivityKind::FormalRite => 1.4,
            ActivityKind::Pilgrimage => 2.0,
            ActivityKind::TempleBlessing => 1.6,

            // Purification restores no magic — its effect is the kegare strip.
            ActivityKind::Misogi | ActivityKind::Harae => 0.0,
        }
    }

    /// HP cost per hour for activities that demand a sacrifice (Yokaijutsu's
    /// blood-pact path). Zero for everything else.
    pub fn health_cost_per_hour(self) -> i32 {
        match self {
            ActivityKind::BloodPact => 4,
            _ => 0,
        }
    }
}

/// Fire to begin restoring magic via a school-specific activity. `hours`
/// represents in-game hours spent — the system multiplies the per-hour rate
/// by this value.
#[derive(Debug, Clone, Message)]
pub struct PerformActivityEvent {
    pub performer: Entity,
    pub activity: ActivityKind,
    pub hours: u32,
}

/// Reads `PerformActivityEvent` and applies the school's pool restoration
/// (and HP cost, where applicable). Idempotent across runs of the same event
/// because each event yields a single pool delta.
pub fn apply_activity_restoration_system(
    mut reader: MessageReader<PerformActivityEvent>,
    mut q: Query<(&mut CombatStats, Option<&crate::kegare::Defilement>)>,
    mut purify_writer: MessageWriter<crate::kegare::PurifyEvent>,
) {
    for ev in reader.read() {
        if ev.hours == 0 {
            continue;
        }

        // Purification rites strip kegare instead of restoring a magic pool.
        // Discrete by design (one rite = one strip), so `hours` only gates that
        // the rite happened, it doesn't scale the strip.
        if let Some(method) = ev.activity.purification() {
            purify_writer.write(crate::kegare::PurifyEvent {
                target: ev.performer,
                method,
            });
            info!(
                "activity: {:?} performer={:?} -> purify {:?}",
                ev.activity, ev.performer, method
            );
            continue;
        }

        let Ok((mut stats, defilement)) = q.get_mut(ev.performer) else {
            continue;
        };

        let school = ev.activity.school();
        // Kegare tilts how much the activity gives back: a defiled supplicant
        // recovers little Kamishin at a shrine; a steeped one draws Yokaijutsu
        // readily from a night rite. No-op outside the kegare system.
        let kegare_mult = defilement
            .map(|d| crate::kegare::regen_multiplier(*d, school))
            .unwrap_or(1.0);
        let gain = ev.activity.restoration_per_hour() * ev.hours as f32 * kegare_mult;
        if gain > 0.0 {
            stats.pool_mut(school).restore_to_base(gain);
        }

        let hp_cost = ev.activity.health_cost_per_hour() * ev.hours as i32;
        if hp_cost > 0 {
            stats.health.current = (stats.health.current - hp_cost).max(0);
        }

        info!(
            "activity: {:?} performer={:?} hours={} -> {:?} +{:.1} (hp cost {})",
            ev.activity, ev.performer, ev.hours, school, gain, hp_cost
        );
    }
}

pub struct ActivitiesPlugin;

impl Plugin for ActivitiesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PerformActivityEvent>()
            .add_systems(Update, apply_activity_restoration_system);
    }
}
