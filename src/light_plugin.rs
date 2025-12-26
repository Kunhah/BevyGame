use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::*;
use bevy::shader::ShaderRef;
use bevy::mesh::{Indices, Mesh};
use bevy::asset::RenderAssetUsages;
use bevy_camera::{Camera2d, RenderTarget, visibility::RenderLayers};
use bevy_sprite_render::{Material2d, Material2dPlugin, MeshMaterial2d, AlphaMode2d};
use bytemuck::{Pod, Zeroable};

use crate::core::Player;

/// Marker for occluders
#[derive(Component)]
pub struct Occluder;

/// Occlusion target resource (texture + size)
#[derive(Resource)]
pub struct OcclusionTarget {
    pub image: Handle<Image>,
    pub size: UVec2,
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
    pub _pad: f32,
}

impl Default for LightUniform {
    fn default() -> Self {
        Self {
            light_uv: Vec2::splat(0.5),
            // Make the light cover most of the screen so the scene is visible by default.
            radius: 1200.0,
            intensity: 1.5,
            color: Vec4::new(1.0, 0.95, 0.8, 1.0),
            occlusion_size: Vec2::new(512.0, 512.0),
            visibility: 1.0,
            _pad: 0.0,
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

    fn alpha_mode(&self) -> AlphaMode2d {
        // Blend so the black quad with alpha darkens the scene while keeping sprites visible.
        AlphaMode2d::Blend
    }
}

/// Create occlusion render target and camera
pub fn setup_occlusion_pass(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let size = UVec2::new(512, 512);

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

    commands.spawn((
        Camera2d,
        Camera {
            order: -10,
            target: RenderTarget::Image(image_handle.clone().into()),
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        RenderLayers::layer(1),
        Name::new("OcclusionCamera"),
    ));

    commands.insert_resource(OcclusionTarget {
        image: image_handle,
        size,
    });
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
) {
    let Some(occ) = occ else {
        info!("spawn_light_quad: OcclusionTarget not ready yet, skipping this frame");
        // Occlusion target not ready yet; try again next frame.
        return;
    };
    let Some(_win) = windows.iter().next() else {
        info!("spawn_light_quad: No window found, skipping");
        return;
    };
    // Make the quad very large so it always covers the camera frustum regardless of DPI/scale.
    let size = Vec2::splat(32.0);

    let mesh_handle = meshes.add(make_quad_mesh(size));

    let mut params = LightUniform::default();
    params.occlusion_size = Vec2::new(occ.size.x as f32, occ.size.y as f32);

    info!("light params radius={} intensity={} uv={:?}", params.radius, params.intensity, params.light_uv);

    let material = mats.add(LightMaterial {
        occlusion_tex: occ.image.clone(),
        params,
    });

    info!(
        "spawn_light_quad: spawned light mesh {:?}, material {:?} (radius={}, intensity={}, uv={:?})",
        mesh_handle,
        material,
        params.radius,
        params.intensity,
        params.light_uv
    );

    commands.spawn((
        Mesh2d(mesh_handle),
        MeshMaterial2d(material),
        Transform::from_translation(Vec3::new(0.0, 0.0, 999.0)),
        RenderLayers::layer(0), // explicitly visible to the main camera
        Name::new("LightFullscreenQuad"),
    ));
}

/// Update LightMaterial params per frame (distance culling / visibility)
pub fn update_light_params(
    mut mats: ResMut<Assets<LightMaterial>>,
    occ: Res<OcclusionTarget>,
    q_cam: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    q_player: Query<&GlobalTransform, With<Player>>,
    q_occluders: Query<&Transform, With<Occluder>>,
) {
    let Some((cam, cam_xform)) = q_cam.iter().next() else {
        return;
    };
    let Some(player_tf) = q_player.iter().next() else {
        return;
    };

    let player_pos = player_tf.translation().truncate();
    let cam_pos = cam_xform.translation().truncate();
    let viewport_scale = Vec2::new(occ.size.x as f32, occ.size.y as f32);

    for (_handle, mat) in mats.iter_mut() {
        let mut params = mat.params;

        // Project player position relative to camera into UV space of the occlusion target.
        let rel = player_pos - cam_pos;
        let uv = Vec2::new(rel.x / viewport_scale.x + 0.5, rel.y / viewport_scale.y + 0.5);
        params.light_uv = uv.clamp(Vec2::ZERO, Vec2::ONE);

        // Map the UV back into world space around the camera for occluder checks.
        let light_world_pos = cam_pos + (params.light_uv - Vec2::splat(0.5)) * viewport_scale;

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

        let mut blocked = false;
        for occluder_tf in q_occluders.iter() {
            let o_pos = occluder_tf.translation.truncate();
            let size = Vec2::splat(32.0);
            let rect = Rect::from_center_size(o_pos, size);
            if line_intersects_rect(player_pos, light_world_pos, rect) {
                blocked = true;
                break;
            }
        }

        params.visibility = if blocked { 0.0 } else { 1.0 };
        mat.params = params;

        info!(
            "update_light_params: light_uv={:?} radius={} intensity={} blocked={} visibility={}",
            params.light_uv, params.radius, params.intensity, blocked, params.visibility
        );
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
            .add_systems(Startup, (setup_occlusion_pass, spawn_light_quad).chain())
            .add_systems(Update, update_light_params);
    }
}
