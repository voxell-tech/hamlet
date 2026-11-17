//! Tool runs in progress: each an entity of its own whose task starts
//! once its input is kept and stops with it, and whose outcome comes
//! back as an input.

use std::future::Future;
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::message::{Message, Messages};
use bevy_ecs::system::{Commands, Query, ResMut};
use bevy_ecs::world::DeferredWorld;
use futures::future::BoxFuture;
use hamlet_store::ids::ConversationId;
use hamlet_store::marker::{self, Request};
use tokio::task::AbortHandle;

use super::{Answer, CallId, Reply};
use crate::app::input::{AddHandler, InputSender};
use crate::core::Core;
use crate::tools::builtin::Outcome;

/// Where ended tool runs go, and the system that takes them in.
pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_handler::<ToolRunEnded, _>(take_in_ran);
}

/// A tool run ended.
#[derive(Message)]
pub(crate) struct ToolRunEnded(pub Ran);

/// Takes in the tool runs that ended.
fn take_in_ran(
    mut ended: ResMut<Messages<ToolRunEnded>>,
    mut core: Core,
) -> bevy_ecs::error::Result {
    ended.drain().try_for_each(|ToolRunEnded(ran)| {
        core.ran(ran)?;
        Ok(())
    })
}

/// A tool run that ended.
pub(crate) struct Ran {
    call: CallId,
    outcome: Outcome,
}

/// A tool run in progress: an entity of its own, despawned when it
/// ends or its turn is stopped. Its task stops with it.
#[derive(Component)]
#[component(on_remove = stopped)]
pub(crate) struct ToolRun {
    pub(super) call: CallId,
    pub(super) replies: Vec<Reply>,
    pub conversation: ConversationId,
    request: Request,
    /// The run's task, once started.
    abort: Option<AbortHandle>,
}

/// The work of a run not started yet: it starts once the input that
/// requested it is kept, so a failed input runs nothing.
#[derive(Component)]
pub(crate) struct UnstartedToolRun(
    Mutex<Option<BoxFuture<'static, ()>>>,
);

/// Starts the work of every run its input kept.
pub(crate) fn start(
    mut runs: Query<(Entity, &mut ToolRun, &mut UnstartedToolRun)>,
    mut commands: Commands,
) {
    for (entity, mut running, mut starting) in &mut runs {
        let work = starting
            .0
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(work) = work {
            running.abort = Some(tokio::spawn(work).abort_handle());
        }
        commands.entity(entity).remove::<UnstartedToolRun>();
    }
}

/// The replies and request an ended run still owes.
struct EndedToolRun {
    replies: Vec<Reply>,
    request: Request,
}

/// A run's entity went: its task, if still running, stops.
fn stopped(world: DeferredWorld, context: HookContext) {
    if let Some(abort) = world
        .get::<ToolRun>(context.entity)
        .and_then(|running| running.abort.as_ref())
    {
        abort.abort();
    }
}

/// Every tool run in progress.
pub(crate) type ToolRuns<'w, 's> =
    Query<'w, 's, (Entity, &'static mut ToolRun)>;

/// The run of `work` for `call`, to spawn; it starts once its input
/// is kept, and its end goes to `inputs`.
pub(super) fn tool_run(
    inputs: &InputSender,
    call: CallId,
    conversation: ConversationId,
    reply: Reply,
    request: Request,
    work: impl Future<Output = Outcome> + Send + 'static,
) -> (ToolRun, UnstartedToolRun) {
    let inputs = inputs.clone();
    let id = call.clone();
    let work = async move {
        let outcome = work.await;
        let _ = inputs.send(ToolRunEnded(Ran { call: id, outcome }));
    };
    let running = ToolRun {
        call,
        replies: vec![reply],
        conversation,
        request,
        abort: None,
    };
    (running, UnstartedToolRun(Mutex::new(Some(Box::pin(work)))))
}

impl Core<'_, '_> {
    /// Keeps the outcome of a tool run that ended and answers each
    /// runtime that made it.
    pub(crate) fn ran(&mut self, ran: Ran) -> Result<()> {
        let Some((entity, mut found)) = self
            .runs
            .iter_mut()
            .find(|(_, running)| running.call == ran.call)
        else {
            return Ok(());
        };
        let running = EndedToolRun {
            replies: std::mem::take(&mut found.replies),
            request: found.request.clone(),
        };
        self.commands.entity(entity).despawn();
        crate::app::fault::point("ran");
        let db = self.realm.store().connection();
        marker::settle(
            db,
            ran.call.identity(),
            &running.request,
            &ran.outcome,
            crate::app::clock::now(),
        )?;
        self.looks.forget(&ran.call);
        for reply in running.replies {
            self.tell_call(
                &reply,
                &ran.call.key,
                Answer::Result(ran.outcome.clone()),
            );
        }
        Ok(())
    }
}
