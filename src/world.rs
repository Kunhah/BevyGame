use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::core::{MainCamera, Player, Position};
use crate::dialogue::{
    load_dialogue, CachedInteractables, Dialogue_Data, Dialogue_State, Interactable,
};
use crate::light_plugin::Occluder;
use crate::quadtree::{Collider, QuadTree, QuadtreeNode};
use bevy_camera::visibility::RenderLayers;

pub fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    query: Query<(Entity, &Transform), With<Collider>>,
    occ: Option<Res<crate::light_plugin::OcclusionTarget>>,
) {
    let mut quadtree = QuadtreeNode::new(Rect::from_center_size(Vec2::ZERO, Vec2::splat(2048.0)), 0);

    for (_, transform) in &query {
        let pos = transform.translation.truncate();
        let rect = Rect::from_center_size(pos, Vec2::splat(32.0));

        quadtree.insert(Collider { bounds: rect });
    }

    commands.insert_resource(QuadTree(quadtree));

    let dialogue_data = load_dialogue();
    commands.insert_resource(Dialogue_Data(dialogue_data));

    commands
        .spawn((
            Camera2d::default(),
            RenderLayers::layer(0), // explicitly see layer 0 (light quad and world)
            MainCamera,
            Transform::from_xyz(0.0, 0.0, 0.0),
            Position { x: 0, y: 0 },
        ));

    commands.spawn((
        Sprite {
            image: asset_server.load("character.png"),
            custom_size: Some(Vec2::new(32.0, 32.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
        Position { x: 0, y: 0 },
        Player,
    ));

    for x in 5..7 {
        commands.spawn((
            Sprite {
                image: asset_server.load("character.png"),
                color: Color::srgb(0.8, 0.1, 0.1),
                custom_size: Some(Vec2::new(32.0, 32.0)),
                ..default()
            },
            Transform::from_xyz(x as f32 * 32.0, 5.0 * 32.0, 0.0),
            Position {
                x: x * 32,
                y: 5 * 32,
            },
            Collider {
                bounds: Rect::from_center_size(
                    Vec2::new(x as f32 * 32.0, 5.0 * 32.0),
                    Vec2::splat(32.0),
                ),
            },
            Visibility::Visible,
            InheritedVisibility::default(),
            ViewVisibility::default(),
            Occluder,
            // Visible in main camera (layer 0) and occlusion camera (layer 1).
            RenderLayers::from_layers(&[0, 1]),
        ));
    }

    for x in 1..3 {
        commands.spawn((
            Sprite {
                image: asset_server.load("character.png"),
                color: Color::srgb(0.8, 0.1, 0.1),
                custom_size: Some(Vec2::new(32.0, 32.0)),
                ..default()
            },
            Transform::from_xyz(x as f32 * 32.0, 5.0 * 32.0, 0.0),
            Position { x: x * 32, y: 5 * 32 },
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

pub fn update_interactable_cache(
    mut cache: ResMut<CachedInteractables>,
    query: Query<(&Transform, &Interactable), With<Interactable>>,
) {
    cache.0 = query.iter().map(|(t, i)| (t.clone(), i.clone())).collect();
}

pub fn update_quad_tree(
    query: Query<(Entity, &Transform), With<Collider>>,
    mut quadTree: ResMut<QuadTree>,
) {
    let mut quadtree = QuadtreeNode::new(Rect::from_center_size(Vec2::ZERO, Vec2::splat(2048.0)), 0);

    for (_, transform) in &query {
        let pos = transform.translation.truncate();
        let rect = Rect::from_center_size(pos, Vec2::splat(32.0));

        quadtree.insert(Collider { bounds: rect });
    }

    quadTree.0 = quadtree;
}

pub fn update_cache(
    cache_interactables: ResMut<CachedInteractables>,
    interactable_query: Query<(&Transform, &Interactable), With<Interactable>>,
    query: Query<(Entity, &Transform), With<Collider>>,
    mut quadTree: ResMut<QuadTree>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if input.just_pressed(KeyCode::KeyP) {
        update_interactable_cache(cache_interactables, interactable_query);
        update_quad_tree(query, quadTree);
    }
}
