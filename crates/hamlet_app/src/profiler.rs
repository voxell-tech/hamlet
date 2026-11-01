//! The Profiler: the recorded frames' times as a chart, and the spans
//! of the frame picked as a tree, from `hamlet_profiler`'s records.

mod chart;
mod hierarchy;

use std::collections::HashSet;
use std::time::Duration;

use bevy::app::{App, PreUpdate};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::{Local, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::{Real, Time};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::views::{FrameProps, column, row};
use bevy_fynix::{keyed, resource};
use hamlet_profiler::{NameId, Profiler};
use hamlet_ui::styles::{Fills, ICON, action, beside, group, note};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::switch::labelled_switch;
use hamlet_ui::{View, with_cx};

use crate::PanelSystems;
use crate::dock::{PanelAppExt, panel};

/// A frame longer than this stops the recording while Pause on Spike
/// is on.
const SPIKE: Duration = Duration::from_millis(17);

/// How often the tree moves on to the newest frame while recording.
const FOLLOW: Duration = Duration::from_millis(500);

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        hamlet_profiler::plugin,
        chart::plugin,
        hierarchy::plugin,
    ))
    // Closed until asked for from the View menu or a tab bar's plus.
    .add_panel(
        "profiler",
        DockWindowKind::new("PROFILER", || panel(screen()))
            .icon(Glyph::Meter.image(ICON)),
    )
    .init_resource::<Watch>()
    .add_systems(
        PreUpdate,
        follow
            .in_set(PanelSystems::Follow)
            .run_if(resource_exists::<Profiler>),
    );
}

/// What the Profiler shows: the frame picked, and how its tree is
/// sorted and opened.
#[derive(Resource)]
pub(crate) struct Watch {
    /// The frame picked in the chart; none follows the newest.
    picked: Option<u64>,
    /// The frame the tree shows.
    shown: Option<u64>,
    recording: bool,
    pause_on_spike: bool,
    sort: Sort,
    /// The rows opened or shut by hand, by the names from the root
    /// down. A root is open until shut.
    toggled: HashSet<Vec<NameId>>,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            picked: None,
            shown: None,
            recording: true,
            pause_on_spike: false,
            sort: Sort::Total,
            toggled: HashSet::new(),
        }
    }
}

/// The column the tree's rows are sorted by, longest first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sort {
    Total,
    SelfTime,
}

/// Keeps the frame shown in step with the recording: the one picked,
/// the one a spike stopped at, or the newest.
fn follow(
    profiler: Res<Profiler>,
    mut watch: ResMut<Watch>,
    time: Res<Time<Real>>,
    mut followed: Local<Duration>,
) {
    let recording = profiler.is_recording();
    let (latest, spiked) = {
        let recorder = profiler.lock();
        (
            recorder.latest().map(|frame| frame.number),
            recorder.spiked(),
        )
    };
    if watch.recording != recording {
        watch.recording = recording;
        if !recording && spiked.is_some() {
            watch.picked = spiked;
        }
    }
    let now = time.elapsed();
    let shown = match watch.picked {
        Some(picked) => Some(picked),
        None if recording && now < *followed + FOLLOW => watch.shown,
        None => latest,
    };
    if watch.shown != shown {
        *followed = now;
        watch.shown = shown;
    }
}

/// Starts or stops the recording. Starting it follows the newest
/// frame again.
fn toggle_recording(world: &mut World) {
    let on = !world.resource::<Profiler>().is_recording();
    world.resource::<Profiler>().set_recording(on);
    let mut watch = world.resource_mut::<Watch>();
    watch.recording = on;
    if on {
        watch.picked = None;
    }
}

fn set_pause_on_spike(world: &mut World, on: bool) {
    world.resource::<Profiler>().set_spike(on.then_some(SPIKE));
    world.resource_mut::<Watch>().pause_on_spike = on;
}

/// The Profiler's contents.
fn screen() -> impl View {
    let record =
        keyed(resource::<Watch, _>(|watch| watch.recording), |&on| {
            if on {
                action(Glyph::Stop, "Pause", toggle_recording)
            } else {
                action(Glyph::Play, "Record", toggle_recording)
            }
        })
        .within(row(()));
    let controls = beside((
        record,
        labelled_switch(
            "Pause on spike over 17 ms",
            resource::<Watch, _>(|watch| watch.pause_on_spike),
            set_pause_on_spike,
        ),
    ));
    let readings = group((
        chart::chart(),
        note(resource::<hierarchy::Tree, _>(|tree| {
            tree.summary.clone()
        })),
    ));
    with_cx(move |cx| {
        column((controls, readings, hierarchy::hierarchy()))
            .gap(cx.theme().space.panel)
            .fills_height()
    })
}
