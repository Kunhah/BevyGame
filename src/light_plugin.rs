use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::*;
use bevy::shader::ShaderRef;
use bevy::mesh::{Indices, Mesh};
use bevy::asset::RenderAssetUsages;
use bevy_camera::{Camera2d, RenderTarget, visibility::RenderLayers};
use bevy_sprite_render::{Material2d, Material2dPlugin, MeshMaterial2d, AlphaMode2d};
use bytemuck::{Pod, Zeroable};
use bevy::window::WindowResized;

use crate::core::{Global_Variables, MainCamera, Player};

/// Marker for occluders
#[derive(Component)]
pub struct Occluder;

/// Marker for a parent entity that already spawned an occlusion mask child.
#[derive(Component)]
pub struct OcclusionMaskParent;

/// Marker for occlusion mask sprites.
#[derive(Component)]
pub struct OcclusionMask;

/// Marker for the offscreen occlusion camera.
#[derive(Component)]
pub struct OcclusionCamera;

/// Marker for the fullscreen light quad.
#[derive(Component)]
pub struct LightQuad;

/// Track the light quad size so we can resize on window changes.
#[derive(Component, Default)]
pub struct LightQuadSize(pub Vec2);

/// Tracks the current light parameters in world space for gameplay visibility checks.
#[derive(Resource, Default, Clone, Copy)]
pub struct LightState {
    pub world_pos: Vec2,
    pub radius: f32,
    pub intensity: f32,
    pub visibility: f32,
}

/// Entities with this component are hidden when light is below the threshold.
#[derive(Component, Clone, Copy)]
pub struct LightSensitive {
    pub threshold: f32,
}

/// Debug overlay marker for showing the occlusion texture.
#[derive(Component)]
pub struct OcclusionDebugSprite;

/// Debug marker showing computed light center in world space.
#[derive(Component)]
pub struct LightCenterMarker;

/// Debug settings for the light system.
#[derive(Resource, Default)]
pub struct LightDebugSettings {
    pub overlay: bool,
    pub mode: u8,
    pub marker: bool,
}

/// Occlusion target resource (texture + size)
#[derive(Resource)]
pub struct OcclusionTarget {
    pub image: Handle<Image>,
    pub size: UVec2,
}

/// White 1x1 image used for occlusion mask sprites.
#[derive(Resource)]
pub struct OcclusionMaskImage {
    pub image: Handle<Image>,
}

/// A simple POD uniform for the shader (packed for WGSL layout)
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, ShaderType)]
pub struct LightUniform {
    pub light_uv: Vec2,
    pub radius: f32,
    pub intensity: f32,
    pub color: Vec4,
    pub occlusion_size: Vec2,
    pub visibility: f32,
    pub debug_mode: f32,
}

const OCCLUSION_SCALE: f32 = 0.5;

impl Default for LightUniform {
    fn default() -> Self {
        Self {
            light_uv: Vec2::splat(0.5),
            // Keep a visible falloff by default.
            radius: 350.0,
            intensity: 3.0,
            color: Vec4::new(1.0, 0.95, 0.8, 1.0),
            occlusion_size: Vec2::new(512.0, 512.0),
            visibility: 1.0,
            debug_mode: 0.0,
        }
    }
}

/// Material: 1 texture binding + 1 uniform block
#[derive(Asset, AsBindGroup, Clone, TypePath)]
pub struct LightMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub occlusion_tex: Handle<Image>,

    #[uniform(2)]
    pub params: LightUniform,
}

impl Material2d for LightMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/light_plugin.wgsl".into()
    }

    fn depth_bias(&self) -> f32 {
        // Draw after most sprites even if z is the same.
        10000.0
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        // Blend so the black quad with alpha darkens the scene while keeping sprites visible.
        AlphaMode2d::Blend
    }
}

/// Create occlusion render target and camera
pub fn setup_occlusion_pass(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window>,
) {
    let size = windows
        .iter()
        .next()
        .map(|win| {
            let physical = win.resolution.physical_size().as_vec2() * OCCLUSION_SCALE;
            UVec2::new(
                physical.x.max(1.0) as u32,
                physical.y.max(1.0) as u32,
            )
        })
        .unwrap_or(UVec2::new(512, 512));

    let mut img = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("occlusion_map"),
            size: Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    img.resize(img.texture_descriptor.size);
    let image_handle = images.add(img);

    let mask = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let mask_handle = images.add(mask);

    commands.spawn((
        Camera2d,
        Camera {
            order: -10,
            target: RenderTarget::Image(image_handle.clone().into()),
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        RenderLayers::layer(1),
        OcclusionCamera,
        Name::new("OcclusionCamera"),
    ));

    commands.insert_resource(OcclusionTarget {
        image: image_handle,
        size,
    });
    commands.insert_resource(OcclusionMaskImage {
        image: mask_handle,
    });
    commands.insert_resource(LightDebugSettings::default());
    commands.insert_resource(LightState::default());

    info!(
        "setup_occlusion_pass: target size={}x{}",
        size.x, size.y
    );
}

/// Helper: make a simple quad mesh sized to `size`
fn make_quad_mesh(size: Vec2) -> Mesh {
    let hw = size.x * 0.5;
    let hh = size.y * 0.5;
    let positions = vec![
        [-hw, -hh, 0.0],
        [hw, -hh, 0.0],
        [hw, hh, 0.0],
        [-hw, hh, 0.0],
    ];
    let normals = vec![[0.0, 0.0, 1.0]; 4];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let indices = vec![0u32, 2, 1, 0, 3, 2];

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Spawn a full-screen quad with LightMaterial (one light)
pub fn spawn_light_quad(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<LightMaterial>>,
    occ: Option<Res<OcclusionTarget>>,
    windows: Query<&Window>,
    q_existing: Query<(), With<LightQuad>>,
) {
    if !q_existing.is_empty() {
        return;
    }
    let Some(occ) = occ else {
        info!("spawn_light_quad: OcclusionTarget not ready yet, skipping this frame");
        // Occlusion target not ready yet; try again next frame.
        return;
    };
    let Some(win) = windows.iter().next() else {
        info!("spawn_light_quad: No window found, skipping");
        return;
    };
    // Make the quad very large so it always covers the camera frustum regardless of DPI/scale.
    let mut size = win.resolution.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        size = Vec2::splat(2048.0);
    }

    let mesh_handle = meshes.add(make_quad_mesh(size));

    let mut params = LightUniform::default();
    params.occlusion_size = Vec2::new(occ.size.x as f32, occ.size.y as f32);

    // info!("light params radius={} intensity={} uv={:?}", params.radius, params.intensity, params.light_uv);

    let material = mats.add(LightMaterial {
        occlusion_tex: occ.image.clone(),
        params,
    });

    // info!(
    //     "spawn_light_quad: spawned light mesh {:?}, material {:?} (radius={}, intensity={}, uv={:?})",
    //     mesh_handle,
    //     material,
    //     params.radius,
    //     params.intensity,
    //     params.light_uv
    // );

    commands.spawn((
        Mesh2d(mesh_handle),
        MeshMaterial2d(material),
        Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
        Visibility::Visible,
        InheritedVisibility::default(),
        ViewVisibility::default(),
        RenderLayers::layer(0), // explicitly visible to the main camera
        LightQuad,
        LightQuadSize(size),
        Name::new("LightFullscreenQuad"),
    ));

    info!(
        "spawn_light_quad: spawned light quad size={:?} occlusion_size={:?}",
        size, params.occlusion_size
    );
}

/// Keep the occlusion camera aligned with the main camera so occlusion matches the view.
pub fn sync_occlusion_camera(
    q_main: Query<&Transform, (With<MainCamera>, Without<OcclusionCamera>)>,
    mut q_occ: Query<&mut Transform, With<OcclusionCamera>>,
) {
    let Ok(main_tf) = q_main.single() else {
        return;
    };
    for mut occ_tf in &mut q_occ {
        *occ_tf = *main_tf;
    }
}

/// Resize the light quad when the window size changes to avoid stretching.
pub fn resize_light_quad(
    mut resized: EventReader<WindowResized>,
    windows: Query<&Window>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut q_quad: Query<(&Mesh2d, &mut LightQuadSize), With<LightQuad>>,
) {
    if resized.is_empty() {
        return;
    }
    resized.clear();
    let Some(win) = windows.iter().next() else { return; };
    let size = win.resolution.size();
    if size.x <= 0.0 || size.y <= 0.0 { return; }
    for (mesh_handle, mut stored) in &mut q_quad {
        if (stored.0 - size).length_squared() < 0.01 {
            continue;
        }
        if let Some(mesh) = meshes.get_mut(&mesh_handle.0) {
            *mesh = make_quad_mesh(size);
            stored.0 = size;
        }
    }
}

/// Resize the occlusion render target when the window size changes.
pub fn resize_occlusion_target(
    mut resized: EventReader<WindowResized>,
    windows: Query<&Window>,
    mut images: ResMut<Assets<Image>>,
    mut occ: ResMut<OcclusionTarget>,
    mut q_cam: Query<&mut Camera, With<OcclusionCamera>>,
    mut mats: ResMut<Assets<LightMaterial>>,
) {
    if resized.is_empty() {
        return;
    }
    resized.clear();
    let Some(win) = windows.iter().next() else { return; };
    let physical = win.resolution.physical_size().as_vec2() * OCCLUSION_SCALE;
    let size = UVec2::new(
        physical.x.max(1.0) as u32,
        physical.y.max(1.0) as u32,
    );
    if size.x == 0 || size.y == 0 {
        return;
    }

    let mut img = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("occlusion_map"),
            size: Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    img.resize(img.texture_descriptor.size);
    let image_handle = images.add(img);

    occ.image = image_handle.clone();
    occ.size = size;

    for mut cam in &mut q_cam {
        cam.target = RenderTarget::Image(image_handle.clone().into());
    }

    for (_handle, mat) in mats.iter_mut() {
        mat.occlusion_tex = image_handle.clone();
        mat.params.occlusion_size = Vec2::new(size.x as f32, size.y as f32);
    }
}

/// Keep the light quad centered on the camera so it always covers the view.
pub fn sync_light_quad(
    q_main: Query<&Transform, (With<MainCamera>, Without<LightQuad>)>,
    mut q_quad: Query<&mut Transform, With<LightQuad>>,
) {
    let Ok(main_tf) = q_main.single() else {
        return;
    };
    for mut quad_tf in &mut q_quad {
        let z = quad_tf.translation.z;
        quad_tf.translation = Vec3::new(main_tf.translation.x, main_tf.translation.y, z);
    }
}

/// Ensure every occluder has a white mask sprite visible only to the occlusion camera.
pub fn ensure_occlusion_masks(
    mut commands: Commands,
    mask: Res<OcclusionMaskImage>,
    q_occluders: Query<Entity, (With<Occluder>, Without<OcclusionMaskParent>)>,
) {
    let mut spawned = 0usize;
    for entity in &q_occluders {
        commands.entity(entity).insert(OcclusionMaskParent).with_children(|parent| {
            parent.spawn((
                Sprite {
                    image: mask.image.clone(),
                    custom_size: Some(Vec2::splat(32.0)),
                    ..default()
                },
                Transform::IDENTITY,
                RenderLayers::layer(1),
                OcclusionMask,
                Name::new("OcclusionMask"),
            ));
        });
        spawned += 1;
    }
    if spawned > 0 {
        info!("ensure_occlusion_masks: spawned {} mask(s)", spawned);
    }
}

/// Toggle a debug overlay that shows the occlusion render target.
pub fn toggle_debug_overlay(
    mut commands: Commands,
    input: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<LightDebugSettings>,
    occ: Option<Res<OcclusionTarget>>,
    q_existing: Query<Entity, With<OcclusionDebugSprite>>,
) {
    if !input.just_pressed(KeyCode::KeyO) {
        return;
    }
    settings.overlay = !settings.overlay;
    info!("Light debug overlay: {}", if settings.overlay { "ON" } else { "OFF" });

    let Some(occ) = occ else {
        warn!("toggle_debug_overlay: OcclusionTarget not ready yet");
        return;
    };

    if settings.overlay {
        if q_existing.is_empty() {
            commands.spawn((
                Sprite {
                    image: occ.image.clone(),
                    custom_size: Some(Vec2::splat(160.0)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(-300.0, 200.0, -5.0)),
                RenderLayers::layer(0),
                OcclusionDebugSprite,
                Name::new("OcclusionDebugSprite"),
            ));
        } else {
            for entity in &q_existing {
                commands.entity(entity).insert(Visibility::Visible);
            }
        }
    } else {
        for entity in &q_existing {
            commands.entity(entity).insert(Visibility::Hidden);
        }
    }
}

/// Toggle forcing the light quad to a solid overlay for debugging.
pub fn toggle_debug_solid(
    input: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<LightDebugSettings>,
) {
    if !input.just_pressed(KeyCode::KeyK) {
        return;
    }
    settings.mode = (settings.mode + 1) % 5;
    info!(
        "Light debug mode: {}",
        match settings.mode {
            0 => "normal",
            1 => "solid",
            2 => "uv-gradient",
            3 => "uv-light",
            _ => "radial",
        }
    );
}

/// Toggle the world-space marker for the light center.
pub fn toggle_debug_marker(
    input: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<LightDebugSettings>,
) {
    if !input.just_pressed(KeyCode::KeyM) {
        return;
    }
    settings.marker = !settings.marker;
    info!("Light debug marker: {}", if settings.marker { "ON" } else { "OFF" });
}

/// Keep the debug overlay near the camera so it's always visible.
pub fn sync_debug_overlay(
    q_main: Query<&Transform, (With<MainCamera>, Without<OcclusionDebugSprite>)>,
    mut q_debug: Query<&mut Transform, With<OcclusionDebugSprite>>,
) {
    let Ok(main_tf) = q_main.single() else {
        return;
    };
    for mut debug_tf in &mut q_debug {
        debug_tf.translation.x = main_tf.translation.x - 300.0;
        debug_tf.translation.y = main_tf.translation.y + 200.0;
    }
}

/// Update LightMaterial params per frame (distance culling / visibility)
pub fn update_light_params(
    mut mats: ResMut<Assets<LightMaterial>>,
    occ: Res<OcclusionTarget>,
    debug: Res<LightDebugSettings>,
    globals: Res<Global_Variables>,
    mut light_state: ResMut<LightState>,
    mask: Option<Res<OcclusionMaskImage>>,
    mut commands: Commands,
    mut q_marker: Query<&mut Transform, With<LightCenterMarker>>,
    q_cam: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    q_player: Query<&GlobalTransform, With<Player>>,
    time: Res<Time>,
    mut log_timer: Local<Timer>,
) {
    if log_timer.duration().as_secs_f32() == 0.0 {
        *log_timer = Timer::from_seconds(1.0, TimerMode::Repeating);
    }
    log_timer.tick(time.delta());
    let Ok((cam, cam_xform)) = q_cam.single() else {
        return;
    };
    let Some(player_tf) = q_player.iter().next() else {
        return;
    };

    let player_pos = player_tf.translation().truncate();
    let cam_pos = cam_xform.translation().truncate();
    let viewport_size = cam
        .logical_viewport_size()
        .unwrap_or_else(|| Vec2::new(occ.size.x as f32, occ.size.y as f32));

    for (_handle, mat) in mats.iter_mut() {
        let mut params = mat.params;
        // Shader distances are in occlusion-texture pixels.
        params.occlusion_size = Vec2::new(occ.size.x as f32, occ.size.y as f32);

        // Project player position into the camera viewport (0..1 UV space).
        let mut viewport_ok = true;
        if globals.0.camera_locked {
            params.light_uv = Vec2::splat(0.5);
        } else {
            let viewport_res = cam.world_to_viewport(cam_xform, player_pos.extend(0.0));
            viewport_ok = viewport_res.is_ok();
            let viewport_pos = viewport_res.unwrap_or(viewport_size * 0.5);
            let mut uv = (viewport_pos / viewport_size).clamp(Vec2::ZERO, Vec2::ONE);
            // Bevy's mesh UVs are Y-up; flip to match screen-space viewport Y.
            uv.y = 1.0 - uv.y;
            params.light_uv = uv;
        }

        // Map the UV back into world space around the camera for occluder checks.
        let light_world_pos = cam
            .viewport_to_world_2d(cam_xform, params.light_uv * viewport_size)
            .unwrap_or(cam_pos);

        // Ensure radius/intensity have sane defaults even if the material was created with zeros.
        if params.radius <= 0.0 {
            params.radius = 800.0;
        }
        if params.intensity <= 0.0 {
            params.intensity = 1.5;
        }

        let dist = player_pos.distance(light_world_pos);

        let max_visible_distance = params.radius;
        if dist > max_visible_distance {
            params.visibility = 0.0;
            mat.params = params;
            continue;
        }

        params.visibility = 1.0;
        params.debug_mode = debug.mode as f32;
        mat.params = params;

        light_state.world_pos = light_world_pos;
        light_state.radius = params.radius;
        light_state.intensity = params.intensity;
        light_state.visibility = params.visibility;

        if debug.marker {
            if let Ok(mut marker_tf) = q_marker.single_mut() {
                marker_tf.translation.x = light_world_pos.x;
                marker_tf.translation.y = light_world_pos.y;
                marker_tf.translation.z = 500.0;
            } else if let Some(mask) = mask.as_ref() {
                commands.spawn((
                    Sprite {
                        image: mask.image.clone(),
                        custom_size: Some(Vec2::splat(10.0)),
                        color: Color::srgb(0.0, 1.0, 1.0),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(
                        light_world_pos.x,
                        light_world_pos.y,
                        500.0,
                    )),
                    RenderLayers::layer(0),
                    LightCenterMarker,
                    Name::new("LightCenterMarker"),
                ));
            }
        }

        if debug.mode == 3 && log_timer.just_finished() {
            info!(
                "light_debug: uv={:?} viewport_size={:?} player_pos={:?} cam_pos={:?} world_to_viewport_ok={}",
                params.light_uv,
                viewport_size,
                player_pos,
                cam_pos,
                viewport_ok
            );
        }

        // info!(
        //     "update_light_params: light_uv={:?} radius={} intensity={} blocked={} visibility={}",
        //     params.light_uv, params.radius, params.intensity, blocked, params.visibility
        // );
    }
}

/// Hide light-sensitive entities when they are in shadow or out of range.
pub fn apply_light_visibility(
    light: Res<LightState>,
    q_occluders: Query<&Transform, With<Occluder>>,
    mut q_entities: Query<(&Transform, &LightSensitive, &mut Visibility)>,
) {
    let light_pos = light.world_pos;
    if light.radius <= 0.0 || light.intensity <= 0.0 || light.visibility <= 0.0 {
        for (_, _, mut vis) in &mut q_entities {
            *vis = Visibility::Hidden;
        }
        return;
    }

    for (tf, sensitive, mut vis) in &mut q_entities {
        let pos = tf.translation.truncate();
        let dist = pos.distance(light_pos);
        if dist > light.radius {
            *vis = Visibility::Hidden;
            continue;
        }

        let mut blocked = false;
        for occ_tf in &q_occluders {
            let o_pos = occ_tf.translation.truncate();
            let size = Vec2::splat(32.0);
            let rect = Rect::from_center_size(o_pos, size);
            if line_intersects_rect(pos, light_pos, rect) {
                blocked = true;
                break;
            }
        }

        let light_factor = if blocked {
            0.0
        } else {
            let falloff = 1.0 - (dist / light.radius).clamp(0.0, 1.0);
            falloff * light.intensity * light.visibility
        };

        *vis = if light_factor < sensitive.threshold {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
}

/// Log occluder movement to verify they are static in world space.
pub fn log_occluder_motion(
    q_occluders: Query<(Entity, &GlobalTransform), With<Occluder>>,
    time: Res<Time>,
    mut timer: Local<Timer>,
    mut last_positions: Local<std::collections::HashMap<Entity, Vec3>>,
) {
    if timer.duration().as_secs_f32() == 0.0 {
        *timer = Timer::from_seconds(1.0, TimerMode::Repeating);
    }
    timer.tick(time.delta());
    if !timer.just_finished() {
        return;
    }

    let mut moved = 0usize;
    let mut max_delta = 0.0f32;
    for (entity, tf) in &q_occluders {
        let pos = tf.translation();
        if let Some(prev) = last_positions.get(&entity) {
            let delta = prev.distance(pos);
            if delta > 0.01 {
                moved += 1;
                if delta > max_delta {
                    max_delta = delta;
                }
            }
        }
        last_positions.insert(entity, pos);
    }

    if moved > 0 {
        info!(
            "occluder_motion: moved={} max_delta={:.3}",
            moved, max_delta
        );
    } else {
        info!("occluder_motion: no movement detected");
    }
}

fn line_intersects_rect(a: Vec2, b: Vec2, rect: Rect) -> bool {
    let corners = [
        rect.min,
        Vec2::new(rect.max.x, rect.min.y),
        rect.max,
        Vec2::new(rect.min.x, rect.max.y),
    ];
    let edges = [(0usize, 1usize), (1, 2), (2, 3), (3, 0)];
    for (i1, i2) in edges.iter() {
        if lines_intersect(a, b, corners[*i1], corners[*i2]) {
            return true;
        }
    }
    false
}

fn lines_intersect(p1: Vec2, p2: Vec2, q1: Vec2, q2: Vec2) -> bool {
    let r = p2 - p1;
    let s = q2 - q1;
    let denom = r.perp_dot(s);
    if denom.abs() < f32::EPSILON {
        return false;
    }
    let t = (q1 - p1).perp_dot(s) / denom;
    let u = (q1 - p1).perp_dot(r) / denom;
    t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0
}

/// Plugin
pub struct LightPlugin;

impl Plugin for LightPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<LightMaterial>::default())
            .add_systems(Startup, setup_occlusion_pass)
            .add_systems(
                Update,
                (
                    spawn_light_quad,
                    sync_occlusion_camera,
                    resize_light_quad,
                    resize_occlusion_target,
                    sync_light_quad,
                    ensure_occlusion_masks,
                    toggle_debug_overlay,
                    toggle_debug_solid,
                    toggle_debug_marker,
                    sync_debug_overlay,
                    update_light_params,
                    apply_light_visibility,
                    log_occluder_motion,
                ),
            );
    }
}
