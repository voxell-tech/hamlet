//! A plugin's pass over the image of one of its cameras: its checked
//! WGSL run over the whole view after the camera draws and before its
//! tonemapping, in Bevy's 3D schedule of that camera only.

use std::collections::HashMap;

use bevy::app::App;
use bevy::asset::{AssetId, Handle};
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::Remove;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::math::Vec4;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{
    ExtractComponent, ExtractComponentPlugin,
};
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, uniform_buffer_sized,
};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor,
    BindGroupLayoutEntries, BufferInitDescriptor, BufferUsages,
    CachedRenderPipelineId, ColorTargetState, ColorWrites,
    FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, Sampler, SamplerBindingType,
    SamplerDescriptor, ShaderStages, TextureFormat,
    TextureSampleType,
};
use bevy::render::renderer::{
    RenderContext, RenderDevice, ViewQuery,
};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::Shader;
use wgpu::RenderPassTimestampWrites;

use crate::compute::{RenderClock, param_bytes};
use crate::guard::{Throttled, TimedSlots};
use crate::shader::PARAMS;

pub(crate) fn plugin(app: &mut App) {
    if app.get_sub_app(RenderApp).is_none() {
        return;
    }
    // An observer, as extracting the pass takes its remove hook.
    app.add_plugins(ExtractComponentPlugin::<ScenePass>::default())
        .add_observer(give_slot);
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .init_resource::<PassPipelines>()
        .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
        .add_systems(
            Core3d,
            run.in_set(Core3dSystems::PostProcess)
                .before(tonemapping),
        );
}

/// On a scene's camera: the pass its owner runs over its image.
#[derive(Component, ExtractComponent, Clone, Debug)]
pub struct ScenePass {
    pub owner: bevy::ecs::entity::Entity,
    pub shader: Handle<Shader>,
    pub params: [Vec4; PARAMS],
    /// Its slot of the clock, if it is timed.
    pub slot: Option<u32>,
}

fn give_slot(
    removed: On<Remove, ScenePass>,
    passes: Query<&ScenePass>,
    slots: Option<ResMut<TimedSlots>>,
) {
    if let Ok(ScenePass {
        slot: Some(slot), ..
    }) = passes.get(removed.entity)
        && let Some(mut slots) = slots
    {
        slots.give(*slot);
    }
}

/// On a view with a pass: the pipeline it runs.
#[derive(Component, Clone, Copy, Debug)]
struct PassPipeline(CachedRenderPipelineId);

#[derive(Resource, Default)]
struct PassPipelines {
    sampler: Option<Sampler>,
    ids: HashMap<
        (AssetId<Shader>, TextureFormat),
        CachedRenderPipelineId,
    >,
}

fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "hamlet_pass",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float {
                    filterable: true,
                }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer_sized(false, None),
                uniform_buffer_sized(false, None),
            ),
        ),
    )
}

/// Gives each view with a pass the pipeline of its shader for its
/// target's format.
fn prepare(
    views: Query<
        (Entity, &ExtractedView, &ScenePass),
        With<ExtractedCamera>,
    >,
    pipeline_cache: Res<PipelineCache>,
    fullscreen: Res<FullscreenShader>,
    mut pipelines: ResMut<PassPipelines>,
    mut commands: Commands,
) {
    for (view, extracted, pass) in &views {
        let format = extracted.target_format;
        let id = *pipelines
            .ids
            .entry((pass.shader.id(), format))
            .or_insert_with(|| {
                pipeline_cache.queue_render_pipeline(
                    RenderPipelineDescriptor {
                        label: Some("hamlet_pass".into()),
                        layout: vec![layout()],
                        vertex: fullscreen.to_vertex_state(),
                        fragment: Some(FragmentState {
                            shader: pass.shader.clone(),
                            targets: vec![Some(ColorTargetState {
                                format,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            })],
                            ..FragmentState::default()
                        }),
                        ..RenderPipelineDescriptor::default()
                    },
                )
            });
        commands.entity(view).insert(PassPipeline(id));
    }
}

/// Runs the pass of the view being drawn over its image, timed where
/// the GPU can, unless its owner is over its GPU time.
fn run(
    view: ViewQuery<(
        &ViewTarget,
        &ExtractedView,
        &ScenePass,
        &PassPipeline,
    )>,
    pipeline_cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    throttled: Res<Throttled>,
    clock: Res<RenderClock>,
    time: Res<bevy::time::Time>,
    mut pipelines: ResMut<PassPipelines>,
    mut ctx: RenderContext,
) {
    let (target, extracted, pass, pipeline) = view.into_inner();
    if throttled.0.contains(&pass.owner) {
        return;
    }
    let Some(pipeline) =
        pipeline_cache.get_render_pipeline(pipeline.0)
    else {
        return;
    };
    let sampler = pipelines
        .sampler
        .get_or_insert_with(|| {
            device.create_sampler(&SamplerDescriptor::default())
        })
        .clone();
    let params =
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("hamlet_pass_params"),
            contents: &param_bytes(&pass.params),
            usage: BufferUsages::UNIFORM,
        });
    let size = extracted.viewport.as_vec4();
    let size = (size.z, size.w);
    let mut frame = Vec::with_capacity(16);
    for value in [
        size.0,
        size.1,
        time.elapsed_secs_wrapped(),
        time.delta_secs(),
    ] {
        frame.extend_from_slice(&value.to_le_bytes());
    }
    let frame =
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("hamlet_pass_frame"),
            contents: &frame,
            usage: BufferUsages::UNIFORM,
        });
    let post = target.post_process_write();
    let bind_group = device.create_bind_group(
        "hamlet_pass",
        &pipeline_cache.get_bind_group_layout(&layout()),
        &BindGroupEntries::sequential((
            post.source,
            &sampler,
            params.as_entire_binding(),
            frame.as_entire_binding(),
        )),
    );
    let timing = pass
        .slot
        .filter(|_| clock.buffer.is_some())
        .zip(clock.set.as_ref());
    let encoder = ctx.command_encoder();
    {
        let mut drawn =
            encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("hamlet_pass"),
                color_attachments: &[Some(
                    RenderPassColorAttachment {
                        view: post.destination,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations::default(),
                    },
                )],
                depth_stencil_attachment: None,
                timestamp_writes: timing.map(|(slot, set)| {
                    RenderPassTimestampWrites {
                        query_set: set,
                        beginning_of_pass_write_index: Some(slot * 2),
                        end_of_pass_write_index: Some(slot * 2 + 1),
                    }
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
        drawn.set_pipeline(pipeline);
        drawn.set_bind_group(0, &bind_group, &[]);
        drawn.draw(0..3, 0..1);
    }
    if let (Some((slot, set)), Some(buffer)) = (timing, &clock.buffer)
    {
        let (queries, offset) = RenderClock::place(slot);
        encoder.resolve_query_set(set, queries, buffer, offset);
    }
}
