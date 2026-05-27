//! 3D rendering foundation for the isometric port (Phase 1).
//!
//! ## Coordinate convention
//!
//! The world keeps its original 2D layout: the **XY plane is the ground** and
//! **+Z is up**. All existing gameplay math (movement, collision, distance
//! checks, the quadtree, facing via `Quat::from_rotation_z`) operates on
//! `translation.xy` and is therefore unchanged by the 3D port — only the camera,
//! meshes, and lighting differ from the old 2D path. Entities stand up out of
//! the ground along +Z.
//!
//! Note: glTF/Blender are Y-up, so models imported in a later phase get a single
//! load-time +90° rotation about X to stand correctly in this Z-up world.

use bevy::prelude::*;
use bevy_camera::{OrthographicProjection, Projection, ScalingMode};

/// True-isometric viewing angles for the camera rig.
pub const ISO_AZIMUTH_DEG: f32 = 45.0;
pub const ISO_ELEVATION_DEG: f32 = 35.264_4; // atan(1/sqrt(2)) — true isometric
/// How far the camera sits from its focus. Orthographic, so this only affects
/// clipping, not apparent size; kept large so the near plane never clips.
pub const ISO_DISTANCE: f32 = 2000.0;
/// Vertical span of the orthographic viewport, in world units. Tune for zoom.
pub const ISO_VIEWPORT_HEIGHT: f32 = 400.0;

/// Default height (in +Z) of a placeholder character box.
pub const CHAR_HEIGHT: f32 = 56.0;

/// Camera offset from its focus point on the ground (Z-up world).
pub fn iso_camera_offset() -> Vec3 {
    let az = ISO_AZIMUTH_DEG.to_radians();
    let el = ISO_ELEVATION_DEG.to_radians();
    Vec3::new(
        ISO_DISTANCE * el.cos() * az.cos(),
        ISO_DISTANCE * el.cos() * az.sin(),
        ISO_DISTANCE * el.sin(),
    )
}

/// Transform for the isometric camera focused on `focus` (a point on the XY
/// ground). World up is +Z.
pub fn iso_camera_transform(focus: Vec3) -> Transform {
    Transform::from_translation(focus + iso_camera_offset()).looking_at(focus, Vec3::Z)
}

/// The orthographic projection for the isometric camera.
pub fn iso_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical {
            viewport_height: ISO_VIEWPORT_HEIGHT,
        },
        near: -ISO_DISTANCE * 2.0,
        far: ISO_DISTANCE * 2.0,
        ..OrthographicProjection::default_3d()
    })
}

/// Shared placeholder mesh/material handles so spawn systems outside `setup`
/// (map tiles, battle combatants, NPCs) can render boxes without their own
/// access to `Assets`. Real glTF art replaces these in a later phase.
#[derive(Resource, Clone)]
pub struct PlaceholderAssets {
    /// 1×1 quad lying in the XY plane (normal +Z) — scale for ground tiles.
    pub ground_quad: Handle<Mesh>,
    /// 1×1×1 cube — scale for obstacles / props / combatants.
    pub unit_cube: Handle<Mesh>,
    pub ground_mat: Handle<StandardMaterial>,
    pub border_mat: Handle<StandardMaterial>,
    pub obstacle_mat: Handle<StandardMaterial>,
    pub npc_mat: Handle<StandardMaterial>,
}

impl PlaceholderAssets {
    pub fn build(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        Self {
            ground_quad: meshes.add(Rectangle::new(1.0, 1.0)),
            unit_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            ground_mat: materials.add(StandardMaterial {
                base_color: Color::srgb(0.18, 0.20, 0.24),
                perceptual_roughness: 0.95,
                ..default()
            }),
            border_mat: materials.add(StandardMaterial {
                base_color: Color::srgb(0.05, 0.07, 0.12),
                perceptual_roughness: 1.0,
                ..default()
            }),
            obstacle_mat: materials.add(StandardMaterial {
                base_color: Color::srgb(0.55, 0.18, 0.18),
                perceptual_roughness: 0.8,
                ..default()
            }),
            npc_mat: materials.add(StandardMaterial {
                base_color: Color::srgb(0.75, 0.75, 0.8),
                perceptual_roughness: 0.6,
                ..default()
            }),
        }
    }
}

/// A placeholder visual: a colored box standing on the XY ground. Attach this
/// in place of the old `Sprite` at spawn time; `hydrate_placeholders` turns it
/// into a real `Mesh3d` + `MeshMaterial3d` once, creating the mesh/material.
/// Real glTF models replace this in a later phase.
#[derive(Component, Clone)]
pub struct PlaceholderVisual {
    pub color: Color,
    /// Footprint on the XY plane (x, y) and height along +Z (z).
    pub size: Vec3,
    /// When true the box rests on the ground (its base at z = 0); otherwise it
    /// is centered on the entity's translation.
    pub grounded: bool,
    /// Set once hydrated so we don't re-offset the transform.
    hydrated: bool,
}

impl PlaceholderVisual {
    /// A standing character-sized box (old 32×32 sprite → 28×28 footprint).
    pub fn character(color: Color) -> Self {
        Self { color, size: Vec3::new(28.0, 28.0, CHAR_HEIGHT), grounded: true, hydrated: false }
    }
    /// A prop/obstacle box with an explicit footprint; `height` is along +Z.
    pub fn prop(color: Color, footprint: Vec2, height: f32) -> Self {
        Self { color, size: footprint.extend(height), grounded: true, hydrated: false }
    }
}

/// Turn freshly-added [`PlaceholderVisual`]s into boxes, and raise grounded ones
/// so their base sits on z = 0. Runs every frame but only touches new entities.
pub fn hydrate_placeholders(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &mut PlaceholderVisual, &mut Transform), Added<PlaceholderVisual>>,
) {
    for (entity, mut vis, mut transform) in &mut q {
        if vis.hydrated {
            continue;
        }
        let mesh = meshes.add(Cuboid::new(vis.size.x, vis.size.y, vis.size.z));
        let material = materials.add(StandardMaterial {
            base_color: vis.color,
            perceptual_roughness: 0.7,
            ..default()
        });
        if vis.grounded {
            transform.translation.z += vis.size.z * 0.5;
        }
        vis.hydrated = true;
        commands
            .entity(entity)
            .insert((Mesh3d(mesh), MeshMaterial3d(material)));
    }
}

/// Spawn the isometric camera and the scene's directional sun + ambient fill.
/// Returns the camera entity so the caller can tag it (e.g. `MainCamera`).
pub fn spawn_iso_camera(commands: &mut Commands, focus: Vec3) -> Entity {
    commands
        .spawn((
            Camera3d::default(),
            iso_projection(),
            iso_camera_transform(focus),
            // Ambient fill is per-camera in Bevy 0.18.
            AmbientLight {
                brightness: 350.0,
                ..default()
            },
        ))
        .id()
}

/// Insert the directional "sun" (shadow-casting) for the scene.
pub fn spawn_sun(commands: &mut Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadows_enabled: true,
            ..default()
        },
        // Shine downward into the XY ground at an angle so boxes cast shadows.
        Transform::default().looking_to(Vec3::new(-0.5, -0.6, -1.0).normalize(), Vec3::Z),
    ));
}
