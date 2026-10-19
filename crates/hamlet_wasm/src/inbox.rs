//! What waits for a plugin to be stepped.

use bevy::prelude::*;
use hamlet_sdk::plugin::Status;

use crate::interface::exports::hamlet::plugin::plugin::Incoming;
use crate::limits::Limits;

/// On the entity of an installed plugin: what happened since its last
/// `step`.
#[derive(Component, Default)]
pub struct Inbox {
    mail: Vec<Incoming>,
    /// The second, by the app's clock, that the count of events
    /// began.
    since: f32,
    events: u32,
}

impl Inbox {
    pub fn push(&mut self, incoming: Incoming) {
        self.mail.push(incoming);
    }

    pub fn is_empty(&self) -> bool {
        self.mail.is_empty()
    }

    pub fn take(&mut self) -> Vec<Incoming> {
        std::mem::take(&mut self.mail)
    }

    /// Whether one more event may be given to the plugin at `now`,
    /// seconds by the app's clock, when it may have `per_second` in
    /// each second. An event it may not have is counted out.
    pub fn admit(&mut self, now: f32, per_second: u32) -> bool {
        if now - self.since >= 1.0 || now < self.since {
            self.since = now;
            self.events = 0;
        }
        if self.events >= per_second {
            return false;
        }
        self.events += 1;
        true
    }
}

/// Whether any plugin has something waiting.
pub fn has_mail(inboxes: Query<&Inbox>) -> bool {
    inboxes.iter().any(|inbox| !inbox.is_empty())
}

/// On the entity of an installed plugin: whether its manifest wants
/// ticks, and when it last had one.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct Ticks {
    wanted: bool,
    last: f32,
}

impl Ticks {
    pub fn of(manifest: &crate::manifest::Manifest) -> Self {
        Self {
            wanted: manifest.ticks,
            last: f32::MIN,
        }
    }

    /// Whether the plugin is given a tick at `now`, seconds by the
    /// app's clock, when it may have `per_second` in each second. No
    /// clock gives every frame's.
    pub fn due(&mut self, now: Option<f32>, per_second: u32) -> bool {
        if !self.wanted || per_second == 0 {
            return false;
        }
        let Some(now) = now else {
            return true;
        };
        if now - self.last < 1.0 / per_second as f32
            && now >= self.last
        {
            return false;
        }
        self.last = now;
        true
    }
}

/// Puts a tick, numbered from the first frame, in the inbox of every
/// running plugin that wants ticks and is due one.
pub fn tick(
    mut plugins: Query<(&Status, &mut Inbox, &mut Ticks)>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    mut frame: Local<u64>,
) {
    *frame += 1;
    let now = time.map(|time| time.elapsed_secs());
    for (status, mut inbox, mut ticks) in &mut plugins {
        if *status == Status::Running
            && ticks.due(now, limits.tick_hz)
        {
            inbox.push(Incoming::Tick(*frame));
        }
    }
}
