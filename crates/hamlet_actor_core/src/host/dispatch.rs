// This file was changed for Hamlet. See NOTICE in this crate.
//! Sends actions and other requests to an actor's task, with the
//! action timeouts.

use std::future::Future;
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_actor_error::ActorError;
use tokio::sync::{mpsc, oneshot};

use crate::actor::action::ActionDispatchError;
use crate::actor::connection::ConnHandle;
use crate::actor::lifecycle_hooks::Reply;
use crate::actor::messages::{ActorEvent, Request};
use crate::actor::task::{
    DispatchCommand, try_send_dispatch_command,
};
use crate::error::ActorLifecycle as ActorLifecycleError;
use crate::telemetry::IncomingInvocationContext;
use crate::websocket::WebSocket;
use crate::{ActorContext, time};

/// An action, or a call, did not end within its time.
#[derive(ActorError)]
#[error("actor", "action_timed_out", "Action timed out")]
pub struct ActionTimedOut;

#[expect(
    clippy::result_large_err,
    reason = "the action route's error, as ported"
)]
pub(crate) async fn dispatch_action_through_task(
    dispatch: &mpsc::UnboundedSender<DispatchCommand>,
    conn: ConnHandle,
    name: String,
    args: Vec<u8>,
    incoming: IncomingInvocationContext,
) -> std::result::Result<Vec<u8>, ActionDispatchError> {
    let (reply_tx, reply_rx) = oneshot::channel();
    try_send_dispatch_command(
        dispatch,
        DispatchCommand::Action {
            name,
            args,
            incoming,
            conn,
            reply: reply_tx,
        },
    )
    .map_err(ActionDispatchError::from_anyhow)?;
    reply_rx
        .await
        .map_err(|_| {
            ActionDispatchError::from_anyhow(
                ActorLifecycleError::DroppedReply.build(),
            )
        })?
        .map_err(ActionDispatchError::from_anyhow)
}

#[expect(
    clippy::result_large_err,
    reason = "the action route's error, as ported"
)]
pub(crate) async fn with_action_dispatch_timeout<T, F>(
    duration: Duration,
    future: F,
) -> std::result::Result<T, ActionDispatchError>
where
    F: Future<Output = std::result::Result<T, ActionDispatchError>>,
{
    time::timeout(duration, future).await.map_err(|_| {
        ActionDispatchError::from_anyhow(ActionTimedOut.build())
    })?
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
pub(crate) async fn with_framework_action_timeout<T, F>(
    duration: Duration,
    future: F,
) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    time::timeout(duration, future)
        .await
        .map_err(|_| ActionTimedOut.build())?
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) async fn dispatch_websocket_open_through_task(
    dispatch: &mpsc::UnboundedSender<DispatchCommand>,
    conn: ConnHandle,
    ws: WebSocket,
    request: Option<Request>,
) -> Result<()> {
    let (reply_tx, reply_rx) = oneshot::channel();
    try_send_dispatch_command(
        dispatch,
        DispatchCommand::OpenWebSocket {
            conn,
            ws,
            request,
            reply: reply_tx,
        },
    )
    .context(
        "actor task stopped before websocket dispatch command could \
         be sent",
    )?;

    reply_rx.await.context(
        "actor task stopped before websocket dispatch reply was sent",
    )?
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) async fn dispatch_workflow_history_through_task(
    dispatch: &mpsc::UnboundedSender<DispatchCommand>,
) -> Result<Option<Vec<u8>>> {
    let (reply_tx, reply_rx) = oneshot::channel();
    try_send_dispatch_command(
        dispatch,
        DispatchCommand::WorkflowHistory { reply: reply_tx },
    )
    .context(
        "actor task stopped before workflow history dispatch \
         command could be sent",
    )?;

    reply_rx.await.context(
        "actor task stopped before workflow history dispatch reply \
         was sent",
    )?
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) async fn dispatch_workflow_replay_request_through_task(
    dispatch: &mpsc::UnboundedSender<DispatchCommand>,
    entry_id: Option<String>,
) -> Result<Option<Vec<u8>>> {
    let (reply_tx, reply_rx) = oneshot::channel();
    try_send_dispatch_command(
        dispatch,
        DispatchCommand::WorkflowReplay {
            entry_id,
            reply: reply_tx,
        },
    )
    .context(
        "actor task stopped before workflow replay dispatch command \
         could be sent",
    )?;

    reply_rx.await.context(
        "actor task stopped before workflow replay dispatch reply \
         was sent",
    )?
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
pub(crate) fn workflow_dispatch_result(
    result: Result<Option<Vec<u8>>>,
) -> Result<(bool, Option<Vec<u8>>)> {
    match result {
        Ok(history) => Ok((true, history)),
        // A dropped reply or an actor that never configured workflows
        // both mean "workflow unsupported". Report that as disabled
        // instead of aborting the whole inspector init, which also
        // carries tab config.
        Err(error)
            if is_dropped_reply_error(&error)
                || is_workflow_not_configured_error(&error) =>
        {
            Ok((false, None))
        }
        Err(error) => Err(error),
    }
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
pub(crate) fn is_dropped_reply_error(error: &anyhow::Error) -> bool {
    let error = ActorError::extract(error);
    error.group() == "actor" && error.code() == "dropped_reply"
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
fn is_workflow_not_configured_error(error: &anyhow::Error) -> bool {
    let error = ActorError::extract(error);
    error.group() == "actor" && error.code() == "not_configured"
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) async fn dispatch_subscribe_request(
    ctx: &ActorContext,
    conn: ConnHandle,
    event_name: String,
) -> Result<()> {
    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.try_send_actor_event(
        ActorEvent::SubscribeRequest {
            conn,
            event_name,
            reply: Reply::from(reply_tx),
        },
        "subscribe_request",
    )?;
    reply_rx.await.context(
        "actor task stopped before subscribe dispatch reply was sent",
    )?
}
