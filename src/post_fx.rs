//! Phase 4 polish: a fullscreen post-processing pass that adds a radial
//! vignette and animated film grain after Bevy's tonemapping. Inserted into
//! the 3D render graph between `Tonemapping` and `EndMainPassPostProcessing`.
//!
//! Attach [`PostFxSettings`] to the camera entity (the iso camera in
//! `render3d::spawn_iso_camera`) to enable the effect on that view.

use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::FullscreenShader;
use bevy::ecs::query::QueryItem;
use bevy::prelude::*;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_graph::{
    NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, FragmentState, MultisampleState, Operations, PipelineCache,
    PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
    Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, TextureFormat,
    TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice};
use bevy::render::view::ViewTarget;
use bevy::render::RenderApp;

const SHADER_PATH: &str = "shaders/post_fx.wgsl";

/// Per-camera settings. Attach as a component to the camera entity.
#[derive(Component, Default, Clone, Copy, ExtractComponent, ShaderType)]
pub struct PostFxSettings {
    /// 0 = no vignette, 1 = corners crushed to black.
    pub vignette_strength: f32,
    /// How gradual the vignette falloff is (0..0.5).
    pub vignette_softness: f32,
    /// 0 = no grain.
    pub grain_strength: f32,
    /// Wall-clock time in seconds; updated each frame by [`tick_post_fx_time`].
    pub time: f32,
}

impl PostFxSettings {
    /// A tasteful default for the dark-anime mood: a soft vignette and a hint
    /// of film grain.
    pub const ANIME_DEFAULT: Self = Self {
        vignette_strength: 0.55,
        vignette_softness: 0.35,
        grain_strength: 0.03,
        time: 0.0,
    };
}

/// Bumps the `time` field on every [`PostFxSettings`] each frame so the grain
/// noise re-seeds (a static seed looks like dust, not grain).
pub fn tick_post_fx_time(time: Res<Time>, mut q: Query<&mut PostFxSettings>) {
    let t = time.elapsed_secs();
    for mut s in &mut q {
        s.time = t;
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct PostFxLabel;

#[derive(Default)]
struct PostFxNode;

impl ViewNode for PostFxNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static PostFxSettings,
        &'static DynamicUniformIndex<PostFxSettings>,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (view_target, _settings, settings_index): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let pipeline_resource = world.resource::<PostFxPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_resource.pipeline_id)
        else {
            return Ok(());
        };
        let settings_uniforms = world.resource::<ComponentUniforms<PostFxSettings>>();
        let Some(settings_binding) = settings_uniforms.uniforms().binding() else {
            return Ok(());
        };

        // `post_process_write` ping-pongs the view's two textures so we read
        // the current contents and write into the other.
        let post_process = view_target.post_process_write();
        let bind_group = render_context.render_device().create_bind_group(
            "post_fx_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipeline_resource.layout),
            &BindGroupEntries::sequential((
                post_process.source,
                &pipeline_resource.sampler,
                settings_binding.clone(),
            )),
        );

        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("post_fx_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        render_pass.set_render_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[settings_index.index()]);
        render_pass.draw(0..3, 0..1);
        Ok(())
    }
}

/// Pipeline data. In Bevy 0.18 the pipeline's bind-group-layout slot takes a
/// [`BindGroupLayoutDescriptor`]; the actual `BindGroupLayout` is resolved
/// later via the pipeline cache when we create the bind group.
#[derive(Resource)]
struct PostFxPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

impl FromWorld for PostFxPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>().clone();
        let fullscreen_shader = world.resource::<FullscreenShader>().clone();
        let layout = BindGroupLayoutDescriptor::new(
            "post_fx_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    uniform_buffer::<PostFxSettings>(true),
                ),
            ),
        );
        let sampler = render_device.create_sampler(&SamplerDescriptor::default());
        let shader = world.resource::<AssetServer>().load(SHADER_PATH);
        let pipeline_id = world
            .resource_mut::<PipelineCache>()
            .queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("post_fx_pipeline".into()),
                layout: vec![layout.clone()],
                vertex: fullscreen_shader.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader,
                    entry_point: Some("fragment".into()),
                    targets: vec![Some(ColorTargetState {
                        format: TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                push_constant_ranges: vec![],
                zero_initialize_workgroup_memory: false,
            });
        Self {
            layout,
            sampler,
            pipeline_id,
        }
    }
}

pub struct PostFxPlugin;

impl Plugin for PostFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<PostFxSettings>::default(),
            UniformComponentPlugin::<PostFxSettings>::default(),
        ))
        .add_systems(Update, tick_post_fx_time);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_render_graph_node::<ViewNodeRunner<PostFxNode>>(Core3d, PostFxLabel)
            .add_render_graph_edges(
                Core3d,
                (Node3d::Tonemapping, PostFxLabel, Node3d::EndMainPassPostProcessing),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<PostFxPipeline>();
    }
}
