//! What keeps plugins' own GPU work from failing the device: the
//! device's own sizes, and a failing device rebuilt, with the plugins
//! whose work was on it stopped. For a plugin the person gave a GPU
//! time a second, its compute and passes are timed with timestamp
//! queries where the device has them, and held back as a whole once
//! over, until its scene is made again. Work of any other plugin is
//! not timed, and nothing holds it back.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use bevy::app::{App, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{Message, MessageWriter};
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists_and_changed;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::{Mut, World};
use bevy::render::RenderApp;
use bevy::render::error_handler::{
    RenderError, RenderErrorHandler, RenderErrorPolicy,
};
use bevy::render::extract_resource::{
    ExtractResource, ExtractResourcePlugin,
};
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{
    BufferUsages, TextureFormat, WgpuFeatures,
};
use bevy::render::renderer::{
    RenderAdapter, RenderAdapterInfo, RenderDevice, RenderQueue,
};
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::storage::ShaderBuffer;
use bevy::render::view::Msaa;
use bevy::time::{Real, Time};
use hamlet_engine_core::{ActiveLimits, LogLine};
use wgpu::{Backend, Limits};

use crate::compute::ComputeJob;
use crate::pass::ScenePass;
use crate::scene::{Scene, Scenes};
use crate::viewport::RenderTuning;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<GpuInfo>()
        .init_resource::<TimedSlots>()
        .init_resource::<Throttled>()
        .init_resource::<Metered>()
        .init_resource::<GpuSpend>()
        .init_resource::<Rebuilds>()
        .add_message::<GpuLost>()
        .add_systems(
            Update,
            (
                know.run_if(
                    resource_exists_and_changed::<RenderDevice>,
                ),
                (meter, start_clock, tally, time_waiting).chain(),
            )
                .chain(),
        );
    if app.get_sub_app(RenderApp).is_some() {
        app.insert_resource(RenderErrorHandler(recover))
            .add_plugins((
                ExtractResourcePlugin::<Throttled>::default(),
                ExtractResourcePlugin::<Metered>::default(),
            ));
    }
}

/// Bytes between two timed slots of the clock's buffer, as wgpu
/// aligns the resolving of queries.
pub(crate) const SLOT_BYTES: u64 = 256;

/// A device failing this soon after it was rebuilt keeps failing.
const AGAIN: Duration = Duration::from_secs(10);

/// The first and the longest wait before a device that keeps failing
/// with no plugin to blame is rebuilt.
const FIRST_WAIT: Duration = Duration::from_secs(1);
const LONGEST_WAIT: Duration = Duration::from_secs(60);

/// What the GPU the engine draws with allows.
#[derive(Resource, Clone, Copy, Debug)]
pub struct GpuInfo {
    /// Whether wgpu bounds the loops of a checked shader on its
    /// backend.
    pub loops_bounded: bool,
    /// Invocations of one compute workgroup.
    pub invocations: u32,
    /// Invocations of one compute workgroup along each axis.
    pub workgroup_size: [u32; 3],
    /// Bytes of a compute workgroup's shared memory.
    pub workgroup_memory: u32,
    /// Workgroups of one dispatch along each axis.
    pub groups: u32,
    /// Bytes of the largest buffer a mesh or a storage binding
    /// holds.
    pub buffer_size: u64,
    /// The most samples a pixel of a camera's image.
    pub samples: u32,
    /// Whether it has timestamp queries.
    pub timed: bool,
    /// Nanoseconds of one tick of its timestamps.
    pub period: f32,
    /// The widest and tallest texture.
    pub texture_size: u32,
}

impl Default for GpuInfo {
    fn default() -> Self {
        let mut gpu = Self {
            loops_bounded: true,
            invocations: 0,
            workgroup_size: [0; 3],
            workgroup_memory: 0,
            groups: 0,
            buffer_size: 0,
            samples: 4,
            timed: false,
            period: 1.0,
            texture_size: 0,
        };
        gpu.limit(&Limits::default());
        gpu
    }
}

impl GpuInfo {
    /// Takes the sizes of `limits`.
    fn limit(&mut self, limits: &Limits) {
        self.invocations =
            limits.max_compute_invocations_per_workgroup;
        self.workgroup_size = [
            limits.max_compute_workgroup_size_x,
            limits.max_compute_workgroup_size_y,
            limits.max_compute_workgroup_size_z,
        ];
        self.workgroup_memory =
            limits.max_compute_workgroup_storage_size;
        self.groups = limits.max_compute_workgroups_per_dimension;
        self.buffer_size = limits
            .max_buffer_size
            .min(limits.max_storage_buffer_binding_size);
        self.texture_size = limits.max_texture_dimension_2d;
    }

    /// `msaa`, or the most samples the GPU has when it has fewer.
    pub fn msaa(&self, msaa: Msaa) -> Msaa {
        if msaa.samples() <= self.samples {
            msaa
        } else {
            Msaa::from_samples(self.samples)
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

/// The owners whose compute and passes are held back.
#[derive(Resource, ExtractResource, Clone, Default, Debug)]
pub struct Throttled(pub HashSet<Entity>);

/// The owners the person gave a GPU time a second, in milliseconds:
/// only their compute and passes are timed.
#[derive(
    Resource, ExtractResource, Clone, Default, PartialEq, Debug,
)]
pub struct Metered(pub HashMap<Entity, f32>);

/// Milliseconds of GPU time each owner's work took this second, and
/// the root of the scene of each owner held back as it went over.
#[derive(Resource, Default, Debug)]
struct GpuSpend {
    since: Duration,
    spent: HashMap<Entity, f32>,
    held: HashMap<Entity, Entity>,
}

/// When the device was last rebuilt, and the wait before rebuilding
/// one that keeps failing with no plugin to blame.
#[derive(Resource, Default, Debug)]
struct Rebuilds {
    last: Option<Instant>,
    wait: Duration,
    /// When the waiting device is rebuilt.
    until: Option<Instant>,
}

/// The buffer the clock's timestamps are resolved into and read back
/// from.
#[derive(Resource, Clone, Debug)]
pub(crate) struct GpuClock(pub(crate) Handle<ShaderBuffer>);

/// The device failed while these owners drew on it: they are to be
/// stopped. The device is built again.
#[derive(Message, Clone, Debug)]
pub struct GpuLost {
    pub owners: Vec<Entity>,
    pub why: String,
}

fn know(
    device: Res<RenderDevice>,
    info: Option<Res<RenderAdapterInfo>>,
    adapter: Option<Res<RenderAdapter>>,
    queue: Option<Res<RenderQueue>>,
    mut gpu: ResMut<GpuInfo>,
) {
    // wgpu bounds loops when it writes these backends' shaders.
    if let Some(info) = info {
        gpu.loops_bounded = matches!(
            info.backend,
            Backend::Vulkan | Backend::Metal | Backend::Dx12
        );
    }
    gpu.limit(&device.limits());
    // The textures a camera draws into, with or without high range.
    let formats = [
        TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Rgba16Float,
        TextureFormat::Depth32Float,
    ];
    let specific = device.features().contains(
        WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    );
    let supported = |format: TextureFormat, samples| {
        let features = match &adapter {
            Some(adapter) if specific => {
                adapter.get_texture_format_features(format)
            }
            _ => format.guaranteed_format_features(device.features()),
        };
        features.flags.sample_count_supported(samples)
    };
    gpu.samples = [8, 4, 2]
        .into_iter()
        .find(|&samples| {
            formats.iter().all(|&format| supported(format, samples))
        })
        .unwrap_or(1);
    gpu.timed =
        device.features().contains(WgpuFeatures::TIMESTAMP_QUERY);
    if let Some(queue) = queue {
        gpu.period = queue.get_timestamp_period();
    }
}

/// Keeps the owners with a GPU time a second the person switched on.
fn meter(
    owners: Query<(Entity, &ActiveLimits), With<Scenes>>,
    mut metered: ResMut<Metered>,
) {
    let now = owners
        .iter()
        .filter_map(|(owner, limits)| {
            Some((owner, limits.0.gpu_time_per_second? as f32))
        })
        .collect::<HashMap<_, _>>();
    if metered.0 != now {
        metered.0 = now;
    }
}

/// Makes the clock's buffer, read back every frame, where the GPU has
/// timestamps, once an owner's work is to be timed.
fn start_clock(
    gpu: Res<GpuInfo>,
    tuning: Res<RenderTuning>,
    metered: Res<Metered>,
    clock: Option<Res<GpuClock>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut commands: Commands,
) {
    if !gpu.timed || clock.is_some() || metered.0.is_empty() {
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

/// Holds back the work of each owner over the GPU time the person
/// gave it this second until its scene is made again, and begins the
/// next second when this one is over.
///
/// Work held back stays off as a whole: let run again each second, an
/// owner near its time would have its passes and compute go off and
/// on, and its view flicker.
fn tally(
    mut spend: ResMut<GpuSpend>,
    mut throttled: ResMut<Throttled>,
    scenes: Query<&Scenes>,
    metered: Res<Metered>,
    time: Res<Time<Real>>,
    mut log: MessageWriter<LogLine>,
) {
    let spend = &mut *spend;
    let root =
        |owner: Entity| scenes.get(owner).ok().and_then(Scenes::root);
    // Switched off and on, reloaded or stopped, the owner starts
    // afresh.
    let before = spend.held.len();
    spend.held.retain(|&owner, held| root(owner) == Some(*held));
    if spend.held.len() != before {
        throttled.0 = spend.held.keys().copied().collect();
    }
    let now = time.elapsed();
    if now.saturating_sub(spend.since) >= Duration::from_secs(1) {
        spend.since = now;
        spend.spent.clear();
        return;
    }
    for (&owner, &millis) in &spend.spent {
        let Some(&most) = metered.0.get(&owner) else {
            continue;
        };
        if millis <= most || spend.held.contains_key(&owner) {
            continue;
        }
        let Some(held) = root(owner) else {
            continue;
        };
        spend.held.insert(owner, held);
        throttled.0.insert(owner);
        log.write(LogLine::warn(
            "render",
            format!(
                "a plugin's GPU work took {millis:.1} ms this \
                 second, so its compute and passes are held back \
                 until it starts again"
            ),
        ));
    }
}

/// Gives metered work that waits for a slot of the clock one, as
/// other work lets its slot go.
fn time_waiting(
    mut jobs: Query<&mut ComputeJob>,
    mut passes: Query<&mut ScenePass>,
    gpu: Res<GpuInfo>,
    tuning: Res<RenderTuning>,
    metered: Res<Metered>,
    mut slots: ResMut<TimedSlots>,
) {
    if !gpu.timed {
        return;
    }
    let most = tuning.timed_slots;
    let waits = |owner: &Entity, slot: Option<u32>| {
        slot.is_none() && metered.0.contains_key(owner)
    };
    let waiting = jobs
        .iter_mut()
        .filter(|job| waits(&job.owner, job.slot))
        .map(|job| {
            (job.owner, job.map_unchanged(|job| &mut job.slot))
        })
        .chain(
            passes
                .iter_mut()
                .filter(|pass| waits(&pass.owner, pass.slot))
                .map(|pass| {
                    (
                        pass.owner,
                        pass.map_unchanged(|pass| &mut pass.slot),
                    )
                }),
        );
    for (owner, mut slot) in waiting {
        let Some(taken) = slots.take(owner, most) else {
            return;
        };
        *slot = Some(taken);
    }
}

/// Rebuilds the device after it failed, and has the owners whose GPU
/// work was on it stopped; when it fails again soon after, the owners
/// of any scene. With no owner to blame it is rebuilt all the same,
/// and one that keeps failing waits longer each time.
fn recover(
    error: &RenderError,
    main_world: &mut World,
    _: &mut World,
) -> RenderErrorPolicy {
    main_world.resource_scope(|world, mut rebuilds: Mut<Rebuilds>| {
        let now = Instant::now();
        // Bevy asks again each frame while rendering is stopped.
        if let Some(until) = rebuilds.until {
            if now < until {
                return RenderErrorPolicy::StopRendering;
            }
            rebuilds.until = None;
            rebuilds.last = Some(now);
            return rebuild();
        }
        let again = rebuilds
            .last
            .is_some_and(|at| now.duration_since(at) < AGAIN);
        let scenes = world
            .query::<&Scene>()
            .iter(world)
            .map(|scene| (scene.owner, scene.gpu_work()))
            .collect::<Vec<_>>();
        let mut owners = scenes
            .iter()
            .filter(|(_, work)| *work)
            .map(|(owner, _)| *owner)
            .collect::<Vec<_>>();
        if owners.is_empty() && again {
            owners = scenes.iter().map(|(owner, _)| *owner).collect();
        }
        if !owners.is_empty() {
            world.write_message(GpuLost {
                owners,
                why: format!("{:?}: {}", error.ty, error.description),
            });
        } else if again {
            // Nothing of a plugin's is on it, so the engine's own
            // drawing fails it: rebuilt at once, it would fail again
            // and flash each frame.
            rebuilds.wait =
                (rebuilds.wait * 2).clamp(FIRST_WAIT, LONGEST_WAIT);
            rebuilds.until = Some(now + rebuilds.wait);
            world.write_message(LogLine::warn(
                "render",
                format!(
                    "the GPU keeps failing, so it is rebuilt in {} s",
                    rebuilds.wait.as_secs()
                ),
            ));
            return RenderErrorPolicy::StopRendering;
        } else {
            rebuilds.wait = Duration::ZERO;
        }
        rebuilds.last = Some(now);
        rebuild()
    })
}

/// A new device, made as the first was.
fn rebuild() -> RenderErrorPolicy {
    // Bevy panics where no GPU is left to make it on, past where a
    // handler can answer.
    RenderErrorPolicy::Recover(RenderCreation::Automatic(Box::<
        WgpuSettings,
    >::default(
    )))
}
