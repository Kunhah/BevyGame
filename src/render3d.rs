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

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy_camera::{OrthographicProjection, Projection, ScalingMode};
use bevy_mod_outline::{OutlineMode, OutlineStencil, OutlineVolume};
// Phase 4 post-processing components.
use bevy::core_pipeline::{
    prepass::{DepthPrepass, NormalPrepass},
    tonemapping::Tonemapping,
};
use bevy::pbr::{DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion};
use bevy::post_process::bloom::Bloom;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection, Msaa};

/// Toon shading parameters — mirrors the `ToonParams` uniform in `toon.wgsl`
/// (field order/layout must match exactly: std140, 80 bytes, 16-aligned).
///
/// **Shading model:** a 3-stop "anime ramp" sampled at the lit luminance. The
/// ramp maps `t = saturate(lit_luminance)` → an RGB multiplier:
///   `t < ramp_t_shadow`  → `shadow_tint.rgb` (deep cool/desaturated)
///   `t ≈ ramp_t_core`    → `core_shadow_color.rgb` (warm "core shadow" band)
///   `t > ramp_t_lit`     → white (the fully-lit end)
/// `ramp_softness` controls how sharp the transitions are (small = hard cel
/// edges, large = smooth gradient). The shader's `anime_ramp(t)` helper is a
/// drop-in replacement point for a future `textureSample(ramp, ...)` when an
/// artist-painted ramp PNG arrives — same call signature, same output.
#[derive(Clone, Copy, ShaderType, Debug, Reflect)]
pub struct ToonParams {
    /// Silhouette rim/Fresnel color (rgb) and strength multiplier (a, unused).
    pub rim_color: Vec4,
    /// Deep-shadow end of the ramp (rgb). `a` reserved.
    pub shadow_tint: Vec4,
    /// Warm "core shadow" mid-stop of the ramp (rgb). `a` reserved.
    pub core_shadow_color: Vec4,
    pub rim_strength: f32,
    pub rim_power: f32,
    /// Position of the deep→core transition along `t` (typical: 0.15..0.30).
    pub ramp_t_shadow: f32,
    /// Position of the core→lit transition along `t` (typical: 0.40..0.60).
    pub ramp_t_lit: f32,
    /// Smoothstep half-width at each transition. Small = hard cel edges
    /// (anime), large = smooth gradient (toon).
    pub ramp_softness: f32,
    /// `crate::effects::HitFlash` uniform — additive warm-white pulse.
    pub hit_flash: f32,
    /// `crate::effects::Dissolve` uniform — 0 = solid, 1 = fully dissolved.
    pub dissolve: f32,
    /// Pad to 80 bytes (multiple of 16, std140 struct size).
    pub _pad: f32,
}

impl Default for ToonParams {
    fn default() -> Self {
        Self {
            rim_color: Vec4::new(0.5, 0.65, 1.0, 1.0),       // cool anime rim
            shadow_tint: Vec4::new(0.22, 0.24, 0.36, 1.0),   // deep cool shadow
            core_shadow_color: Vec4::new(0.62, 0.45, 0.50, 1.0), // warm rust core
            rim_strength: 0.30,
            rim_power: 3.5,
            ramp_t_shadow: 0.18, // deep→core sits low — most of the dark side stays deep
            ramp_t_lit: 0.45,    // core→lit edge near classic anime "terminator"
            ramp_softness: 0.04, // sharp cel transitions
            hit_flash: 0.0,
            dissolve: 0.0,
            _pad: 0.0,
        }
    }
}

/// `StandardMaterial` extension that bands the lit result into cel steps and
/// adds a rim light. Keeps Bevy's real lighting + shadows underneath.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct ToonExtension {
    #[uniform(100)]
    pub params: ToonParams,
}

impl Default for ToonExtension {
    fn default() -> Self {
        Self { params: ToonParams::default() }
    }
}

impl MaterialExtension for ToonExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/toon.wgsl".into()
    }
}

/// The toon material applied to characters.
pub type ToonMaterial = ExtendedMaterial<StandardMaterial, ToonExtension>;

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

/// Project a screen-space cursor to a point on the XY ground plane (z = 0) by
/// casting the camera's 3D ray and intersecting it with the ground. Returns the
/// ground `(x, y)`; `None` if the ray is parallel to / points away from the
/// plane. This is the 3D replacement for the old `viewport_to_world_2d` picking.
pub fn cursor_to_ground(
    camera: &Camera,
    cam_transform: &GlobalTransform,
    cursor: Vec2,
) -> Option<Vec2> {
    let ray = camera.viewport_to_world(cam_transform, cursor).ok()?;
    let t = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Dir3::Z))?;
    Some(ray.get_point(t).xy())
}

/// Runtime-controllable isometric camera state. The camera follows the player
/// (with a WSAD nudge that drifts back), can spin (Q/E yaw) and tilt (R/F
/// pitch), and zoom (mouse wheel). See [`drive_camera`].
#[derive(Resource)]
pub struct CameraRig {
    /// Azimuth around the vertical axis (radians) — Q/E spin.
    pub yaw: f32,
    /// Elevation above the ground (radians) — `[` / `]` tilt.
    pub pitch: f32,
    /// Offset magnitude from focus (orthographic, so clipping only).
    pub distance: f32,
    /// Orthographic viewport height (world units) — mouse-wheel zoom.
    pub zoom: f32,
    /// Ground point the camera looks at. When locked it tracks the player (WSAD
    /// nudges it, then it drifts back); when unlocked WSAD roams it freely.
    pub focus: Vec2,
    /// Set once the focus has been seeded to the player position.
    pub initialized: bool,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            yaw: ISO_AZIMUTH_DEG.to_radians(),
            pitch: ISO_ELEVATION_DEG.to_radians(),
            distance: ISO_DISTANCE,
            zoom: ISO_VIEWPORT_HEIGHT,
            focus: Vec2::ZERO,
            initialized: false,
        }
    }
}

/// Camera offset from focus for the rig's current yaw/pitch (Z-up world).
pub fn rig_offset(rig: &CameraRig) -> Vec3 {
    Vec3::new(
        rig.distance * rig.pitch.cos() * rig.yaw.cos(),
        rig.distance * rig.pitch.cos() * rig.yaw.sin(),
        rig.distance * rig.pitch.sin(),
    )
}

/// Drives the isometric camera. In gameplay states (Exploring/Battle): WSAD pan,
/// Q/E spin (yaw), `[` / `]` tilt (pitch), mouse-wheel zoom. `L` toggles
/// follow-lock (see `movement::toggle_camera_lock`): when **locked** the camera
/// follows the player and WSAD only nudges (drifts back); when **unlocked** WSAD
/// roams freely and the camera stays put. Input is ignored outside gameplay so
/// menus/dialogue/shop keep WSAD/Q/E/etc. This is the sole owner of the
/// `MainCamera` transform + projection.
pub fn drive_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: bevy::ecs::message::MessageReader<bevy::input::mouse::MouseWheel>,
    globals: Res<crate::core::Global_Variables>,
    game_state: Res<crate::core::GameState>,
    mut rig: ResMut<CameraRig>,
    player_q: Query<&Transform, (With<crate::core::Player>, Without<crate::core::MainCamera>)>,
    mut cam_q: Query<(&mut Transform, &mut Projection), With<crate::core::MainCamera>>,
) {
    const PAN_SPEED: f32 = 1300.0;
    const YAW_SPEED: f32 = 1.8;
    const TILT_SPEED: f32 = 1.4;
    const PITCH_MIN: f32 = 0.26; // ~15°
    const PITCH_MAX: f32 = 1.31; // ~75°
    const ZOOM_STEP: f32 = 1.12;
    const ZOOM_MIN: f32 = 80.0;
    const ZOOM_MAX: f32 = 4000.0;
    const FOLLOW_SPEED: f32 = 8.0;
    const SNAP_DIST: f32 = 3000.0;

    let dt = time.delta_secs();
    let locked = globals.0.camera_locked;
    let gameplay = matches!(
        game_state.0,
        crate::core::Game_State::Exploring | crate::core::Game_State::Battle
    );

    let Ok(player_tf) = player_q.single() else {
        return;
    };
    let player_xy = player_tf.translation.truncate();
    if !rig.initialized {
        rig.focus = player_xy;
        rig.initialized = true;
    }

    // Camera control input is only read during gameplay, so menus/dialogue/shop
    // keep their own WSAD/Q/E/etc. bindings.
    let mut pan = Vec2::ZERO;
    if gameplay {
        if keys.pressed(KeyCode::KeyQ) {
            rig.yaw += YAW_SPEED * dt;
        }
        if keys.pressed(KeyCode::KeyE) {
            rig.yaw -= YAW_SPEED * dt;
        }
        if keys.pressed(KeyCode::BracketRight) {
            rig.pitch = (rig.pitch + TILT_SPEED * dt).min(PITCH_MAX);
        }
        if keys.pressed(KeyCode::BracketLeft) {
            rig.pitch = (rig.pitch - TILT_SPEED * dt).max(PITCH_MIN);
        }

        let mut scroll = 0.0;
        for ev in wheel.read() {
            scroll += ev.y;
        }
        if scroll != 0.0 {
            rig.zoom = (rig.zoom * ZOOM_STEP.powf(-scroll)).clamp(ZOOM_MIN, ZOOM_MAX);
        }

        // Pan in the ground plane, relative to the current yaw.
        let forward = Vec2::new(-rig.yaw.cos(), -rig.yaw.sin());
        let right = Vec2::new(-rig.yaw.sin(), rig.yaw.cos());
        if keys.pressed(KeyCode::KeyW) {
            pan += forward;
        }
        if keys.pressed(KeyCode::KeyS) {
            pan -= forward;
        }
        if keys.pressed(KeyCode::KeyD) {
            pan += right;
        }
        if keys.pressed(KeyCode::KeyA) {
            pan -= right;
        }
    }
    let pan_delta = if pan != Vec2::ZERO {
        pan.normalize() * PAN_SPEED * dt
    } else {
        Vec2::ZERO
    };

    // Update the focus point: locked follows the player (pan nudges, then it
    // drifts back via the lerp); unlocked only moves by WSAD.
    if locked {
        let alpha = (FOLLOW_SPEED * dt).clamp(0.0, 1.0);
        rig.focus = rig.focus.lerp(player_xy, alpha) + pan_delta;
    } else {
        rig.focus += pan_delta;
    }

    // Apply the transform + projection.
    let Ok((mut cam_tf, mut proj)) = cam_q.single_mut() else {
        return;
    };
    let focus3 = rig.focus.extend(0.0);
    let desired = focus3 + rig_offset(&rig);
    if cam_tf.translation.distance(desired) > SNAP_DIST {
        cam_tf.translation = desired; // teleport: snap instead of long pan
    } else {
        let alpha = (FOLLOW_SPEED * dt).clamp(0.0, 1.0);
        cam_tf.translation = cam_tf.translation.lerp(desired, alpha);
    }
    cam_tf.look_at(focus3, Vec3::Z);

    if let Projection::Orthographic(ortho) = proj.as_mut() {
        ortho.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: rig.zoom,
        };
    }
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
    /// When true, hydrate with the toon [`ToonMaterial`] instead of a plain
    /// `StandardMaterial`.
    pub toon: bool,
    /// Set once hydrated so we don't re-offset the transform.
    hydrated: bool,
}

impl PlaceholderVisual {
    /// A standing character-sized capsule (old 32×32 sprite → 28×28 footprint).
    /// Characters are toon-shaded by default.
    pub fn character(color: Color) -> Self {
        Self { color, size: Vec3::new(28.0, 28.0, CHAR_HEIGHT), grounded: true, toon: true, hydrated: false }
    }
    /// A prop/obstacle box with an explicit footprint; `height` is along +Z.
    pub fn prop(color: Color, footprint: Vec2, height: f32) -> Self {
        Self { color, size: footprint.extend(height), grounded: true, toon: false, hydrated: false }
    }
    /// Render this placeholder with the toon material.
    pub fn toon(mut self) -> Self {
        self.toon = true;
        self
    }
}

/// Turn freshly-added [`PlaceholderVisual`]s into boxes, and raise grounded ones
/// so their base sits on z = 0. Runs every frame but only touches new entities.
pub fn hydrate_placeholders(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut toon_materials: ResMut<Assets<ToonMaterial>>,
    mut q: Query<(Entity, &mut PlaceholderVisual, &mut Transform), Added<PlaceholderVisual>>,
) {
    for (entity, mut vis, mut transform) in &mut q {
        if vis.hydrated {
            continue;
        }
        // Toon placeholders use a capsule standing along +Z (Capsule3d is
        // Y-aligned, so rotate the mesh +90° about X). Cel banding + rim only
        // read on curved surfaces — flat cube faces have a constant normal, so
        // PBR already renders them flat and banding changes nothing. A cube-ish
        // size (height ≈ footprint) yields a sphere (length 0). Real models in
        // Phase 5 are curved, so they'll show the effect.
        let (mesh, z_off) = if vis.toon {
            let radius = vis.size.x.min(vis.size.y) * 0.5;
            let length = (vis.size.z - 2.0 * radius).max(0.0);
            let m = Mesh::from(Capsule3d::new(radius, length))
                .rotated_by(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
            (meshes.add(m), vis.size.z * 0.5)
        } else {
            (
                meshes.add(Cuboid::new(vis.size.x, vis.size.y, vis.size.z)),
                vis.size.z * 0.5,
            )
        };
        if vis.grounded {
            transform.translation.z += z_off;
        }
        vis.hydrated = true;
        let mut ent = commands.entity(entity);
        ent.insert(Mesh3d(mesh));
        if vis.toon {
            ent.insert((
                MeshMaterial3d(toon_materials.add(ToonMaterial {
                    base: StandardMaterial {
                        base_color: vis.color,
                        perceptual_roughness: 0.6,
                        ..default()
                    },
                    extension: ToonExtension::default(),
                })),
                // Ink outline (inverted hull, constant screen-space width).
                OutlineVolume {
                    visible: true,
                    width: 3.0,
                    colour: Color::srgb(0.02, 0.02, 0.05),
                },
                OutlineMode::ExtrudeFlat,
            ));
        } else {
            ent.insert(MeshMaterial3d(materials.add(StandardMaterial {
                base_color: vis.color,
                perceptual_roughness: 0.7,
                ..default()
            })));
        }
        // Every mesh writes the outline stencil so closer geometry occludes
        // outlines behind it (an outline is hidden when something is in front).
        ent.insert(OutlineStencil::default());
    }
}

/// Set each outline's logical-pixel width from object size (sublinear: small
/// things get small outlines, bigger things get only slightly bigger outlines)
/// combined with a milder distance falloff (further entities = thinner).
pub fn scale_outline_width_by_distance(
    cam_q: Query<&GlobalTransform, With<crate::core::MainCamera>>,
    // Menu-stage actors are framed by a separate camera far from the world, so
    // distance-to-`MainCamera` is meaningless for them — they keep their
    // hydrated outline width.
    mut q: Query<
        (&GlobalTransform, Option<&PlaceholderVisual>, &mut OutlineVolume),
        Without<crate::menu::MenuActor>,
    >,
) {
    const BASE_WIDTH: f32 = 2.0;
    const SIZE_REF: f32 = 50.0; // typical character footprint scale
    const SIZE_POWER: f32 = 0.30; // strongly sublinear — big things don't get big outlines
    const DIST_POWER: f32 = 2.0; // moderate distance thinning
    const WIDTH_MIN: f32 = 0.6;
    const WIDTH_MAX: f32 = 4.0;

    let Ok(cam) = cam_q.single() else {
        return;
    };
    let cam_pos = cam.translation();
    for (gt, vis_opt, mut outline) in &mut q {
        let size = vis_opt
            .map(|v| v.size.x.min(v.size.y).min(v.size.z).max(8.0))
            .unwrap_or(SIZE_REF);
        let size_factor = (size / SIZE_REF).powf(SIZE_POWER);
        let d = gt.translation().distance(cam_pos).max(1.0);
        let dist_factor = (ISO_DISTANCE / d).powf(DIST_POWER);
        let new_width = (BASE_WIDTH * size_factor * dist_factor).clamp(WIDTH_MIN, WIDTH_MAX);
        // Only touch the component when it meaningfully changes. Mutating
        // `OutlineVolume` flags it for render-world re-extraction; in a
        // turn-based game the camera is static most frames, so the width is
        // usually identical and the write is pure churn.
        if (outline.width - new_width).abs() > 1.0e-3 {
            outline.width = new_width;
        }
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
            // Low, cool ambient fill so cel shadows read deep and moody.
            AmbientLight {
                color: Color::srgb(0.6, 0.7, 1.0),
                brightness: 120.0,
                ..default()
            },
            // ---------- Phase 4: post-processing stack ----------
            // Filmic tonemap — crushes highlights and deepens shadows naturally.
            Tonemapping::AgX,
            // Restrained bloom around bright highlights / eventual emissives.
            Bloom { intensity: 0.08, ..Bloom::NATURAL },
            // Dark, cool atmospheric fog for depth/mood.
            DistanceFog {
                color: Color::srgb(0.06, 0.08, 0.13),
                falloff: FogFalloff::Exponential { density: 0.0005 },
                ..default()
            },
            // Cool, desaturated shadows + a touch of midtone contrast — the
            // signature adult-anime grade. Tune the per-section values to taste.
            ColorGrading {
                global: ColorGradingGlobal {
                    exposure: -0.05,
                    temperature: -0.06, // slightly cool overall
                    ..default()
                },
                shadows: ColorGradingSection {
                    saturation: 0.55,
                    contrast: 1.05,
                    ..default()
                },
                midtones: ColorGradingSection {
                    saturation: 0.90,
                    contrast: 1.10,
                    ..default()
                },
                highlights: ColorGradingSection {
                    saturation: 0.85,
                    contrast: 1.0,
                    ..default()
                },
            },
            // Contact shadows / AO crevices — adds depth and detail. SSAO
            // requires MSAA off (it samples the depth buffer directly).
            ScreenSpaceAmbientOcclusion::default(),
            Msaa::Off,
            DepthPrepass,
            NormalPrepass,
            // Vignette + film-grain fullscreen pass (post_fx.rs); tasteful
            // defaults for the dark-anime mood.
            crate::post_fx::PostFxSettings::ANIME_DEFAULT,
        ))
        .id()
}

/// Dev aid (only registered when the `ISO_SHOT` env var is set): after a short
/// delay, jump to the exploration world at a wide zoom and save one framebuffer
/// screenshot to `/tmp/iso_checkpoint.png`. No effect on normal runs.
pub fn debug_screenshot_once(
    mut commands: Commands,
    time: Res<Time>,
    mut game_state: ResMut<crate::core::GameState>,
    mut globals: ResMut<crate::core::Global_Variables>,
    mut rig: ResMut<CameraRig>,
    player_q: Query<&Transform, With<crate::core::Player>>,
    cam_q: Query<&Transform, (With<crate::core::MainCamera>, Without<crate::core::Player>)>,
    mut elapsed: Local<f32>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    // Force exploration + a fixed close zoom every frame (a stray wheel event in
    // the headless capture can otherwise drift the zoom).
    game_state.0 = crate::core::Game_State::Exploring;
    // Free-aim the camera at the isolated ToonTestSphere (north of spawn) so
    // nothing occludes it. world_origin = tile center (2048, 2048), sphere at +800 Y.
    // Frame both the test capsule (north, closer to the camera) and the player
    // cluster (south, farther) so distance-based outline thinning is comparable.
    globals.0.camera_locked = false;
    rig.focus = Vec2::new(2048.0, 2448.0);
    rig.zoom = 1100.0;
    rig.yaw = std::f32::consts::PI * 0.25; // default iso angle
    *elapsed += time.delta_secs();
    if *elapsed < 3.0 {
        return;
    }
    *done = true;
    let player = player_q.single().map(|t| t.translation).ok();
    let cam = cam_q.single().map(|t| t.translation).ok();
    info!(
        "ISO_SHOT: zoom={} player={:?} cam={:?}",
        rig.zoom, player, cam
    );
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
        .observe(bevy::render::view::screenshot::save_to_disk(
            "/tmp/iso_checkpoint.png",
        ));
    info!("ISO_SHOT: saved /tmp/iso_checkpoint.png");
}

/// Tighter orthographic zoom for the main-menu stage, so the cast reads as a
/// hero shot rather than tiny distant figures.
pub const MENU_VIEWPORT_HEIGHT: f32 = 210.0;

/// A camera for the main-menu 3D stage. Reuses the game's full toon + post
/// pipeline (via [`spawn_iso_camera`]) so the cast matches the in-game look,
/// then zooms in and brightens the ambient fill for a title-screen feel. The
/// returned entity keeps the default camera order/active flag — the menu plugin
/// tags it and manages `is_active`.
pub fn spawn_menu_stage_camera(commands: &mut Commands, focus: Vec3) -> Entity {
    let cam = spawn_iso_camera(commands, focus);
    commands.entity(cam).insert((
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: MENU_VIEWPORT_HEIGHT,
            },
            near: -ISO_DISTANCE * 2.0,
            far: ISO_DISTANCE * 2.0,
            ..OrthographicProjection::default_3d()
        }),
        // Warm, bright fill so the characters pop against the dark stage.
        AmbientLight {
            color: Color::srgb(0.90, 0.88, 1.0),
            brightness: 700.0,
            ..default()
        },
    ));
    cam
}

/// Insert the directional "sun" (shadow-casting) for the scene.
pub fn spawn_sun(commands: &mut Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadows_enabled: true,
            ..default()
        },
        // Cross-light the scene: the key light comes from the side relative to
        // the default iso camera (+x+y) so the shadow terminator falls across
        // visible surfaces — that's where cel banding reads. (Frontlighting from
        // the camera direction looks flat.)
        Transform::default().looking_to(Vec3::new(-0.85, 0.4, -0.75).normalize(), Vec3::Z),
    ));
}
