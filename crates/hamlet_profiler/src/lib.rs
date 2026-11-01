//! The frame profiler's records: the spans of each frame on the main
//! and render threads, as a tree with each span's total and self
//! time, and the GPU's time per frame. The last [`KEPT`] frames are
//! kept.

mod gpu;
mod layer;
mod record;
mod schedules;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::log::{BoxedLayer, warn};
use bevy::render::renderer::{RenderGraph, RenderGraphSystems};
use bevy::render::{ExtractSchedule, RenderApp};

pub use self::record::{
    FLOOR, Frame, KEPT, Lane, NameId, Recorder, Span, Timing,
};

/// Ties the records to the schedules and the GPU, once [`layer`] is
/// installed as Bevy's `LogPlugin`'s `custom_layer`.
pub fn plugin(app: &mut App) {
    let Some(profiler) =
        app.world().get_resource::<Profiler>().cloned()
    else {
        warn!(
            "the profiler's layer is not installed: nothing is \
             recorded"
        );
        return;
    };
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .insert_resource(profiler)
        .add_systems(ExtractSchedule, schedules::publish)
        .add_systems(
            RenderGraph,
            (
                gpu::begin.in_set(RenderGraphSystems::Begin),
                gpu::end.in_set(RenderGraphSystems::Finish),
            ),
        );
}

/// The layer that records, for `LogPlugin::custom_layer`. It puts
/// the [`Profiler`] it records into in `app`.
pub fn layer(app: &mut App) -> Option<BoxedLayer> {
    let profiler = Profiler(Arc::new(Shared {
        recording: AtomicBool::new(true),
        recorder: Mutex::default(),
    }));
    app.insert_resource(profiler.clone());
    Some(Box::new(layer::Recording(profiler)))
}

/// The records, shared with the layer on every thread.
#[derive(Resource, Clone)]
pub struct Profiler(Arc<Shared>);

struct Shared {
    /// Read before the lock, so a paused recorder costs a span next
    /// to nothing.
    recording: AtomicBool,
    recorder: Mutex<Recorder>,
}

impl Profiler {
    pub fn lock(&self) -> MutexGuard<'_, Recorder> {
        self.0
            .recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub fn is_recording(&self) -> bool {
        self.0.recording.load(Ordering::Relaxed)
    }

    /// Records from the next frame on, or stops and drops the frame
    /// under way.
    pub fn set_recording(&self, on: bool) {
        let mut recorder = self.lock();
        if on {
            recorder.resume();
        } else {
            recorder.close();
        }
        self.0.recording.store(on, Ordering::Relaxed);
    }

    /// Stops recording at the first frame longer than `spike`.
    pub fn set_spike(&self, spike: Option<Duration>) {
        self.lock().set_spike(spike);
    }

    /// Stops recording, the recorder already closed.
    fn stop(&self) {
        self.0.recording.store(false, Ordering::Relaxed);
    }

    fn set_gpu(&self, number: u64, took: Duration) {
        let spiked = self.lock().set_gpu(number, took);
        if spiked.is_some() {
            self.stop();
        }
    }
}
