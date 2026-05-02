use bevy::prelude::*;

use crate::battle::{CombatMovePoints, EnemyEncounter, WorldAlly, WorldNpc};
use crate::city_data::CityCatalog;
use crate::core::{MainCamera, Player, Position};
use crate::dialogue::{load_dialogue, CachedInteractables, Dialogue_Data, Interactable};
use crate::economy::{MerchantNpc, Merchants};
use crate::governance::GovernorNpc;
use crate::light_plugin::Occluder;
use crate::map::{tile_center_world, MapTiles, TILE_WORLD_SIZE};
use crate::quadtree::{Collider, QuadTree, QuadtreeNode};
use crate::services::{ServiceKind, ServiceNpc};
use bevy::sprite::Anchor;
use bevy_camera::{Camera2d, visibility::RenderLayers};

const MAIN_CAMERA_HEIGHT: f32 = 700.0;
const Y_SORT_SCALE: f32 = 0.001;
const OCCLUSION_FADE_SPEED: f32 = 8.0;

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
    asset_server: Res<AssetServer>,
    map: Res<MapTiles>,
    cities: Res<CityCatalog>,
    merchants: Res<Merchants>,
    query: Query<&Collider>,
    occ: Option<Res<crate::light_plugin::OcclusionTarget>>,
) {
    let mut quadtree = QuadtreeNode::new(Rect::from_center_size(Vec2::ZERO, Vec2::splat(2048.0)), 0);

    for collider in &query {
        quadtree.insert(collider.clone());
    }

    commands.insert_resource(QuadTree(quadtree));

    let dialogue_data = load_dialogue();
    commands.insert_resource(Dialogue_Data(dialogue_data));

    commands
        .spawn((
            Camera2d,
            RenderLayers::layer(0), // explicitly see layer 0 (light quad and world)
            MainCamera,
            Transform::from_xyz(
                tile_center_world(Position::default()).x,
                tile_center_world(Position::default()).y,
                MAIN_CAMERA_HEIGHT,
            ),
        ));

    commands.spawn((
        Sprite {
            image: asset_server.load("character.png"),
            custom_size: Some(Vec2::new(32.0, 32.0)),
            ..default()
        },
        Transform::from_translation(tile_center_world(Position::default()).extend(0.0)),
        Player,
        VisualOcclusionTarget,
        YSort { base_z: 0.0 },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
        CombatMovePoints::default(),
    ));

    commands.spawn((
        Sprite {
            image: asset_server.load("character.png"),
            color: Color::srgb(0.3, 0.9, 0.3),
            custom_size: Some(Vec2::new(32.0, 32.0)),
            ..default()
        },
        Transform::from_xyz(4.0 * 32.0, 0.0, 0.0),
        EnemyEncounter { id: 1 },
        VisualOcclusionTarget,
        YSort { base_z: 0.0 },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
    ));

    for merchant in merchants.0.values() {
        if let Some(spawn_pos) = first_tile_world_position_for_region(&map, merchant.region_id) {
            commands.spawn((
                Sprite {
                    image: asset_server.load("character.png"),
                    color: Color::srgb(0.2, 0.6, 0.9),
                    custom_size: Some(Vec2::new(32.0, 32.0)),
                    ..default()
                },
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
                Sprite {
                    image: asset_server.load("character.png"),
                    color: Color::srgb(0.85, 0.2, 0.2),
                    custom_size: Some(Vec2::new(32.0, 32.0)),
                    ..default()
                },
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
                    Sprite {
                        image: asset_server.load("character.png"),
                        color,
                        custom_size: Some(Vec2::new(32.0, 32.0)),
                        ..default()
                    },
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

    commands.spawn((
        Sprite {
            image: asset_server.load("character.png"),
            color: Color::srgb(0.9, 0.8, 0.2),
            custom_size: Some(Vec2::new(32.0, 32.0)),
            ..default()
        },
        Transform::from_xyz(-2.0 * 32.0, 0.0, 0.0),
        WorldAlly,
        VisualOcclusionTarget,
        YSort { base_z: 0.0 },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
        Name::new("AllyA"),
    ));

    commands.spawn((
        Sprite {
            image: asset_server.load("character.png"),
            color: Color::srgb(0.9, 0.6, 0.2),
            custom_size: Some(Vec2::new(32.0, 32.0)),
            ..default()
        },
        Transform::from_xyz(-3.0 * 32.0, -1.0 * 32.0, 0.0),
        WorldAlly,
        VisualOcclusionTarget,
        YSort { base_z: 0.0 },
        crate::light_plugin::LightSensitive { threshold: 0.15 },
        Name::new("AllyB"),
    ));

    let tower_base = Vec3::new(6.0 * 32.0, 5.0 * 32.0, 0.0);
    commands.spawn((
        Sprite {
            image: asset_server.load("character.png"),
            color: Color::srgb(0.62, 0.42, 0.22),
            custom_size: Some(Vec2::new(96.0, 192.0)),
            ..default()
        },
        Anchor::BOTTOM_CENTER,
        Transform::from_translation(tower_base),
        Collider {
            bounds: Rect::from_center_size(
                Vec2::new(tower_base.x, tower_base.y + 10.0),
                Vec2::new(36.0, 20.0),
            ),
        },
        Visibility::Visible,
        InheritedVisibility::default(),
        ViewVisibility::default(),
        Occluder,
        RenderLayers::layer(0),
        FadeWhenCovered,
        VisualOccluder {
            size: Vec2::new(120.0, 180.0),
            fade_alpha: 0.35,
            solid_alpha: 1.0,
        },
        YSort { base_z: 0.0 },
        Name::new("Tower"),
    ));

    for x in 1..3 {
        commands.spawn((
            Sprite {
                image: asset_server.load("character.png"),
                color: Color::srgb(0.8, 0.1, 0.1),
                custom_size: Some(Vec2::new(32.0, 32.0)),
                ..default()
            },
            Transform::from_xyz(x as f32 * 32.0, 5.0 * 32.0, 0.0),
            Interactable {
                name: "Test interactable".to_string(),
                dialogue_id: "The last goodbye 1".to_string(),
            },
        ));
    }

    // Temporary debug: show the occlusion texture on screen to verify occluders render.
    if let Some(occ) = occ {
        commands.spawn((
            Sprite {
                image: occ.image.clone(),
                custom_size: Some(Vec2::splat(128.0)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 50.0),
            Name::new("OcclusionDebugSprite"),
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

pub fn apply_y_sort(mut query: Query<(&mut Transform, &YSort)>) {
    for (mut transform, y_sort) in &mut query {
        transform.translation.z = y_sort.base_z - transform.translation.y * Y_SORT_SCALE;
    }
}

pub fn update_visual_occluders(
    time: Res<Time>,
    targets: Query<&Transform, With<VisualOcclusionTarget>>,
    mut occluders: Query<(&Transform, &VisualOccluder, &mut Sprite), With<FadeWhenCovered>>,
    mut target_positions: Local<Vec<Vec2>>,
) {
    let delta = (time.delta_secs() * OCCLUSION_FADE_SPEED).clamp(0.0, 1.0);

    // Precompute target positions once instead of re-iterating the targets
    // query inside every occluder loop body (O(n²) → O(n + m)).
    target_positions.clear();
    target_positions.extend(targets.iter().map(|t| t.translation.truncate()));

    for (transform, occluder, mut sprite) in &mut occluders {
        let base = transform.translation.truncate();
        let half_width = occluder.size.x * 0.5;
        let min_x = base.x - half_width;
        let max_x = base.x + half_width;
        let min_y = base.y;
        let max_y = base.y + occluder.size.y;

        let should_fade = target_positions
            .iter()
            .any(|pos| pos.x >= min_x && pos.x <= max_x && pos.y >= min_y && pos.y <= max_y);

        let target_alpha = if should_fade {
            occluder.fade_alpha
        } else {
            occluder.solid_alpha
        };
        let color = sprite.color.to_srgba();
        let next_alpha = color.alpha + (target_alpha - color.alpha) * delta;
        if (next_alpha - color.alpha).abs() > f32::EPSILON {
            sprite.color = Color::srgba(color.red, color.green, color.blue, next_alpha);
        }
    }
}
