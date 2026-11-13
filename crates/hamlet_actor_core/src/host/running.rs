// This file was changed for Hamlet. See NOTICE in this crate.
//! One generation of an actor's task, as a host starts it, calls its
//! actions and stops it. The steps follow the engine registry's
//! start, action dispatch and stop paths.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::HostHandle;
use super::dispatch::{
    dispatch_action_through_task, with_action_dispatch_timeout,
};
use crate::actor::action::ActionDispatchError;
use crate::actor::kv::LegacyActorKv;
use crate::actor::task::{
    DispatchCommand, LifecycleCommand, try_send_lifecycle_command,
};
use crate::telemetry::IncomingInvocationContext;
use crate::types::{ActorKey, format_actor_key};
use crate::{
    ActorContext, ActorFactory, ActorTask, Inspector, RuntimeSpawner,
    ShutdownKind, SqliteDb,
};

/// What a host gives to start one generation of an actor.
pub struct StartRequest {
    pub actor_id: String,
    pub name: String,
    pub key: ActorKey,
    pub generation: u32,
    pub factory: Arc<ActorFactory>,
    pub handle: HostHandle,
    /// The actor's storage was never written, so the start skips
    /// probing it for the core's schema.
    pub new_storage: bool,
    /// The input the actor is created with. The core keeps it with
    /// the actor's first state, so a later start's input is unused,
    /// except as a fork's input.
    pub input: Option<Vec<u8>>,
}

/// A started actor's task.
pub struct RunningActor {
    actor_id: String,
    generation: u32,
    ctx: ActorContext,
    lifecycle: mpsc::UnboundedSender<LifecycleCommand>,
    dispatch: mpsc::UnboundedSender<DispatchCommand>,
    action_timeout: Duration,
    join: Mutex<Option<JoinHandle<Result<()>>>>,
}

impl RunningActor {
    /// Builds the actor's context, spawns its task and waits until
    /// the task has started.
    pub async fn start(request: StartRequest) -> Result<Self> {
        let StartRequest {
            actor_id,
            name,
            key,
            generation,
            factory,
            handle,
            new_storage,
            input,
        } = request;
        let action_timeout = factory.config().action_timeout;
        let sql = SqliteDb::new_with_remote_sqlite(
            handle.clone(),
            actor_id.clone(),
            Some(format_actor_key(&key)),
            Some(u64::from(generation)),
            factory.config().has_database,
            true,
        )?;
        let ctx = ActorContext::build(
            actor_id.clone(),
            name,
            key,
            "local".to_owned(),
            handle.namespace().to_owned(),
            Some(generation),
            handle.pool_name().to_owned(),
            factory.config().clone(),
            LegacyActorKv::new(handle.clone(), actor_id.clone()),
            sql,
        );
        ctx.configure_envoy(handle, Some(generation));
        ctx.set_new_actor_storage(new_storage);
        ctx.set_start_received_at(crate::time::Instant::now());

        let (lifecycle_tx, lifecycle_rx) = mpsc::unbounded_channel();
        let (dispatch_tx, dispatch_rx) = mpsc::unbounded_channel();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        ctx.configure_lifecycle_events(Some(events_tx));
        ctx.cancel_sleep_timer();
        ctx.set_local_alarm_callback(Some(Arc::new({
            let lifecycle_tx = lifecycle_tx.clone();
            move || {
                let lifecycle_tx = lifecycle_tx.clone();
                Box::pin(async move {
                    let (reply, rx) = oneshot::channel();
                    if let Err(error) = try_send_lifecycle_command(
                        &lifecycle_tx,
                        LifecycleCommand::FireAlarm { reply },
                    ) {
                        tracing::warn!(
                            ?error,
                            "failed to enqueue actor alarm"
                        );
                        return;
                    }
                    let _ = rx.await;
                })
            }
        })));
        let task = ActorTask::new(
            actor_id.clone(),
            generation,
            lifecycle_rx,
            dispatch_rx,
            events_rx,
            factory,
            ctx.clone(),
            input,
        );
        let join = RuntimeSpawner::spawn(task.run());
        let (reply, rx) = oneshot::channel();
        try_send_lifecycle_command(
            &lifecycle_tx,
            LifecycleCommand::Start { reply },
        )
        .context("send actor task start command")?;
        rx.await
            .context("receive actor task start reply")?
            .with_context(|| format!("start actor `{actor_id}`"))?;
        ctx.configure_inspector(Some(Inspector::new()));
        Ok(Self {
            actor_id,
            generation,
            ctx,
            lifecycle: lifecycle_tx,
            dispatch: dispatch_tx,
            action_timeout,
            join: Mutex::new(Some(join)),
        })
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Calls an action: a connection for the call, the action through
    /// the actor task within the kind's action timeout, then the
    /// connection closes.
    pub async fn action(
        &self,
        name: &str,
        args: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let conn = self
            .ctx
            .connect_conn_with_request(Vec::new(), None, async {
                Ok::<Vec<u8>, anyhow::Error>(Vec::new())
            })
            .await?;
        let result = with_action_dispatch_timeout(
            self.action_timeout,
            dispatch_action_through_task(
                &self.dispatch,
                conn.clone(),
                name.to_owned(),
                args,
                IncomingInvocationContext::default(),
            ),
        )
        .await;
        let _ = conn.disconnect(None).await;
        result.map_err(ActionDispatchError::into_anyhow)
    }

    /// Stops the task and waits until it has ended.
    pub async fn stop(&self, reason: ShutdownKind) {
        let (reply, rx) = oneshot::channel();
        if try_send_lifecycle_command(
            &self.lifecycle,
            LifecycleCommand::Stop { reason, reply },
        )
        .is_ok()
        {
            let _ = rx.await;
        }
        let join = self.join.lock().take();
        if let Some(join) = join {
            let _ = join.await;
        }
        tracing::debug!(
            actor_id = self.actor_id,
            generation = self.generation,
            "actor stopped"
        );
    }
}

#[cfg(test)]
#[path = "../../tests/running.rs"]
mod tests;
