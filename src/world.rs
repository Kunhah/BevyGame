use bevy::prelude::*;

use crate::battle::{CombatMovePoints, EnemyEncounter, WorldAlly, WorldNpc, WorldYokai, YokaiKind};
use crate::city_data::CityCatalog;
use crate::combat_plugin::{Bound, CombatStats, ResurrectionPoint, ResurrectionStanding, StatPool};
use crate::core::{MainCamera, Player, Position};
use crate::dialogue::{CachedInteractables, Interactable};
use crate::economy::{MerchantNpc, Merchants};
use crate::governance::GovernorNpc;
use crate::light_plugin::Occluder;
use crate::map::{tile_center_world, MapTiles, TILE_WORLD_SIZE};
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
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // The player spawns at the center of tile (0, 0). All the hardcoded test
    // entities below were authored relative to a 512-unit world, so we offset
    // them by the same origin so they remain visible near the player as
    // `TILE_WORLD_SIZE` grows.
    let world_origin = tile_center_world(Position::default());
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
    commands.entity(camera).insert(MainCamera);
    spawn_sun(&mut commands);

    let mut player_stats = CombatStats::default();
    player_stats.health = StatPool::<i32>::new(120);
    commands.spawn((
        PlaceholderVisual::character(Color::WHITE),
        Transform::from_translation(origin3),
        Player,
        // The player has signed the Merchant's Contract; this drives
        // resurrection eligibility (combat_plugin::enqueue_resurrection_on_death).
        Bound,
        ResurrectionStanding::default(),
        player_stats,
        VisualOcclusionTarget,
        YSort { base_z: 0.0 },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
        CombatMovePoints::default(),
    ));

    // Shrine: the player respawns at this location after the resurrection
    // delay elapses. Spawned once at world setup; the
    // `teleport_on_resurrection` system snaps any resurrected entity to the
    // closest one.
    commands.spawn((
        PlaceholderVisual::prop(Color::srgb(0.55, 0.75, 0.95), Vec2::splat(40.0), 40.0),
        Transform::from_translation(origin3 + Vec3::new(-12.0 * 32.0, 4.0 * 32.0, 0.0)),
        ResurrectionPoint,
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

    let ally_spawn = origin3 + Vec3::new(-2.0 * 32.0, -2.0 * 32.0, 0.0);
    let ally_colors = [
        Color::srgb(0.9, 0.8, 0.2),
        Color::srgb(0.9, 0.6, 0.2),
        Color::srgb(0.5, 0.85, 0.4),
    ];
    for (i, color) in ally_colors.into_iter().enumerate() {
        commands.spawn((
            PlaceholderVisual::character(color),
            Transform::from_translation(ally_spawn),
            WorldAlly,
            VisualOcclusionTarget,
            YSort { base_z: 0.0 },
            crate::light_plugin::LightSensitive { threshold: 0.15 },
            Name::new(format!("Ally{}", i)),
        ));
    }

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
    const THICK: f32 = 24.0;
    const WALL_H: f32 = 110.0;
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
    let mut quadtree =
        QuadtreeNode::new(Rect::from_center_size(Vec2::ZERO, Vec2::splat(2048.0)), 0);
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

// `apply_y_sort` and `update_visual_occluders` were 2D-only (fake depth via z,
// and sprite-alpha fade when covered). In 3D the depth buffer handles ordering
// and these are removed. The `YSort` / `VisualOccluder` / `FadeWhenCovered` /
// `VisualOcclusionTarget` marker types are kept (inert) so existing spawn sites
// still compile; they'll be cleaned up as 3D occlusion is built out.
