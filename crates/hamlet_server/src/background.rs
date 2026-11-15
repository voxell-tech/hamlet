//! Work off the loop: what awaits the remote workspace or the disk
//! runs as a task, started once the input that asked for it is kept,
//! and its result comes back as an input of its own, so no handler
//! awaits.

use std::future::Future;
use std::sync::{Mutex, PoisonError};

use bevy_app::{App, PreUpdate};
use bevy_ecs::error::Result;
use bevy_ecs::message::Message;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::system::{Commands, ResMut};
use futures::future::BoxFuture;
use hamlet_protocol::client::{
    ServerMessage, SettingsViewWorkspaceMove,
};
use hamlet_protocol::runtime::RuntimeMessage;
use tokio::sync::mpsc::UnboundedSender;

use crate::calls::CallId;
use crate::calls::looks::Found;
use crate::core::{Core, UnpushedLodge};
use crate::server::InputMessage;
use crate::talk::heard;

/// The channel tasks send their results through, as inputs, and the
/// system that takes them in.
pub(crate) fn plugin(app: &mut App) {
    let finished =
        crate::server::input_channel::<BackgroundResult>(app);
    app.insert_resource(BackgroundWork::new(finished))
        .add_message::<BackgroundFinished>()
        .add_systems(
            PreUpdate,
            take_in_finished.run_if(on_message::<BackgroundFinished>),
        );
}

/// A task finished, with what it awaited off the loop.
#[derive(Message)]
pub(crate) struct BackgroundFinished(pub BackgroundResult);

/// Takes in what a task finished with. A look's finding is kept for
/// its call, and the runtime's request for the call is heard again,
/// so its system finds it.
fn take_in_finished(
    mut finished: InputMessage<BackgroundFinished>,
    mut core: Core,
    mut commands: Commands,
) -> Result {
    let Some(BackgroundFinished(finished)) = finished.take() else {
        return Ok(());
    };
    match finished {
        BackgroundResult::Reply { client, message } => {
            core.answer(client, message)?;
        }
        BackgroundResult::Moving { progress, answer } => {
            core.remote.moving.0 = Some(progress);
            core.changed.state_changed_outside_change = true;
            if let Some((client, message)) = answer {
                core.answer(client, message)?;
            }
        }
        BackgroundResult::Looked { call, found, again } => {
            if let Some(receiver) = core.looks.found(call, found) {
                core.remote.unpushed.write(UnpushedLodge(receiver));
            }
            // A runtime that started again since asks again.
            if core.runtimes.generation(again.from)
                == Some(again.generation)
            {
                heard::write(
                    &mut commands,
                    again.from,
                    again.message,
                );
            }
        }
    }
    Ok(())
}

/// A task's result.
pub(crate) enum BackgroundResult {
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
        again: RunAgainRequest,
    },
}

/// A runtime's request to run again: the runtime, the process that
/// asked, and the message.
pub(crate) struct RunAgainRequest {
    pub from: bevy_ecs::entity::Entity,
    pub generation: u64,
    pub message: RuntimeMessage,
}

/// The tasks an input asked for, and where they send what they
/// finished with. A task starts once its input is kept, so a failed
/// input sets none off.
#[derive(Resource)]
pub(crate) struct BackgroundWork {
    finished_sender: UnboundedSender<BackgroundResult>,
    /// Reached only through `&mut`; the lock makes it `Sync`.
    queued_futures: Mutex<Vec<BoxFuture<'static, ()>>>,
}

/// A task's way to send more than one result.
#[derive(Clone)]
pub(crate) struct BackgroundReporter(
    UnboundedSender<BackgroundResult>,
);

impl BackgroundReporter {
    pub fn send(&self, finished: BackgroundResult) {
        // The loop is gone only as the server stops.
        let _ = self.0.send(finished);
    }
}

impl BackgroundWork {
    pub fn new(
        finished_sender: UnboundedSender<BackgroundResult>,
    ) -> Self {
        Self {
            finished_sender,
            queued_futures: Mutex::new(Vec::new()),
        }
    }

    fn queued(&mut self) -> &mut Vec<BoxFuture<'static, ()>> {
        self.queued_futures
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `work` on a task once the input is kept; what it
    /// finishes with is an input.
    pub fn spawn(
        &mut self,
        work: impl Future<Output = BackgroundResult> + Send + 'static,
    ) {
        let done = self.finished_sender.clone();
        self.queued().push(Box::pin(async move {
            let _ = done.send(work.await);
        }));
    }

    /// Runs the work `make` gives on a task once the input is kept,
    /// which reports as it goes.
    pub fn spawn_many<Work>(
        &mut self,
        make: impl FnOnce(BackgroundReporter) -> Work,
    ) where
        Work: Future<Output = ()> + Send + 'static,
    {
        let report = BackgroundReporter(self.finished_sender.clone());
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
pub(crate) fn start(mut later: ResMut<BackgroundWork>) {
    later.start();
}
