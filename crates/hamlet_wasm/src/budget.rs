//! What the plugins may spend on the main thread: each plugin a time
//! and a fuel in a second, over every kind of call, and all of them a
//! time in a frame, where a plugin left out of one frame goes first
//! in the next.

use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::PluginSet;
use crate::limits::Limits;

/// Starts the budget of each frame.
pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Frame>()
        .add_systems(Update, begin.in_set(PluginSet::Collect));
}

/// The time the plugins' calls took in this frame, and its number.
#[derive(Resource, Default)]
pub struct Frame {
    number: u64,
    spent: Duration,
}

impl Frame {
    /// Whether the calls of this frame have time left.
    pub fn open(&self, limits: &Limits) -> bool {
        self.spent < Duration::from_millis(limits.frame_ms)
    }

    pub fn charge(&mut self, took: Duration) {
        self.spent += took;
    }

    pub fn number(&self) -> u64 {
        self.number
    }
}

fn begin(mut frame: ResMut<Frame>) {
    frame.number += 1;
    frame.spent = Duration::ZERO;
}

/// What one plugin spent in the second under way, and the frames it
/// was last called in by the frame's turn and last drawn in.
#[derive(Clone, Copy, Default, Debug)]
pub struct Spent {
    since: Option<Instant>,
    time: Duration,
    fuel: u64,
    /// The time the last call took.
    pub took: Duration,
    pub served: u64,
    pub drawn: u64,
}

impl Spent {
    /// Counts a call that took `took` and used `fuel`.
    pub fn charge(&mut self, took: Duration, fuel: u64) {
        self.roll();
        self.time += took;
        self.fuel += fuel;
        self.took = took;
    }

    /// Whether the plugin spent its time or its fuel of this second.
    pub fn over(&self, limits: &Limits) -> bool {
        !self.fresh(Instant::now())
            && (self.time
                >= Duration::from_millis(limits.plugin_ms_per_second)
                || self.fuel >= limits.plugin_fuel_per_second)
    }

    /// Whether a second has passed since the count began.
    fn fresh(&self, now: Instant) -> bool {
        self.since.is_none_or(|since| {
            now.duration_since(since) >= Duration::from_secs(1)
        })
    }

    fn roll(&mut self) {
        let now = Instant::now();
        if self.fresh(now) {
            (self.since, self.time, self.fuel) =
                (Some(now), Duration::ZERO, 0);
        }
    }
}

/// The plugins of `due`, each with the frame it was last served in,
/// in the order they are served: the one served longest ago first.
pub fn turns(mut due: Vec<(Entity, u64)>) -> Vec<Entity> {
    due.sort_by_key(|(_, served)| *served);
    due.into_iter().map(|(plugin, _)| plugin).collect()
}
