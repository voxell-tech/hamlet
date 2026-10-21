//! What plugins' own GPU work may spend: the GPU's time of each
//! plugin's compute and passes, timed with timestamp queries where
//! the device has them, its work held back for the rest of a second
//! once it is over; and a failing device rebuilt, with the plugins
//! whose work was on it stopped.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bevy::app::{App, AppExit, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{Message, MessageWriter};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists_and_changed;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::World;
use bevy::render::RenderApp;
use bevy::render::error_handler::{
    RenderError, RenderErrorHandler, RenderErrorPolicy,
};
use bevy::render::extract_resource::{
    ExtractResource, ExtractResourcePlugin,
};
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{BufferUsages, WgpuFeatures};
use bevy::render::renderer::{
    RenderAdapterInfo, RenderDevice, RenderQueue,
};
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::storage::ShaderBuffer;
use bevy::time::{Real, Time};
use hamlet_engine_core::{Budgets, LogLine};
use wgpu::Backend;

use crate::scene::Scene;
use crate::viewport::RenderTuning;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<GpuInfo>()
        .init_resource::<TimedSlots>()
        .init_resource::<Throttled>()
        .init_resource::<GpuSpend>()
        .add_message::<GpuLost>()
        .add_systems(
            Update,
            (
                (know, start_clock).chain().run_if(
                    resource_exists_and_changed::<RenderDevice>,
                ),
                tally,
            ),
        );
    if app.get_sub_app(RenderApp).is_some() {
        app.insert_resource(RenderErrorHandler(recover))
            .add_plugins(
                ExtractResourcePlugin::<Throttled>::default(),
            );
    }
}

/// Bytes between two timed slots of the clock's buffer, as wgpu
/// aligns the resolving of queries.
pub(crate) const SLOT_BYTES: u64 = 256;

/// What the GPU the engine draws with allows.
#[derive(Resource, Clone, Copy, Debug)]
pub struct GpuInfo {
    /// Whether wgpu bounds the loops of a checked shader on its
    /// backend.
    pub loops_bounded: bool,
    /// Invocations of one compute workgroup.
    pub invocations: u32,
    /// Whether it has timestamp queries.
    pub timed: bool,
    /// Nanoseconds of one tick of its timestamps.
    pub period: f32,
    /// The widest and tallest texture.
    pub texture_size: u32,
}

impl Default for GpuInfo {
    fn default() -> Self {
        Self {
            loops_bounded: true,
            invocations: 256,
            timed: false,
            period: 1.0,
            texture_size: 8_192,
        }
    }
}

/// The slots of the clock, each timing one compute or pass of an
/// owner.
#[derive(Resource, Default, Debug)]
pub(crate) struct TimedSlots {
    free: Vec<u32>,
    next: u32,
    pub(crate) owners: HashMap<u32, Entity>,
}

impl TimedSlots {
    /// A slot for work of `owner`, if one is left.
    pub(crate) fn take(
        &mut self,
        owner: Entity,
        most: u32,
    ) -> Option<u32> {
        let slot = self.free.pop().or_else(|| {
            (self.next < most).then(|| {
                self.next += 1;
                self.next - 1
            })
        })?;
        self.owners.insert(slot, owner);
        Some(slot)
    }

    pub(crate) fn give(&mut self, slot: u32) {
        if self.owners.remove(&slot).is_some() {
            self.free.push(slot);
        }
    }
}

/// The owners whose GPU work waits for the next second.
#[derive(Resource, ExtractResource, Clone, Default, Debug)]
pub struct Throttled(pub HashSet<Entity>);

/// Milliseconds of GPU time each owner's work took this second.
#[derive(Resource, Default, Debug)]
struct GpuSpend {
    since: Duration,
    spent: HashMap<Entity, f32>,
}

/// The buffer the clock's timestamps are resolved into and read back
/// from.
#[derive(Resource, Clone, Debug)]
pub(crate) struct GpuClock(pub(crate) Handle<ShaderBuffer>);

/// The device failed while these owners had GPU work of their own on
/// it: they are to be stopped. The device is built again.
#[derive(Message, Clone, Debug)]
pub struct GpuLost {
    pub owners: Vec<Entity>,
    pub why: String,
}

fn know(
    device: Res<RenderDevice>,
    adapter: Option<Res<RenderAdapterInfo>>,
    queue: Option<Res<RenderQueue>>,
    mut gpu: ResMut<GpuInfo>,
) {
    // wgpu bounds loops when it writes these backends' shaders.
    if let Some(adapter) = adapter {
        gpu.loops_bounded = matches!(
            adapter.backend,
            Backend::Vulkan | Backend::Metal | Backend::Dx12
        );
    }
    let limits = device.limits();
    gpu.invocations = limits.max_compute_invocations_per_workgroup;
    gpu.texture_size = limits.max_texture_dimension_2d;
    gpu.timed =
        device.features().contains(WgpuFeatures::TIMESTAMP_QUERY);
    if let Some(queue) = queue {
        gpu.period = queue.get_timestamp_period();
    }
}

/// Makes the clock's buffer, read back every frame, where the GPU has
/// timestamps.
fn start_clock(
    gpu: Res<GpuInfo>,
    tuning: Res<RenderTuning>,
    clock: Option<Res<GpuClock>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut commands: Commands,
) {
    if !gpu.timed || clock.is_some() {
        return;
    }
    let size = u64::from(tuning.timed_slots) * SLOT_BYTES;
    let mut buffer = ShaderBuffer::with_size(
        size as usize,
        RenderAssetUsages::default(),
    );
    buffer.buffer_description.usage |= BufferUsages::QUERY_RESOLVE;
    let buffer = buffers.add(buffer);
    commands.insert_resource(GpuClock(buffer.clone()));
    commands.spawn(Readback::buffer(buffer)).observe(read_clock);
}

/// Adds the time each timed slot took to its owner's second.
fn read_clock(
    read: On<ReadbackComplete>,
    slots: Res<TimedSlots>,
    gpu: Res<GpuInfo>,
    mut spend: ResMut<GpuSpend>,
) {
    let tick = |at: usize| {
        read.data
            .get(at..at + 8)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u64::from_le_bytes)
    };
    for (&slot, &owner) in &slots.owners {
        let at = (u64::from(slot) * SLOT_BYTES) as usize;
        if let (Some(begun), Some(ended)) = (tick(at), tick(at + 8))
            && ended > begun
        {
            let millis = (ended - begun) as f32 * gpu.period / 1.0e6;
            *spend.spent.entry(owner).or_default() += millis;
        }
    }
}

/// Holds back the work of each owner over its GPU time this second,
/// and begins the next second when this one is over.
fn tally(
    mut spend: ResMut<GpuSpend>,
    mut throttled: ResMut<Throttled>,
    budgets: Res<Budgets>,
    time: Res<Time<Real>>,
    mut log: MessageWriter<LogLine>,
) {
    let now = time.elapsed();
    if now.saturating_sub(spend.since) >= Duration::from_secs(1) {
        spend.since = now;
        spend.spent.clear();
        if !throttled.0.is_empty() {
            throttled.0.clear();
        }
        return;
    }
    for (&owner, &millis) in &spend.spent {
        if millis > budgets.gpu_time_per_second
            && throttled.0.insert(owner)
        {
            log.write(LogLine::warn(
                "render",
                format!(
                    "a plugin's GPU work took {millis:.1} ms this \
                     second, and waits for the next"
                ),
            ));
        }
    }
}

/// Rebuilds the device after it failed, and has the owners whose GPU
/// work was on it stopped. With no such work, the engine stops as
/// Bevy's own handler has it.
fn recover(
    error: &RenderError,
    main_world: &mut World,
    _: &mut World,
) -> RenderErrorPolicy {
    let owners = main_world
        .query::<&Scene>()
        .iter(main_world)
        .filter(|scene| scene.gpu_work())
        .map(|scene| scene.owner)
        .collect::<Vec<_>>();
    if owners.is_empty() {
        main_world.write_message(AppExit::error());
        return RenderErrorPolicy::StopRendering;
    }
    main_world.write_message(GpuLost {
        owners,
        why: format!("{:?}: {}", error.ty, error.description),
    });
    RenderErrorPolicy::Recover(RenderCreation::Automatic(Box::<
        WgpuSettings,
    >::default(
    )))
}
