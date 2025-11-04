// src/light_plugin.rs
use bevy::prelude::*;
use bevy::render::render_resource::*;
use bevy::sprite::{Material2d, Material2dPlugin, MaterialMesh2dBundle};
use bevy::render::camera::RenderTarget;
use bevy::render::view::RenderLayers;
use bytemuck::{Pod, Zeroable};
use bevy::reflect::TypePath;

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
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LightUniform {
    pub light_uv: [f32; 2],     // 8
    pub radius: f32,            // 12
    pub intensity: f32,         // 16
    pub color: [f32; 4],        // 32
    pub occlusion_size: [f32; 2],// 40
    pub visibility: f32,        // 44
    pub _pad: f32,              // 48 -> multiple of 16
}

impl Default for LightUniform {
    fn default() -> Self {
        Self {
            light_uv: [0.5, 0.5],
            radius: 150.0,
            intensity: 1.2,
            color: [1.0, 0.95, 0.8, 1.0],
            occlusion_size: [512.0, 512.0],
            visibility: 1.0,
            _pad: 0.0,
        }
    }
}

/// Material: 1 texture binding + 1 uniform block
#[derive(Asset)]
#[derive(AsBindGroup, TypeUuid, Clone, TypePath)]
#[uuid = "a8cbbf12-8e28-44a1-9b9b-15b9c0a0a1a7"]
pub struct LightMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub occlusion_tex: Handle<Image>,

    #[uniform(2)]
    pub params: LightUniform,
}

impl Material2d for LightMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(HANDLE_LIGHT_WGSL.clone())
    }
}

// Shader handle constant
pub const HANDLE_LIGHT_WGSL: Handle<Shader> =
    Handle::weak_from_u64(Shader::TYPE_UUID, 0xF00DF00D);

// add shader asset from WGSL file embedded at compile time
pub fn add_light_shader(mut shaders: ResMut<Assets<Shader>>) {
    // adjust path if your file is located elsewhere
    let wgsl_src = include_str!("../shaders/light_plugin.wgsl");
    shaders.set(HANDLE_LIGHT_WGSL.clone(), Shader::from_wgsl(wgsl_src));
}

/// Create occlusion render target and camera
pub fn setup_occlusion_pass(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
) {
    let size = UVec2::new(512, 512);

    // Use RGBA8 for broad compatibility
    let mut img = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("occlusion_map"),
            size: Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    img.resize(img.texture_descriptor.size); // allocate pixel data
    let image_handle = images.add(img);

    commands.spawn((
        Camera2dBundle {
            camera: Camera {
                order: -10,
                target: RenderTarget::Image(image_handle.clone().into()),
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
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
    // positions (xy), z = 0
    let hw = size.x * 0.5;
    let hh = size.y * 0.5;
    let positions = vec![
        [-hw, -hh, 0.0],
        [ hw, -hh, 0.0],
        [ hw,  hh, 0.0],
        [-hw,  hh, 0.0],
    ];
    let normals = vec![[0.0, 0.0, 1.0]; 4];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let indices = vec![0u32, 2, 1, 0, 3, 2];

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.set_indices(Some(Indices::U32(indices)));
    mesh
}

/// Spawn a full-screen quad with LightMaterial (one light)
pub fn spawn_light_quad(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<LightMaterial>>,
    occ: Res<OcclusionTarget>,
    windows: Query<&Window>,
) {
    let win = windows.single().unwrap();
    let size = Vec2::new(win.width(), win.height());

    let mesh_handle = meshes.add(make_quad_mesh(size));

    let mut params = LightUniform::default();
    params.occlusion_size = [occ.size.x as f32, occ.size.y as f32];

    let material = mats.add(LightMaterial {
        occlusion_tex: occ.image.clone(),
        params,
    });

    commands.spawn((
        MaterialMesh2dBundle {
            mesh: mesh_handle.into(),
            material,
            transform: Transform::from_translation(Vec3::new(0.0, 0.0, 999.0)),
            ..default()
        },
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
    let (cam, _cam_xform) = match q_cam.get_single() {
        Ok(v) => v,
        Err(_) => return,
    };
    let player_tf = match q_player.get_single() {
        Ok(v) => v,
        Err(_) => return,
    };

    let player_pos = player_tf.translation.truncate();

    for (_handle, mat) in mats.iter_mut() {
        let mut params = mat.params;

        // approximated light world pos (depends on your occlusion camera mapping)
        let light_world_pos = Vec2::new(
            params.light_uv[0] * occ.size.x as f32 - occ.size.x as f32 / 2.0,
            params.light_uv[1] * occ.size.y as f32 - occ.size.y as f32 / 2.0,
        );

        let dist = player_pos.distance(light_world_pos);

        let max_visible_distance = 600.0;
        if dist > max_visible_distance {
            params.visibility = 0.0;
            mat.params = params;
            continue;
        }

        // quick line-of-sight test
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
            .add_systems(Startup, add_light_shader)
            .add_systems(Startup, spawn_light_quad);
    }
}
