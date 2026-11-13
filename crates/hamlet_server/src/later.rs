//! Work off the loop: what awaits the remote workspace or the disk
//! runs as a task, started once the input that asked for it is kept,
//! and its result comes back as an input of its own, so no handler
//! awaits.

use std::future::Future;
use std::sync::{Mutex, PoisonError};

use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use futures::future::BoxFuture;
use hamlet_protocol::client::{
    ServerMessage, SettingsViewWorkspaceMove,
};
use hamlet_protocol::runtime::RuntimeMessage;
use tokio::sync::mpsc::UnboundedSender;

use crate::calls::CallId;
use crate::calls::looks::Found;

/// The channel tasks send their results through, as inputs.
pub(crate) fn plugin(app: &mut bevy_app::App) {
    let done = crate::server::arriving::<Finished>(app);
    app.insert_resource(Later::new(done));
}

/// A task's result.
pub(crate) enum Finished {
    /// A message for a client, such as an image it asked to read.
    Reply { client: u64, message: ServerMessage },
    /// The progress of moving the lodges, and the answer once it is
    /// over.
    Moving {
        progress: SettingsViewWorkspaceMove,
        answer: Option<(u64, ServerMessage)>,
    },
    /// A look's finding for a call. The runtime's request for the
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

/// The tasks an input asked for, and where they send what they
/// finished with. A task starts once its input is kept, so a failed
/// input sets none off.
#[derive(Resource)]
pub(crate) struct Later {
    done: UnboundedSender<Finished>,
    /// Reached only through `&mut`; the lock makes it `Sync`.
    queued: Mutex<Vec<BoxFuture<'static, ()>>>,
}

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
        Self {
            done,
            queued: Mutex::new(Vec::new()),
        }
    }

    fn queued(&mut self) -> &mut Vec<BoxFuture<'static, ()>> {
        self.queued
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `work` on a task once the input is kept; what it
    /// finishes with is an input.
    pub fn spawn(
        &mut self,
        work: impl Future<Output = Finished> + Send + 'static,
    ) {
        let done = self.done.clone();
        self.queued().push(Box::pin(async move {
            let _ = done.send(work.await);
        }));
    }

    /// Runs the work `make` gives on a task once the input is kept,
    /// which reports as it goes.
    pub fn spawn_many<Work>(
        &mut self,
        make: impl FnOnce(Report) -> Work,
    ) where
        Work: Future<Output = ()> + Send + 'static,
    {
        let report = Report(self.done.clone());
        self.queued().push(Box::pin(make(report)));
    }

    /// Starts the tasks the kept input asked for.
    pub fn start(&mut self) {
        for work in self.queued().drain(..) {
            tokio::spawn(work);
        }
    }

    /// Drops the tasks of an input that failed.
    pub fn discard(&mut self) {
        self.queued().clear();
    }
}

/// Starts the tasks the kept input asked for.
pub(crate) fn start(mut later: ResMut<Later>) {
    later.start();
}
