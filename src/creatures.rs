//! Data-driven creature templates + overworld behaviour.
//!
//! A *creature template* (authored in [`assets/data/creatures.ron`]) bundles
//! everything that defines one kind of world critter:
//!   * cosmetic identity (name + placeholder tint),
//!   * an overworld [`Disposition`] — the high-level "is it hostile / does it
//!     run away" knob, plus the [`BehaviorParams`] that tune the radii/speeds,
//!   * the combat personality ([`AIParameters`]) handed to the battle
//!     behaviour-tree evaluator, and
//!   * an optional [`YokaiKind`] whose stat block / abilities / BT profile the
//!     creature fights with (templates without one fall back to the generic
//!     enemy stat block).
//!
//! The per-spawn encounter id (used for quest/hunt matching) is intentionally
//! kept out of the template and passed to [`spawn_creature`] at placement time,
//! since the same species can appear in many distinct encounters.
//!
//! At runtime each spawned creature carries a [`Creature`] component holding
//! its home anchor and a small state machine. [`drive_creatures`] ticks that
//! machine every frame while exploring: Hostile creatures chase and start a
//! fight on contact, Territorial ones guard a home and leash back, Skittish
//! ones flee, Passive ones wander, and Friendly ones hold their ground.

use std::collections::HashMap;
use std::fs;

use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::battle::{
    start_battle, BattleState, EnemyEncounter, WorldAlly, WorldYokai, YokaiKind,
};
use crate::combat_plugin::{AIParameters, TurnManager, TurnOrder};
use crate::constants::PLAYER_SPEED;
use crate::core::{GameState, Game_State, Player};
use crate::render3d::PlaceholderVisual;

const CREATURE_CATALOG_PATH: &str = "assets/data/creatures.ron";

// ---------------------------------------------------------------------------
// Template data
// ---------------------------------------------------------------------------

/// How a creature reacts to the player in the overworld. This is the single
/// most important behaviour knob — everything else just tunes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Disposition {
    /// Attacks on sight: chases the player as soon as they enter detection
    /// range and starts a battle on contact.
    Hostile,
    /// Peaceful until disturbed. Guards a home area and chases intruders who
    /// come close, but gives up and walks home if lured past its leash.
    Territorial,
    /// Never fights. Flees directly away from the player whenever they get
    /// within detection range.
    Skittish,
    /// Ignores the player entirely and wanders aimlessly around its home.
    Passive,
    /// Stands its ground. Friendly — never chases, never flees.
    Friendly,
}

impl Disposition {
    /// Whether this disposition can ever initiate (and therefore needs an
    /// [`EnemyEncounter`] tag so a battle can start).
    pub fn is_aggressive(self) -> bool {
        matches!(self, Disposition::Hostile | Disposition::Territorial)
    }
}

/// Numeric tuning for the overworld state machine. Distances are world units
/// (the old sprite grid is 32 units per tile); speeds are units/second.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BehaviorParams {
    /// Range at which the creature notices the player.
    pub detection_radius: f32,
    /// Range at which a Hostile/Territorial creature actually starts the fight.
    pub engage_radius: f32,
    /// Movement speed while chasing or fleeing.
    pub move_speed: f32,
    /// How far a Territorial creature will stray from home before giving up and
    /// returning (its leash). Also caps how far Passive creatures wander.
    pub leash_radius: f32,
    /// Radius of the wander box around home for Passive creatures.
    pub wander_radius: f32,
}

impl Default for BehaviorParams {
    fn default() -> Self {
        Self {
            detection_radius: 256.0,
            engage_radius: 36.0,
            move_speed: PLAYER_SPEED * 0.55,
            leash_radius: 384.0,
            wander_radius: 96.0,
        }
    }
}

/// One authored creature kind. Deserialised from `creatures.ron`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatureTemplate {
    /// Display name (logs / future UI).
    pub name: String,
    /// Placeholder capsule tint, linear sRGB 0..=1 per channel.
    pub color: [f32; 3],
    /// Overworld reaction to the player.
    pub disposition: Disposition,
    /// Radii / speeds tuning the overworld behaviour.
    #[serde(default)]
    pub behavior: BehaviorParams,
    /// Combat personality consumed by the behaviour-tree evaluator in battle.
    #[serde(default)]
    pub ai: AIParameters,
    /// Optional species whose battle stats / abilities / BT profile this
    /// creature fights with. `None` → generic enemy block. This is the
    /// creature's *combat identity* and is intrinsic to the species; the
    /// per-spawn encounter id (for quest/hunt matching) is supplied separately
    /// at [`spawn_creature`] time, not baked into the template.
    #[serde(default)]
    pub yokai: Option<YokaiKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreatureCatalogData {
    pub templates: HashMap<String, CreatureTemplate>,
}

#[derive(Resource, Default)]
pub struct CreatureCatalog(pub CreatureCatalogData);

// ---------------------------------------------------------------------------
// Runtime component + state machine
// ---------------------------------------------------------------------------

/// Live state attached to every spawned creature in the world.
#[derive(Component, Debug, Clone)]
pub struct Creature {
    /// Key into [`CreatureCatalog`].
    pub template: String,
    /// Spawn point, used as the leash / wander anchor.
    pub home: Vec2,
    /// Current overworld behaviour state.
    pub state: CreatureState,
    /// Active wander destination (Passive only).
    pub wander_target: Option<Vec2>,
    /// Seconds until the creature picks a new wander destination.
    pub wander_cooldown: f32,
}

impl Creature {
    pub fn new(template: impl Into<String>, home: Vec2) -> Self {
        Self {
            template: template.into(),
            home,
            state: CreatureState::Idle,
            wander_target: None,
            wander_cooldown: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatureState {
    /// Standing still (Friendly, or aware-but-out-of-range).
    Idle,
    /// Drifting toward a wander target (Passive).
    Wander,
    /// Closing on the player (Hostile / Territorial).
    Chase,
    /// Running away from the player (Skittish).
    Flee,
    /// Walking back toward home after losing interest (Territorial leash).
    Returning,
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// Spawn a creature from a template id at `pos` (world translation). Returns
/// `None` if the template id is unknown.
///
/// `encounter_id` is the *placement-time* identity used for quest/hunt
/// matching (see [`crate::quests`] / [`crate::contract`]) and for
/// `BattleState::enemy_id`; it is deliberately NOT part of the template, since
/// the same species can appear in many different encounters. Pass `Some(id)`
/// for a creature a quest or hunt cares about; `None` for ambient critters
/// (they can still fight if aggressive — the battle just isn't tagged).
///
/// The combat stat block / abilities come from the template's `yokai` species
/// (its combat identity); a [`WorldYokai`] tag is attached whenever one is set.
pub fn spawn_creature(
    commands: &mut Commands,
    catalog: &CreatureCatalog,
    template_id: &str,
    pos: Vec3,
    encounter_id: Option<u32>,
) -> Option<Entity> {
    let tmpl = catalog.0.templates.get(template_id)?;
    let color = Color::srgb(tmpl.color[0], tmpl.color[1], tmpl.color[2]);

    let mut e = commands.spawn((
        PlaceholderVisual::character(color),
        Transform::from_translation(pos),
        Creature::new(template_id, pos.truncate()),
        tmpl.ai,
        crate::world::VisualOcclusionTarget,
        crate::world::YSort { base_z: 0.0 },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
        Name::new(format!("Creature({})", tmpl.name)),
    ));

    // Combat identity is intrinsic to the species.
    if let Some(kind) = tmpl.yokai {
        e.insert(WorldYokai { kind });
    }
    // Encounter identity is a per-spawn concern, decoupled from the template.
    if let Some(id) = encounter_id {
        e.insert(EnemyEncounter { id });
    }
    Some(e.id())
}

// ---------------------------------------------------------------------------
// Asset loading
// ---------------------------------------------------------------------------

fn load_creature_catalog(mut catalog: ResMut<CreatureCatalog>) {
    match fs::read_to_string(CREATURE_CATALOG_PATH) {
        Ok(text) => match ron::de::from_str::<CreatureCatalogData>(&text) {
            Ok(data) => {
                let count = data.templates.len();
                catalog.0 = data;
                info!("Loaded {count} creature template(s) from {CREATURE_CATALOG_PATH}");
            }
            Err(err) => warn!("Failed to parse {CREATURE_CATALOG_PATH}: {err}"),
        },
        Err(err) => warn!("Failed to read {CREATURE_CATALOG_PATH}: {err}"),
    }
}

// ---------------------------------------------------------------------------
// Overworld behaviour driver
// ---------------------------------------------------------------------------

/// Tick every creature's overworld state machine. Runs only while exploring;
/// hostile contact hands off to the existing [`start_battle`] flow.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn drive_creatures(
    mut commands: Commands,
    time: Res<Time>,
    catalog: Res<CreatureCatalog>,
    mut game_state: ResMut<GameState>,
    mut battle_state: ResMut<BattleState>,
    mut tm: ResMut<TurnManager>,
    mut turn_order: ResMut<TurnOrder>,
    player_q: Query<
        (Entity, &Transform, Option<&crate::characters::CharacterKind>),
        (With<Player>, Without<Creature>),
    >,
    ally_q: Query<
        (Entity, &Transform, Option<&crate::characters::CharacterKind>),
        (With<WorldAlly>, Without<Creature>),
    >,
    mut creature_q: Query<
        (Entity, &mut Transform, &mut Creature, Option<&EnemyEncounter>, Option<&WorldYokai>),
        (Without<Player>, Without<WorldAlly>),
    >,
) {
    if game_state.0 != Game_State::Exploring || battle_state.active {
        return;
    }
    let Ok((player_entity, player_tf, player_kind)) = player_q.single() else {
        return;
    };
    let player_kind = player_kind.copied();
    let player_pos = player_tf.translation.truncate();
    let dt = time.delta_secs();
    let mut rng = rand::rng();

    // Collected once so the battle hand-off can pass the ally roster.
    let allies: Vec<(Entity, Transform, Option<crate::characters::CharacterKind>)> =
        ally_q.iter().map(|(e, t, k)| (e, *t, k.copied())).collect();

    for (entity, mut transform, mut creature, encounter, yokai) in creature_q.iter_mut() {
        let Some(tmpl) = catalog.0.templates.get(&creature.template) else {
            continue;
        };
        let params = tmpl.behavior;
        let pos = transform.translation.truncate();
        let to_player = player_pos - pos;
        let dist = to_player.length();

        match tmpl.disposition {
            Disposition::Hostile => {
                if dist <= params.detection_radius {
                    creature.state = CreatureState::Chase;
                    if dist <= params.engage_radius {
                        // Reached the player — start the fight and stop.
                        game_state.0 = Game_State::Battle;
                        start_battle(
                            &mut commands,
                            &mut battle_state,
                            &mut tm,
                            &mut turn_order,
                            // Untagged (id 0) when this spawn carries no
                            // encounter identity — it still fights, just isn't
                            // matched by any quest/hunt.
                            encounter.map(|e| e.id).unwrap_or(0),
                            None,
                            None,
                            yokai.map(|y| y.kind).or(tmpl.yokai),
                            entity,
                            player_entity,
                            player_tf.translation,
                            transform.translation,
                            allies.clone(),
                            player_kind,
                        );
                        return;
                    }
                    step_toward(&mut transform, to_player, params.move_speed, dt);
                } else {
                    creature.state = CreatureState::Idle;
                }
            }
            Disposition::Territorial => {
                let from_home = pos - creature.home;
                let leashed = from_home.length() >= params.leash_radius;
                let player_in_range = dist <= params.detection_radius;
                if creature.state == CreatureState::Returning {
                    // Keep walking home until we arrive, then re-evaluate.
                    if from_home.length() <= params.engage_radius {
                        creature.state = CreatureState::Idle;
                    } else {
                        step_toward(&mut transform, creature.home - pos, params.move_speed, dt);
                    }
                } else if player_in_range && !leashed {
                    creature.state = CreatureState::Chase;
                    if dist <= params.engage_radius {
                        game_state.0 = Game_State::Battle;
                        start_battle(
                            &mut commands,
                            &mut battle_state,
                            &mut tm,
                            &mut turn_order,
                            // Untagged (id 0) when this spawn carries no
                            // encounter identity — it still fights, just isn't
                            // matched by any quest/hunt.
                            encounter.map(|e| e.id).unwrap_or(0),
                            None,
                            None,
                            yokai.map(|y| y.kind).or(tmpl.yokai),
                            entity,
                            player_entity,
                            player_tf.translation,
                            transform.translation,
                            allies.clone(),
                            player_kind,
                        );
                        return;
                    }
                    step_toward(&mut transform, to_player, params.move_speed, dt);
                } else if leashed {
                    creature.state = CreatureState::Returning;
                    step_toward(&mut transform, creature.home - pos, params.move_speed, dt);
                } else {
                    creature.state = CreatureState::Idle;
                }
            }
            Disposition::Skittish => {
                if dist <= params.detection_radius && dist > f32::EPSILON {
                    creature.state = CreatureState::Flee;
                    // Move directly away from the player.
                    step_toward(&mut transform, -to_player, params.move_speed, dt);
                } else {
                    creature.state = CreatureState::Idle;
                }
            }
            Disposition::Passive => {
                creature.wander_cooldown -= dt;
                let target = match creature.wander_target {
                    Some(t) if creature.wander_cooldown > 0.0 && pos.distance(t) > 6.0 => t,
                    _ => {
                        // Pick a fresh point inside the wander box around home.
                        let angle = rng.random_range(0.0..std::f32::consts::TAU);
                        let radius = rng.random_range(0.0..params.wander_radius);
                        let t = creature.home + Vec2::from_angle(angle) * radius;
                        creature.wander_target = Some(t);
                        creature.wander_cooldown = rng.random_range(1.5..4.0);
                        t
                    }
                };
                creature.state = CreatureState::Wander;
                // Wander at a relaxed pace (half the chase/flee speed).
                step_toward(&mut transform, target - pos, params.move_speed * 0.5, dt);
            }
            Disposition::Friendly => {
                creature.state = CreatureState::Idle;
            }
        }
    }
}

/// Move `transform` along `dir` (need not be normalised) by `speed * dt`,
/// without overshooting when `dir` is shorter than the step. Preserves Z.
fn step_toward(transform: &mut Transform, dir: Vec2, speed: f32, dt: f32) {
    let len = dir.length();
    if len <= f32::EPSILON {
        return;
    }
    let step = (speed * dt).min(len);
    let delta = dir / len * step;
    transform.translation.x += delta.x;
    transform.translation.y += delta.y;
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct CreaturesPlugin;

impl Plugin for CreaturesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatureCatalog>()
            // PreStartup so the catalog is populated before `world::setup`
            // (a `Startup` system) tries to spawn demo creatures from it.
            .add_systems(PreStartup, load_creature_catalog)
            // Drive creatures after the player moves so chase/flee react to the
            // current frame's player position.
            .add_systems(Update, drive_creatures.after(crate::movement::player_movement));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped catalog must round-trip through serde or no creature
    /// templates load.
    #[test]
    fn shipped_catalog_parses() {
        let text = std::fs::read_to_string(CREATURE_CATALOG_PATH)
            .expect("creatures.ron exists at the documented path");
        let parsed: CreatureCatalogData =
            ron::de::from_str(&text).expect("creatures.ron deserialises");
        assert!(
            !parsed.templates.is_empty(),
            "expected at least one creature template",
        );
    }
}
