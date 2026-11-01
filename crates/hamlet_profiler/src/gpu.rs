//! The GPU's time per frame: a timestamp written by an empty compute
//! pass before the render graph's passes and another by one after
//! them, read back a few frames later. Metal times passes only at
//! their edges, so this is the finest the frame can be split there.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::render::renderer::{RenderDevice, RenderQueue};

use crate::Profiler;

/// How many frames' timestamps may be on their way back at once.
const SLOTS: usize = 4;

/// A slot's readback, set from wgpu's callback.
const WAITING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// The timestamps of the frames in flight.
#[derive(Resource)]
pub(crate) struct GpuTimer {
    /// Nanoseconds per timestamp tick.
    period: f64,
    slots: Vec<Slot>,
    /// The slot the frame being drawn writes to.
    current: Option<usize>,
}

struct Slot {
    queries: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    read: wgpu::Buffer,
    state: SlotState,
}

enum SlotState {
    Free,
    Writing(u64),
    /// Shared with the readback's callback, which wgpu runs on
    /// whichever thread polls the device.
    Reading(u64, Arc<AtomicU8>),
}

impl GpuTimer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let buffer = |usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("profiler timestamps"),
                size: 2 * size_of::<u64>() as u64,
                usage,
                mapped_at_creation: false,
            })
        };
        let slots = (0..SLOTS)
            .map(|_| Slot {
                queries: device.create_query_set(
                    &wgpu::QuerySetDescriptor {
                        label: Some("profiler timestamps"),
                        ty: wgpu::QueryType::Timestamp,
                        count: 2,
                    },
                ),
                resolved: buffer(
                    wgpu::BufferUsages::QUERY_RESOLVE
                        | wgpu::BufferUsages::COPY_SRC,
                ),
                read: buffer(
                    wgpu::BufferUsages::MAP_READ
                        | wgpu::BufferUsages::COPY_DST,
                ),
                state: SlotState::Free,
            })
            .collect();
        Self {
            period: f64::from(queue.get_timestamp_period()),
            slots,
            current: None,
        }
    }
}

/// Takes in the timestamps that are back, then writes the first of
/// the frame being drawn.
pub(crate) fn begin(
    timer: Option<ResMut<GpuTimer>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    profiler: Res<Profiler>,
    mut commands: Commands,
) {
    let Some(mut timer) = timer else {
        if device.features().contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            commands.insert_resource(GpuTimer::new(
                device.wgpu_device(),
                &queue,
            ));
        }
        return;
    };
    let period = timer.period;
    for slot in &mut timer.slots {
        let SlotState::Reading(number, state) = &slot.state else {
            continue;
        };
        let number = *number;
        match state.load(Ordering::Acquire) {
            WAITING => continue,
            MAPPED => {
                let Ok(view) = slot.read.slice(..).get_mapped_range()
                else {
                    slot.read.unmap();
                    slot.state = SlotState::Free;
                    continue;
                };
                let stamp = |at: usize| {
                    view.get(at * 8..at * 8 + 8)
                        .and_then(|bytes| bytes.try_into().ok())
                        .map_or(0, u64::from_le_bytes)
                };
                let (start, end) = (stamp(0), stamp(1));
                drop(view);
                slot.read.unmap();
                if end > start {
                    let took = (end - start) as f64 * period;
                    profiler.set_gpu(
                        number,
                        Duration::from_nanos(took as u64),
                    );
                }
            }
            _ => {}
        }
        slot.state = SlotState::Free;
    }
    let Some(number) = profiler.lock().rendering() else {
        return;
    };
    let Some(at) = timer
        .slots
        .iter()
        .position(|slot| matches!(slot.state, SlotState::Free))
    else {
        return;
    };
    let slot = &mut timer.slots[at];
    stamp(&device, &queue, &slot.queries, Stamp::Start, None);
    slot.state = SlotState::Writing(number);
    timer.current = Some(at);
}

/// Writes the last timestamp of the frame being drawn, and asks for
/// both back.
pub(crate) fn end(
    timer: Option<ResMut<GpuTimer>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let Some(mut timer) = timer else {
        return;
    };
    let Some(at) = timer.current.take() else {
        return;
    };
    let slot = &mut timer.slots[at];
    let SlotState::Writing(number) = slot.state else {
        return;
    };
    stamp(
        &device,
        &queue,
        &slot.queries,
        Stamp::End,
        Some((&slot.resolved, &slot.read)),
    );
    let state = Arc::new(AtomicU8::new(WAITING));
    let told = state.clone();
    slot.read.map_async(wgpu::MapMode::Read, .., move |result| {
        let to = if result.is_ok() { MAPPED } else { FAILED };
        told.store(to, Ordering::Release);
    });
    slot.state = SlotState::Reading(number, state);
}

/// Which edge of the frame a timestamp marks.
#[derive(Clone, Copy)]
enum Stamp {
    Start,
    End,
}

/// Submits an empty compute pass that writes the timestamp at
/// `which`, then copies both from `queries` to `read` through
/// `resolved` when given them.
fn stamp(
    device: &RenderDevice,
    queue: &RenderQueue,
    queries: &wgpu::QuerySet,
    which: Stamp,
    readback: Option<(&wgpu::Buffer, &wgpu::Buffer)>,
) {
    let mut encoder = device.wgpu_device().create_command_encoder(
        &wgpu::CommandEncoderDescriptor {
            label: Some("profiler timestamp"),
        },
    );
    let (start, end) = match which {
        Stamp::Start => (Some(0), None),
        Stamp::End => (None, Some(1)),
    };
    drop(encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("profiler timestamp"),
        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
            query_set: queries,
            beginning_of_pass_write_index: start,
            end_of_pass_write_index: end,
        }),
    }));
    if let Some((resolved, read)) = readback {
        encoder.resolve_query_set(queries, 0..2, resolved, 0);
        encoder.copy_buffer_to_buffer(resolved, 0, read, 0, None);
    }
    queue.submit([encoder.finish()]);
}
