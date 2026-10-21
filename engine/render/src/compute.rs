//! A plugin's compute: its checked WGSL run over its buffers, each
//! frame it is asked to, before the cameras draw. The workgroups of
//! one owner in a frame are capped, an owner over its GPU time waits,
//! and each run is timed where the GPU has timestamps.

use std::collections::{HashMap, HashSet};

use bevy::app::App;
use bevy::asset::{AssetId, Assets, Handle};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::DeferredWorld;
use bevy::math::Vec4;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_sized, uniform_buffer_sized,
};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor,
    BindGroupLayoutEntries, Buffer, BufferDescriptor,
    BufferInitDescriptor, BufferUsages, CachedComputePipelineId,
    CommandEncoderDescriptor, ComputePassDescriptor,
    ComputePipelineDescriptor, PipelineCache, ShaderStages,
    WgpuFeatures,
};
use bevy::render::renderer::{
    RenderDevice, RenderQueue, render_system,
};
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp, RenderStartup,
    RenderSystems,
};
use bevy::shader::Shader;
use bevy::time::Time;
use hamlet_engine_core::PluginBudgets;
use wgpu::{
    ComputePassTimestampWrites, QuerySet, QuerySetDescriptor,
    QueryType,
};

use crate::guard::{GpuClock, SLOT_BYTES, Throttled, TimedSlots};
use crate::shader::PARAMS;
use crate::viewport::RenderTuning;

pub(crate) fn plugin(app: &mut App) {
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .init_resource::<Jobs>()
        .init_resource::<RenderClock>()
        .init_resource::<Running>()
        .add_systems(RenderStartup, forget_device)
        .add_systems(ExtractSchedule, extract)
        .add_systems(
            Render,
            run.in_set(RenderSystems::Render).before(render_system),
        );
}

/// A plugin's compute, as its scene keeps it.
#[derive(Component, Clone, Debug)]
#[component(on_remove = give_slot)]
pub(crate) struct ComputeJob {
    pub(crate) owner: Entity,
    pub(crate) shader: Handle<Shader>,
    pub(crate) data: Handle<ShaderBuffer>,
    pub(crate) extra: Option<Handle<ShaderBuffer>>,
    pub(crate) params: [Vec4; PARAMS],
    pub(crate) groups: [u32; 3],
    pub(crate) every_frame: bool,
    /// Counts the dispatches asked for; a run is owed while it is
    /// ahead of the runs made.
    pub(crate) asked: u64,
    /// Its slot of the clock, if it is timed.
    pub(crate) slot: Option<u32>,
}

fn give_slot(mut world: DeferredWorld, context: HookContext) {
    let slot = world
        .get::<ComputeJob>(context.entity)
        .and_then(|job| job.slot);
    if let Some(slot) = slot {
        world.resource_mut::<TimedSlots>().give(slot);
    }
}

/// The jobs of the frame, as extracted.
#[derive(Resource, Default)]
struct Jobs {
    jobs: Vec<(Entity, ComputeJob)>,
    throttled: HashSet<Entity>,
    /// The workgroups each owner may run in the frame.
    workgroups: HashMap<Entity, u32>,
    time: f32,
    delta: f32,
    slots: u32,
    clock: Option<Handle<ShaderBuffer>>,
}

/// The query set the clock writes, made once the device is known, and
/// the buffer its timestamps are resolved into this frame.
#[derive(Resource, Default)]
pub(crate) struct RenderClock {
    pub(crate) set: Option<QuerySet>,
    pub(crate) buffer: Option<Buffer>,
}

impl RenderClock {
    /// The range of queries of `slot`, and where they are resolved.
    pub(crate) fn place(slot: u32) -> (std::ops::Range<u32>, u64) {
        (slot * 2..slot * 2 + 2, u64::from(slot) * SLOT_BYTES)
    }
}

fn extract(
    jobs: Extract<Query<(Entity, &ComputeJob)>>,
    throttled: Extract<Res<Throttled>>,
    owners: Extract<Query<&PluginBudgets>>,
    tuning: Extract<Res<RenderTuning>>,
    time: Extract<Res<Time>>,
    clock: Extract<Option<Res<GpuClock>>>,
    shaders: Extract<Res<Assets<Shader>>>,
    mut running: ResMut<Running>,
    mut commands: Commands,
) {
    running
        .pipelines
        .retain(|&shader, _| shaders.contains(shader));
    commands.insert_resource(Jobs {
        jobs: jobs
            .iter()
            .map(|(entity, job)| (entity, job.clone()))
            .collect(),
        throttled: throttled.0.clone(),
        workgroups: jobs
            .iter()
            .filter_map(|(_, job)| {
                let budgets = owners.get(job.owner).ok()?;
                Some((job.owner, budgets.workgroups_per_frame))
            })
            .collect(),
        time: time.elapsed_secs_wrapped(),
        delta: time.delta_secs(),
        slots: tuning.timed_slots,
        clock: clock.as_ref().map(|clock| clock.0.clone()),
    });
}

/// Forgets what was made on the device before, as the renderer starts
/// again on a new one after a failed device.
fn forget_device(mut commands: Commands) {
    commands.insert_resource(Running::default());
    commands.insert_resource(RenderClock::default());
}

/// What [`run`] keeps from frame to frame, on the device it was made
/// on.
#[derive(Resource, Default)]
struct Running {
    layout: Option<BindGroupLayoutDescriptor>,
    /// The pipeline of each shader, while the shader is kept.
    pipelines: HashMap<AssetId<Shader>, CachedComputePipelineId>,
    /// The dispatches each job ran.
    ran: HashMap<Entity, u64>,
    /// Bound for a job with no second buffer.
    spare: Option<Buffer>,
    frames: u32,
}

fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "hamlet_compute",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                uniform_buffer_sized(false, None),
            ),
        ),
    )
}

/// Runs each job owed a run, within its owner's workgroups of the
/// frame, timing it where the GPU can, and submits them before the
/// cameras draw.
fn run(
    jobs: Res<Jobs>,
    pipeline_cache: Res<PipelineCache>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut clock: ResMut<RenderClock>,
    mut running: ResMut<Running>,
) {
    running.frames = running.frames.wrapping_add(1);
    let timed =
        device.features().contains(WgpuFeatures::TIMESTAMP_QUERY);
    if timed && clock.set.is_none() && jobs.slots > 0 {
        clock.set = Some(device.wgpu_device().create_query_set(
            &QuerySetDescriptor {
                label: Some("hamlet_clock"),
                ty: QueryType::Timestamp,
                count: jobs.slots * 2,
            },
        ));
    }
    clock.buffer = jobs
        .clock
        .as_ref()
        .and_then(|buffer| buffers.get(buffer))
        .map(|buffer| buffer.buffer.clone());
    let mut encoder =
        device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("hamlet_compute"),
        });
    if let Some(buffer) = &clock.buffer {
        encoder.clear_buffer(buffer, 0, None);
    }
    let layout = running.layout.get_or_insert_with(layout).clone();
    let spare = running
        .spare
        .get_or_insert_with(|| {
            device.create_buffer(&BufferDescriptor {
                label: Some("hamlet_spare"),
                size: 16,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
        .clone();
    // A dispatch past it would fail the device.
    let widest = device.limits().max_compute_workgroups_per_dimension;
    let mut spent = HashMap::<Entity, u32>::new();
    let frame = |count: u32| {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&jobs.time.to_le_bytes());
        bytes.extend_from_slice(&jobs.delta.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    };
    for (entity, job) in &jobs.jobs {
        let owed = job.every_frame
            || running.ran.get(entity).copied().unwrap_or(0)
                < job.asked;
        let timing = job
            .slot
            .filter(|_| clock.buffer.is_some())
            .zip(clock.set.as_ref());
        // Untimed, it would escape its owner's GPU time; it waits for
        // a slot.
        let waits = clock.set.is_some() && timing.is_none();
        if job.asked == 0
            || !owed
            || waits
            || jobs.throttled.contains(&job.owner)
        {
            continue;
        }
        let [x, y, z] = job.groups;
        let groups = x.saturating_mul(y).saturating_mul(z);
        let most =
            jobs.workgroups.get(&job.owner).copied().unwrap_or(0);
        let used = spent.entry(job.owner).or_default();
        if used.saturating_add(groups) > most
            || job.groups.iter().any(|&along| along > widest)
        {
            continue;
        }
        let id = *running
            .pipelines
            .entry(job.shader.id())
            .or_insert_with(|| {
                pipeline_cache.queue_compute_pipeline(
                    ComputePipelineDescriptor {
                        label: Some("hamlet_compute".into()),
                        layout: vec![layout.clone()],
                        shader: job.shader.clone(),
                        entry_point: Some("main".into()),
                        zero_initialize_workgroup_memory: true,
                        ..ComputePipelineDescriptor::default()
                    },
                )
            });
        let (Some(pipeline), Some(data)) = (
            pipeline_cache.get_compute_pipeline(id),
            buffers.get(&job.data),
        ) else {
            continue;
        };
        let extra = match job
            .extra
            .as_ref()
            .and_then(|extra| buffers.get(extra))
        {
            Some(extra) => &extra.buffer,
            None => {
                // Every owner's compute may write it, so none reads
                // what another wrote.
                encoder.clear_buffer(&spare, 0, None);
                &spare
            }
        };
        *used += groups;
        let params =
            device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("hamlet_params"),
                contents: param_bytes(&job.params).as_slice(),
                usage: BufferUsages::UNIFORM,
            });
        let times =
            device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("hamlet_frame"),
                contents: &frame(running.frames),
                usage: BufferUsages::UNIFORM,
            });
        let bind_group = device.create_bind_group(
            "hamlet_compute",
            &pipeline_cache.get_bind_group_layout(&layout),
            &BindGroupEntries::sequential((
                params.as_entire_binding(),
                data.buffer.as_entire_binding(),
                extra.as_entire_binding(),
                times.as_entire_binding(),
            )),
        );
        {
            let mut pass =
                encoder.begin_compute_pass(&ComputePassDescriptor {
                    label: Some("hamlet_compute"),
                    timestamp_writes: timing.map(|(slot, set)| {
                        ComputePassTimestampWrites {
                            query_set: set,
                            beginning_of_pass_write_index: Some(
                                slot * 2,
                            ),
                            end_of_pass_write_index: Some(
                                slot * 2 + 1,
                            ),
                        }
                    }),
                });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(x, y, z);
        }
        if let (Some((slot, set)), Some(buffer)) =
            (timing, &clock.buffer)
        {
            let (queries, offset) = RenderClock::place(slot);
            encoder.resolve_query_set(set, queries, buffer, offset);
        }
        running.ran.insert(*entity, job.asked);
    }
    let alive = jobs
        .jobs
        .iter()
        .map(|(entity, _)| *entity)
        .collect::<HashSet<_>>();
    running.ran.retain(|entity, _| alive.contains(entity));
    queue.submit([encoder.finish()]);
}

/// The bytes of `params`, as a uniform holds them.
pub(crate) fn param_bytes(params: &[Vec4; PARAMS]) -> Vec<u8> {
    params
        .iter()
        .flat_map(|param| param.to_array())
        .flat_map(f32::to_le_bytes)
        .collect()
}
