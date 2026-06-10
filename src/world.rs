use bevy::prelude::*;

use crate::battle::{
    CombatMovePoints, EnemyEncounter, FinalBoss, WorldAlly, WorldNpc, WorldYokai, YokaiKind,
    FINAL_BOSS_ENCOUNTER_ID,
};
use crate::city_data::CityCatalog;
use crate::combat_plugin::{
    AwaitingResurrection, Bound, Dead, ResurrectionPoint, ResurrectionStanding,
};
use crate::characters::{CharacterKind, SelectedParty};
use crate::skill_tree::PartyProgression;
use crate::core::{GameState, Game_State, MainCamera, Player, Timestamp};
use crate::dialogue::{CachedInteractables, Interactable};
use crate::economy::{MerchantNpc, Merchants};
use crate::governance::GovernorNpc;
use crate::light_plugin::Occluder;
use crate::map::{tile_center_world, MapTiles, PLAYER_SPAWN_TILE, TILE_WORLD_SIZE};
use crate::quadtree::{Collider, QuadTree, QuadtreeNode};
use crate::render3d::{spawn_iso_camera, spawn_sun, PlaceholderAssets, PlaceholderVisual};
use crate::services::{ServiceKind, ServiceNpc};

#[derive(Component, Clone, Copy, Debug)]
pub struct YSort {
    pub base_z: f32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct VisualOccluder {
    pub size: Vec2,
    pub fade_alpha: f32,
    pub solid_alpha: f32,
}

#[derive(Component)]
pub struct FadeWhenCovered;

#[derive(Component)]
pub struct VisualOcclusionTarget;

pub fn setup(
    mut commands: Commands,
    map: Res<MapTiles>,
    cities: Res<CityCatalog>,
    merchants: Res<Merchants>,
    query: Query<&Collider>,
    creatures: Res<crate::creatures::CreatureCatalog>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // The party spawns at the centre of `PLAYER_SPAWN_TILE`, kept inside the
    // impassable map border so every direction has a loaded neighbour. All the
    // hardcoded test entities below were authored relative to a 512-unit world,
    // so we offset them by the same origin so they remain visible near the
    // player as `TILE_WORLD_SIZE` grows.
    let world_origin = tile_center_world(PLAYER_SPAWN_TILE);
    let origin3 = world_origin.extend(0.0);

    // Shared placeholder meshes/materials for spawn systems outside `setup`.
    commands.insert_resource(PlaceholderAssets::build(&mut meshes, &mut materials));

    let mut quadtree = QuadtreeNode::new(Rect::from_center_size(Vec2::ZERO, Vec2::splat(2048.0)), 0);

    for collider in &query {
        quadtree.insert(collider.clone());
    }

    commands.insert_resource(QuadTree(quadtree));

    // Isometric 3D camera (XY ground, +Z up) focused on the player spawn, plus
    // a shadow-casting sun and an ambient fill so the dark scene reads.
    let camera = spawn_iso_camera(&mut commands, origin3);
    // `IsDefaultUiCamera` so all untargeted gameplay UI (HUD, pause, party
    // select) renders here. The main menu lives on its own 2D camera and
    // targets it explicitly via `UiTargetCamera`, so the two never collide.
    commands
        .entity(camera)
        .insert((MainCamera, IsDefaultUiCamera));
    spawn_sun(&mut commands);

    // The player + party are NOT spawned here. `setup` runs at `Startup`,
    // before the player has chosen their roster on the party-selection screen,
    // so the avatar + companions are spawned later by `spawn_party` (below) the
    // first time a gameplay state is entered, reading the `SelectedParty`.

    // Shrine: the player respawns at this location after the resurrection
    // delay elapses. Spawned once at world setup; the
    // `teleport_on_resurrection` system snaps any resurrected entity to the
    // closest one.
    commands.spawn((
        PlaceholderVisual::prop(Color::srgb(0.55, 0.75, 0.95), Vec2::splat(40.0), 40.0),
        Transform::from_translation(origin3 + Vec3::new(-12.0 * 32.0, 4.0 * 32.0, 0.0)),
        ResurrectionPoint,
        // Doubles as the revive sanctuary: standing here completes any pending
        // resurrection for downed party members (see `revive_shrine_system`).
        ReviveShrine,
        YSort { base_z: 0.0 },
    ));

    commands.spawn((
        PlaceholderVisual::character(Color::srgb(0.3, 0.9, 0.3)),
        Transform::from_translation(origin3 + Vec3::new(4.0 * 32.0, 0.0, 0.0)),
        EnemyEncounter { id: 1 },
        VisualOcclusionTarget,
        YSort { base_z: 0.0 },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
    ));

    // Yokai scattered around the player's spawn so the battle loop is
    // exercisable. Each carries `WorldYokai { kind }`, which routes
    // `start_battle` to `spawn_yokai_combatant` (per-species stats, BT
    // profile, and ability set).
    let yokai_seedlings: [(YokaiKind, Vec3, Color); 4] = [
        // Onibi — fast, fragile fire wisp. Tinted hot orange.
        (
            YokaiKind::Onibi,
            Vec3::new(7.0 * 32.0, 2.0 * 32.0, 0.0),
            Color::srgb(0.95, 0.55, 0.20),
        ),
        // Kappa — sturdy river demon. Algae-green.
        (
            YokaiKind::Kappa,
            Vec3::new(-5.0 * 32.0, 4.0 * 32.0, 0.0),
            Color::srgb(0.30, 0.65, 0.45),
        ),
        // Kasha — mental caster. Ashen purple.
        (
            YokaiKind::Kasha,
            Vec3::new(2.0 * 32.0, -6.0 * 32.0, 0.0),
            Color::srgb(0.55, 0.30, 0.65),
        ),
        // Second Onibi to test "two enemies of the same kind in different cells".
        (
            YokaiKind::Onibi,
            Vec3::new(-8.0 * 32.0, -3.0 * 32.0, 0.0),
            Color::srgb(0.95, 0.55, 0.20),
        ),
    ];
    for (i, (kind, pos, color)) in yokai_seedlings.into_iter().enumerate() {
        commands.spawn((
            PlaceholderVisual::character(color),
            Transform::from_translation(origin3 + pos),
            // EnemyEncounter ids start at 100 to avoid colliding with the
            // generic id-1 enemy above and the city-governor encounters
            // (10_000+) further down.
            EnemyEncounter { id: 100 + i as u32 },
            WorldYokai { kind },
            VisualOcclusionTarget,
            YSort { base_z: 0.0 },
            crate::light_plugin::LightSensitive { threshold: 0.15 },
            Name::new(format!("WorldYokai({} #{})", kind.label(), i)),
        ));
    }

    // Data-driven creatures (see `assets/data/creatures.ron`). Each carries a
    // disposition that `crate::creatures::drive_creatures` reads every frame:
    // hostile ones chase + start fights, territorial ones guard a home, the
    // skittish hare flees, the tanuki wanders, the shrine fox stands guard.
    // The encounter id is supplied here at placement time (not in the
    // template): the aggressive ones get ids in the 300+ range for quest/hunt
    // matching, the ambient critters get `None`.
    let creature_seedlings: [(&str, Vec3, Option<u32>); 6] = [
        ("wild_onibi", Vec3::new(9.0 * 32.0, -2.0 * 32.0, 0.0), Some(300)),
        ("river_kappa", Vec3::new(-7.0 * 32.0, 7.0 * 32.0, 0.0), Some(301)),
        ("kasha_stalker", Vec3::new(5.0 * 32.0, 8.0 * 32.0, 0.0), Some(302)),
        ("skittish_hare", Vec3::new(3.0 * 32.0, 5.0 * 32.0, 0.0), None),
        ("wandering_tanuki", Vec3::new(-3.0 * 32.0, -7.0 * 32.0, 0.0), None),
        ("shrine_fox", Vec3::new(-11.0 * 32.0, 5.0 * 32.0, 0.0), None),
    ];
    for (template_id, offset, encounter_id) in creature_seedlings {
        if crate::creatures::spawn_creature(
            &mut commands,
            &creatures,
            template_id,
            origin3 + offset,
            encounter_id,
        )
        .is_none()
        {
            warn!("creature template '{template_id}' not found in catalog");
        }
    }

    for merchant in merchants.0.values() {
        if let Some(spawn_pos) = first_tile_world_position_for_region(&map, merchant.region_id) {
            commands.spawn((
                PlaceholderVisual::character(Color::srgb(0.2, 0.6, 0.9)),
                Transform::from_translation(spawn_pos),
                WorldNpc {
                    id: merchant.id as u32,
                },
                VisualOcclusionTarget,
                YSort { base_z: 0.0 },
                MerchantNpc {
                    merchant_id: merchant.id,
                },
                Interactable {
                    name: format!("Merchant {}", merchant.name),
                    dialogue_id: "The last goodbye 1".to_string(),
                },
                crate::light_plugin::LightSensitive { threshold: 0.15 },
                Name::new(format!("MerchantNPC({})", merchant.name)),
            ));
        } else {
            warn!(
                "No tile found for merchant {} in region {}",
                merchant.name, merchant.region_id
            );
        }
    }

    for city in cities.0.values() {
        let Some(&region_id) = city.region_ids.first() else {
            continue;
        };
        if let Some(mut spawn_pos) = first_tile_world_position_for_region(&map, region_id) {
            spawn_pos.x += TILE_WORLD_SIZE * 0.75;
            commands.spawn((
                PlaceholderVisual::character(Color::srgb(0.85, 0.2, 0.2)),
                Transform::from_translation(spawn_pos),
                WorldNpc {
                    id: 10_000 + city.id as u32,
                },
                EnemyEncounter {
                    id: 10_000 + city.id as u32,
                },
                VisualOcclusionTarget,
                YSort { base_z: 0.0 },
                GovernorNpc { city_id: city.id },
                Interactable {
                    name: format!("Governor {} {}", city.governor_title, city.governor_name),
                    dialogue_id: "The last goodbye 1".to_string(),
                },
                crate::light_plugin::LightSensitive { threshold: 0.15 },
                Name::new(format!("GovernorNPC({})", city.name)),
            ));

            let service_defs = [
                (
                    ServiceKind::Inn,
                    Color::srgb(0.90, 0.76, 0.42),
                    Vec2::new(TILE_WORLD_SIZE * 1.5, TILE_WORLD_SIZE * 0.1),
                    "Innkeeper",
                ),
                (
                    ServiceKind::Transport,
                    Color::srgb(0.35, 0.80, 0.92),
                    Vec2::new(TILE_WORLD_SIZE * 1.8, TILE_WORLD_SIZE * 0.8),
                    "Transport Master",
                ),
                (
                    ServiceKind::CraftingHall,
                    Color::srgb(0.88, 0.42, 0.18),
                    Vec2::new(TILE_WORLD_SIZE * 2.2, TILE_WORLD_SIZE * 0.4),
                    "Craft Hall Steward",
                ),
                (
                    ServiceKind::CastleGate,
                    Color::srgb(0.75, 0.75, 0.75),
                    Vec2::new(TILE_WORLD_SIZE * 2.6, TILE_WORLD_SIZE * 0.7),
                    "Castle Gate Guard",
                ),
                (
                    ServiceKind::Shrine,
                    Color::srgb(0.92, 0.86, 0.55),
                    Vec2::new(TILE_WORLD_SIZE * 1.2, TILE_WORLD_SIZE * 1.3),
                    "Shrine Maiden",
                ),
            ];
            for (kind, color, offset, label) in service_defs {
                commands.spawn((
                    PlaceholderVisual::character(color),
                    Transform::from_xyz(spawn_pos.x + offset.x, spawn_pos.y + offset.y, 0.0),
                    WorldNpc {
                        id: 30_000 + city.id as u32 * 10 + kind as u32,
                    },
                    VisualOcclusionTarget,
                    YSort { base_z: 0.0 },
                    ServiceNpc {
                        city_id: city.id,
                        region_id,
                        kind,
                    },
                    Interactable {
                        name: format!("{} of {}", label, city.name),
                        dialogue_id: "The last goodbye 1".to_string(),
                    },
                    crate::light_plugin::LightSensitive { threshold: 0.15 },
                    Name::new(format!("{}({})", label.replace(' ', ""), city.name)),
                ));
            }
        }
    }

    // (Party companions are spawned by `spawn_party`, driven by `SelectedParty`.)

    let test_obstacles = [
        (Vec3::new(2.0 * 32.0, -3.0 * 32.0, 0.0), Color::srgb(0.55, 0.55, 0.55)),
        (Vec3::new(-4.0 * 32.0, 1.0 * 32.0, 0.0), Color::srgb(0.45, 0.40, 0.35)),
    ];
    for (i, (offset, color)) in test_obstacles.into_iter().enumerate() {
        let world_pos = origin3 + offset;
        let size = Vec2::splat(48.0);
        commands.spawn((
            PlaceholderVisual::prop(color, size, 48.0),
            Transform::from_translation(world_pos),
            Collider {
                bounds: Rect::from_center_size(world_pos.truncate(), size),
            },
            Occluder::new(size),
            YSort { base_z: 0.0 },
            Name::new(format!("TestObstacle{}", i)),
        ));
    }

    let tower_base = origin3 + Vec3::new(6.0 * 32.0, 5.0 * 32.0, 0.0);
    commands.spawn((
        PlaceholderVisual::prop(Color::srgb(0.62, 0.42, 0.22), Vec2::splat(96.0), 192.0),
        Transform::from_translation(tower_base),
        Collider {
            bounds: Rect::from_center_size(
                Vec2::new(tower_base.x, tower_base.y + 10.0),
                Vec2::new(36.0, 20.0),
            ),
        },
        // Shadow footprint is the tower's base — same rectangle as the
        // collider — so the cast shadow lines up with the trunk on the
        // ground rather than the much-taller visible mesh.
        Occluder::with_offset(Vec2::new(36.0, 20.0), Vec2::new(0.0, 10.0)),
        FadeWhenCovered,
        VisualOccluder {
            size: Vec2::new(120.0, 180.0),
            fade_alpha: 0.35,
            solid_alpha: 1.0,
        },
        YSort { base_z: 0.0 },
        Name::new("Tower"),
    ));

    let test_interactables = [
        (
            Vec3::new(1.0 * 32.0, 5.0 * 32.0, 0.0),
            Color::srgb(0.2, 0.6, 0.9),
            "Villager A",
            "The last goodbye 1",
        ),
        (
            Vec3::new(2.0 * 32.0, 5.0 * 32.0, 0.0),
            Color::srgb(0.9, 0.4, 0.7),
            "Villager B",
            "Interactable 1",
        ),
    ];
    for (offset, color, label, dialogue_id) in test_interactables {
        commands.spawn((
            PlaceholderVisual::character(color),
            Transform::from_translation(origin3 + offset),
            Interactable {
                name: label.to_string(),
                dialogue_id: dialogue_id.to_string(),
            },
            VisualOcclusionTarget,
            YSort { base_z: 0.0 },
            crate::light_plugin::LightSensitive { threshold: 0.15 },
            Name::new(label.to_string()),
        ));
    }

    // Extra enemies for testing (Space engages an adjacent one). A mix of
    // generic encounters and a couple of yokai (which spawn species-specific
    // combatants). Ids 200+ avoid the existing id-1 / 100+ / 10_000+ ranges.
    let extra_enemies: [(Vec3, Color, u32, Option<YokaiKind>); 5] = [
        (Vec3::new(6.0 * 32.0, 6.0 * 32.0, 0.0), Color::srgb(0.85, 0.25, 0.25), 200, None),
        (Vec3::new(8.0 * 32.0, -2.0 * 32.0, 0.0), Color::srgb(0.80, 0.30, 0.30), 201, None),
        (Vec3::new(-6.0 * 32.0, 7.0 * 32.0, 0.0), Color::srgb(0.90, 0.35, 0.20), 202, Some(YokaiKind::Onibi)),
        (Vec3::new(-9.0 * 32.0, -5.0 * 32.0, 0.0), Color::srgb(0.30, 0.60, 0.45), 203, Some(YokaiKind::Kappa)),
        (Vec3::new(10.0 * 32.0, 3.0 * 32.0, 0.0), Color::srgb(0.55, 0.30, 0.65), 204, Some(YokaiKind::Kasha)),
    ];
    for (offset, color, id, kind) in extra_enemies {
        let mut e = commands.spawn((
            PlaceholderVisual::character(color),
            Transform::from_translation(origin3 + offset),
            EnemyEncounter { id },
            VisualOcclusionTarget,
            YSort { base_z: 0.0 },
            crate::light_plugin::LightSensitive { threshold: 0.15 },
            Name::new(format!("TestEnemy{id}")),
        ));
        if let Some(kind) = kind {
            e.insert(WorldYokai { kind });
        }
    }

    // Dedicated toon-shader reference: a big isolated upright capsule north of
    // spawn so the cel bands + rim light (and the capsule orientation) are easy
    // to see. The ISO_SHOT dev capture aims the camera here.
    commands.spawn((
        PlaceholderVisual::prop(Color::srgb(0.72, 0.52, 0.50), Vec2::splat(70.0), 180.0).toon(),
        Transform::from_translation((world_origin + Vec2::new(0.0, 800.0)).extend(0.0)),
        Name::new("ToonTestCapsule"),
    ));

    // --- Main-quest spine -------------------------------------------------
    // The opening beat: a village elder beside the spawn who charges the party
    // with the run's goal. Talking to them (walk up, press E) plays
    // `seirei_intro`, whose final node "elder_charge" satisfies objective 1 of
    // the main quest. Placed just north-east of the party's start tile.
    commands.spawn((
        PlaceholderVisual::character(Color::srgb(0.86, 0.82, 0.55)),
        Transform::from_translation(origin3 + Vec3::new(1.0 * 32.0, 2.0 * 32.0, 0.0)),
        VisualOcclusionTarget,
        YSort { base_z: 0.0 },
        Interactable {
            name: "Village Elder".to_string(),
            dialogue_id: "seirei_intro".to_string(),
        },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
        Name::new("VillageElder"),
    ));

    // The final boss: the Gashadokuro at the defiled shrine, far to the east of
    // the village. Walk adjacent and press Space to engage. Felling it carries
    // `FinalBoss` into combat, so `end_battle_on_death` wins the run, and its
    // `EnemyEncounter` id closes the main quest's kill objective.
    commands.spawn((
        PlaceholderVisual::character(Color::srgb(0.93, 0.92, 0.86)),
        Transform::from_translation(origin3 + Vec3::new(28.0 * 32.0, 6.0 * 32.0, 0.0)),
        EnemyEncounter {
            id: FINAL_BOSS_ENCOUNTER_ID,
        },
        FinalBoss,
        VisualOcclusionTarget,
        YSort { base_z: 0.0 },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
        Name::new("Gashadokuro"),
    ));

    // A walk-in building (open-topped room with a doorway) for testing how
    // walls occlude a character standing inside, especially while rotating the
    // camera (Q/E). Walls carry colliders; the front doorway is the way in.
    spawn_house(&mut commands, world_origin + Vec2::new(9.0 * 32.0, -9.0 * 32.0));
}

/// Spawn a simple placeholder building: four tall walls forming a room with a
/// gap in the front (low-Y) wall for a door. Each wall is a collider so the
/// player is blocked except through the doorway. Open-topped so the interior is
/// visible from above; the near walls test occlusion from the iso camera.
fn spawn_house(commands: &mut Commands, center: Vec2) {
    const HALF: f32 = 130.0;
    const THICK: f32 = 32.0;
    const WALL_H: f32 = 180.0;
    const DOOR: f32 = 72.0;
    let color = Color::srgb(0.45, 0.40, 0.34);
    let seg = (2.0 * HALF - DOOR) * 0.5; // front-wall segment length on each side of the door
    let walls: [(Vec2, Vec2); 5] = [
        (Vec2::new(center.x, center.y + HALF), Vec2::new(2.0 * HALF + THICK, THICK)), // back
        (Vec2::new(center.x - HALF, center.y), Vec2::new(THICK, 2.0 * HALF + THICK)), // left
        (Vec2::new(center.x + HALF, center.y), Vec2::new(THICK, 2.0 * HALF + THICK)), // right
        (Vec2::new(center.x - (DOOR * 0.5 + seg * 0.5), center.y - HALF), Vec2::new(seg, THICK)), // front-left
        (Vec2::new(center.x + (DOOR * 0.5 + seg * 0.5), center.y - HALF), Vec2::new(seg, THICK)), // front-right
    ];
    for (i, (wall_center, footprint)) in walls.into_iter().enumerate() {
        commands.spawn((
            PlaceholderVisual::prop(color, footprint, WALL_H),
            Transform::from_translation(wall_center.extend(0.0)),
            Collider {
                bounds: Rect::from_center_size(wall_center, footprint),
            },
            Name::new(format!("HouseWall{i}")),
        ));
    }
}

fn first_tile_world_position_for_region(map: &MapTiles, region_id: u16) -> Option<Vec3> {
    for (y, row) in map.tiles.iter().enumerate() {
        for (x, tile) in row.iter().enumerate() {
            if tile.location_id == region_id {
                return Some(tile_center_world(crate::core::Position {
                    x: x as i32,
                    y: y as i32,
                })
                .extend(0.0));
            }
        }
    }
    None
}

pub fn update_interactable_cache(
    mut cache: ResMut<CachedInteractables>,
    query: Query<(&Transform, &Interactable), With<Interactable>>,
) {
    rebuild_interactable_cache(&mut cache, &query);
}

pub fn update_quad_tree(
    query: Query<&Collider>,
    mut quad_tree: ResMut<QuadTree>,
) {
    rebuild_quad_tree(&query, &mut quad_tree);
}

fn rebuild_interactable_cache(
    cache: &mut CachedInteractables,
    query: &Query<(&Transform, &Interactable), With<Interactable>>,
) {
    let entries = cache.0.len().max(query.iter().size_hint().0);
    cache.0.clear();
    cache.0.reserve(entries);
    // Transform is Copy, so dereferencing avoids the clone we used to do.
    for (transform, interactable) in query.iter() {
        cache.0.push((*transform, interactable.clone()));
    }
}

fn rebuild_quad_tree(query: &Query<&Collider>, quad_tree: &mut QuadTree) {
    // Enclose all colliders. The world is centered on the tile origin (~2048),
    // far from (0,0), so the old fixed origin-centered root rect (±1024) missed
    // every collider and collision never registered.
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    for collider in query.iter() {
        min = min.min(collider.bounds.min);
        max = max.max(collider.bounds.max);
    }
    let root = if min.x <= max.x {
        Rect::from_corners(min - Vec2::splat(512.0), max + Vec2::splat(512.0))
    } else {
        Rect::from_center_size(Vec2::ZERO, Vec2::splat(2048.0))
    };
    let mut quadtree = QuadtreeNode::new(root, 0);
    for collider in query.iter() {
        quadtree.insert(collider.clone());
    }
    quad_tree.0 = quadtree;
}

/// Only rebuilds the interactable cache and quadtree when something actually
/// changed. Previously this ran every frame, cloning every Transform and
/// Interactable plus rebuilding the entire quadtree. The vast majority of
/// frames have neither colliders nor interactables changing, so the dirty-bit
/// short-circuit is a clear win.
pub fn update_cache(
    mut cache_interactables: ResMut<CachedInteractables>,
    interactable_query: Query<(&Transform, &Interactable), With<Interactable>>,
    interactable_changed: Query<
        Entity,
        Or<(
            Added<Interactable>,
            Changed<Interactable>,
            Changed<Transform>,
        )>,
    >,
    removed_interactables: RemovedComponents<Interactable>,
    collider_query: Query<&Collider>,
    collider_changed: Query<Entity, Or<(Added<Collider>, Changed<Collider>)>>,
    removed_colliders: RemovedComponents<Collider>,
    mut quad_tree: ResMut<QuadTree>,
) {
    let interactables_dirty =
        !interactable_changed.is_empty() || !removed_interactables.is_empty();
    if interactables_dirty {
        rebuild_interactable_cache(&mut cache_interactables, &interactable_query);
    }

    let colliders_dirty = !collider_changed.is_empty() || !removed_colliders.is_empty();
    if colliders_dirty {
        rebuild_quad_tree(&collider_query, &mut quad_tree);
    }
}

/// Spawn the player avatar (party leader) and companion allies from the chosen
/// [`SelectedParty`], exactly once, the first time a gameplay state is entered.
///
/// Deferred out of [`setup`] (a `Startup` system) because the roster isn't known
/// until the party-selection screen has run. This runs every frame but no-ops
/// while still in the menus and after it has spawned once.
/// Skill points each party member starts the run with, so the skill screen
/// (`K`) is usable immediately.
const STARTING_SKILL_POINTS: u32 = 6;

pub fn spawn_party(
    mut commands: Commands,
    game_state: Res<GameState>,
    selected: Res<SelectedParty>,
    mut progression: ResMut<PartyProgression>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    // Hold until the player has left the menus into actual gameplay.
    if matches!(game_state.0, Game_State::MainMenu | Game_State::PartySelection) {
        return;
    }

    // Seed persistent skill progression for everyone in the party (idempotent:
    // only fills in members that don't already have an entry).
    for kind in selected.0.iter().copied() {
        let progress = progression.entry_mut(kind);
        if progress.learned.is_empty() && progress.spent == 0 && progress.available == 0 {
            progress.available = STARTING_SKILL_POINTS;
        }
    }

    let origin3 = tile_center_world(PLAYER_SPAWN_TILE).extend(0.0);

    // Leader → the overworld Player avatar. SelectedParty defaults non-empty, so
    // the fallback is purely defensive.
    let leader = selected.leader().unwrap_or(CharacterKind::Rina);
    commands.spawn((
        // Toon-shaded hero capsule, tinted to the leader's colour.
        PlaceholderVisual::character(leader.color()).toon(),
        Transform::from_translation(origin3),
        Player,
        leader, // CharacterKind tag — drives the leader's in-battle identity.
        // The player has signed the Merchant's Contract; this drives resurrection
        // eligibility (combat_plugin::enqueue_resurrection_on_death).
        Bound,
        ResurrectionStanding::default(),
        leader.combat_stats(),
        VisualOcclusionTarget,
        YSort { base_z: 0.0 },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
        CombatMovePoints::default(),
    ));

    // Companions → WorldAlly entities, fanned out beside the leader so they
    // don't stack on one tile.
    let ally_base = origin3 + Vec3::new(-2.0 * 32.0, -2.0 * 32.0, 0.0);
    let companions = selected.companions();
    for (i, kind) in companions.iter().copied().enumerate() {
        commands.spawn((
            PlaceholderVisual::character(kind.color()),
            Transform::from_translation(ally_base + Vec3::new(i as f32 * 32.0, 0.0, 0.0)),
            WorldAlly,
            kind,
            // Companions carry the same combat/contract components as the leader
            // so they can fall, be resurrected, and — if promoted — step into the
            // leader role without missing any state (see `apply_set_leader_system`
            // and `auto_promote_dead_leader_system`).
            kind.combat_stats(),
            Bound,
            ResurrectionStanding::default(),
            CombatMovePoints::default(),
            VisualOcclusionTarget,
            YSort { base_z: 0.0 },
            crate::light_plugin::LightSensitive { threshold: 0.15 },
            Name::new(kind.display_name()),
        ));
    }

    *spawned = true;
    info!(
        "Spawned party: leader {:?} + {} companion(s)",
        leader,
        companions.len()
    );
}

/// Marker for the sanctuary where downed party members are revived. Stand on it
/// to finalise any pending resurrection (see [`revive_shrine_system`]). The same
/// entity is also a [`ResurrectionPoint`], so the resurrected characters wake up
/// right here.
#[derive(Component)]
pub struct ReviveShrine;

/// Request to promote a party member to leader (the overworld avatar). Emitted by
/// the pause-menu "Party" page; applied by [`apply_set_leader_system`].
#[derive(Message)]
pub struct SetLeaderRequest {
    pub kind: CharacterKind,
}

/// Swap the [`Player`] marker from the current leader onto the world entity of
/// the requested (living) party member, demoting the old leader to a
/// [`WorldAlly`]. Both entities already hold the full leader component set
/// (combat stats, contract, move points), so only the markers and the
/// [`SelectedParty`] ordering need to change. A downed member can't be made
/// leader — there must always be a living avatar.
pub fn apply_set_leader_system(
    mut commands: Commands,
    mut requests: MessageReader<SetLeaderRequest>,
    mut party: ResMut<SelectedParty>,
    player_q: Query<Entity, With<Player>>,
    ally_q: Query<(Entity, &CharacterKind), (With<WorldAlly>, Without<Dead>)>,
) {
    for ev in requests.read() {
        let Ok(old_leader) = player_q.single() else {
            continue;
        };
        let Some((new_leader, _)) = ally_q.iter().find(|(_, k)| **k == ev.kind) else {
            // No living companion of that kind — already leader, or downed.
            continue;
        };

        commands.entity(old_leader).remove::<Player>().insert(WorldAlly);
        commands.entity(new_leader).remove::<WorldAlly>().insert(Player);
        promote_in_party_order(&mut party, ev.kind);
        info!("Leader changed to {:?}", ev.kind);
    }
}

/// If the leader has fallen but a companion still stands, hand the avatar to a
/// living companion so the overworld is never controlled by a corpse. The old
/// leader keeps its `Dead` / `AwaitingResurrection` state and trails the party as
/// a downed companion until revived. Runs only while exploring so it never races
/// the battle's world links.
pub fn auto_promote_dead_leader_system(
    mut commands: Commands,
    game_state: Res<GameState>,
    mut party: ResMut<SelectedParty>,
    dead_leader_q: Query<Entity, (With<Player>, With<Dead>)>,
    living_ally_q: Query<(Entity, &CharacterKind), (With<WorldAlly>, Without<Dead>)>,
) {
    if game_state.0 != Game_State::Exploring {
        return;
    }
    let Ok(old_leader) = dead_leader_q.single() else {
        return;
    };
    let Some((new_leader, &new_kind)) = living_ally_q.iter().next() else {
        // Whole party is down — the battle layer raises GameOver; nothing to do.
        return;
    };

    commands.entity(old_leader).remove::<Player>().insert(WorldAlly);
    commands.entity(new_leader).remove::<WorldAlly>().insert(Player);
    promote_in_party_order(&mut party, new_kind);
    info!("Leader fell — {:?} takes the lead", new_kind);
}

/// Move `kind` to the front of the party order (the leader slot), shifting the
/// previous leader down into the companion ranks.
fn promote_in_party_order(party: &mut SelectedParty, kind: CharacterKind) {
    if let Some(pos) = party.0.iter().position(|k| *k == kind) {
        let k = party.0.remove(pos);
        party.0.insert(0, k);
    }
}

/// Standing on the [`ReviveShrine`] finalises every pending resurrection now:
/// each downed party member's deadline is pulled to the present so
/// `process_resurrection_queue_system` restores them this frame, snapping them
/// to the shrine via `teleport_on_resurrection`.
pub fn revive_shrine_system(
    game_state: Res<GameState>,
    timestamp: Res<Timestamp>,
    player_q: Query<&Transform, With<Player>>,
    shrine_q: Query<&Transform, With<ReviveShrine>>,
    mut awaiting_q: Query<&mut AwaitingResurrection>,
) {
    if game_state.0 != Game_State::Exploring {
        return;
    }
    let Ok(player_tf) = player_q.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();
    let at_shrine = shrine_q
        .iter()
        .any(|t| t.translation.truncate().distance(player_pos) <= 48.0);
    if !at_shrine {
        return;
    }

    for mut awaiting in awaiting_q.iter_mut() {
        if awaiting.ready_at_timestamp > timestamp.0 {
            awaiting.ready_at_timestamp = timestamp.0;
        }
    }
}

// `apply_y_sort` and `update_visual_occluders` were 2D-only (fake depth via z,
// and sprite-alpha fade when covered). In 3D the depth buffer handles ordering
// and these are removed. The `YSort` / `VisualOccluder` / `FadeWhenCovered` /
// `VisualOcclusionTarget` marker types are kept (inert) so existing spawn sites
// still compile; they'll be cleaned up as 3D occlusion is built out.
