//! Work off the loop: what awaits the remote workspace or the disk
//! runs as a task, and its result comes back as an input of its own,
//! so no handler awaits.

use std::future::Future;

use bevy_ecs::resource::Resource;
use hamlet_protocol::client::{
    ServerMessage, SettingsViewWorkspaceMove,
};
use hamlet_protocol::runtime::RuntimeMessage;
use tokio::sync::mpsc::UnboundedSender;

use crate::calls::CallId;
use crate::calls::looks::Found;

/// What a task finished with.
pub(crate) enum Finished {
    /// A message for a client, such as an image it asked to read.
    Reply { client: u64, message: ServerMessage },
    /// How moving the lodges goes, and the answer once it is over.
    Moving {
        progress: SettingsViewWorkspaceMove,
        answer: Option<(u64, ServerMessage)>,
    },
    /// What a look for a call found. The runtime's request for the
    /// call runs again, `again`, and finds it.
    Looked {
        call: CallId,
        found: Found,
        again: Again,
    },
}

/// A runtime's request to run again: the runtime, the process that
/// asked, and the message.
pub(crate) struct Again {
    pub from: bevy_ecs::entity::Entity,
    pub generation: u64,
    pub message: RuntimeMessage,
}

/// Where tasks send what they finished with.
#[derive(Resource, Clone)]
pub(crate) struct Later(UnboundedSender<Finished>);

/// A task's way to send more than one result.
#[derive(Clone)]
pub(crate) struct Report(UnboundedSender<Finished>);

impl Report {
    pub fn send(&self, finished: Finished) {
        // The loop is gone only as the server stops.
        let _ = self.0.send(finished);
    }
}

impl Later {
    pub fn new(done: UnboundedSender<Finished>) -> Self {
        Self(done)
    }

    /// Runs `work` on a task; what it finishes with is an input.
    pub fn spawn(
        &self,
        work: impl Future<Output = Finished> + Send + 'static,
    ) {
        let done = self.0.clone();
        tokio::spawn(async move {
            let _ = done.send(work.await);
        });
    }

    /// Runs the work `make` gives on a task, which reports as it
    /// goes.
    pub fn spawn_many<Work>(&self, make: impl FnOnce(Report) -> Work)
    where
        Work: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(make(Report(self.0.clone())));
    }
}
