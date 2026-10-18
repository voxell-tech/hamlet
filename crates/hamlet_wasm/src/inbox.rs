//! What waits for a plugin to be stepped.

use bevy::prelude::*;
use hamlet_sdk::plugin::Status;

use crate::interface::exports::hamlet::plugin::plugin::Incoming;

/// On the entity of an installed plugin: what happened since its last
/// `step`.
#[derive(Component, Default)]
pub struct Inbox(Vec<Incoming>);

impl Inbox {
    pub fn push(&mut self, incoming: Incoming) {
        self.0.push(incoming);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn take(&mut self) -> Vec<Incoming> {
        std::mem::take(&mut self.0)
    }
}

/// Whether any plugin has something waiting.
pub fn has_mail(inboxes: Query<&Inbox>) -> bool {
    inboxes.iter().any(|inbox| !inbox.is_empty())
}

/// Puts a tick, numbered from the first, in the inbox of every
/// running plugin.
pub fn tick(
    mut plugins: Query<(&Status, &mut Inbox)>,
    mut frame: Local<u64>,
) {
    *frame += 1;
    for (status, mut inbox) in &mut plugins {
        if *status == Status::Running {
            inbox.push(Incoming::Tick(*frame));
        }
    }
}
