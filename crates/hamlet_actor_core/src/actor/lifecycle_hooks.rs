// This file was changed for Hamlet. See NOTICE in this crate.
use std::cell::Cell;

use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

use crate::actor::connection::ConnHandle;
use crate::actor::context::ActorContext;
use crate::actor::fork::ActorForkStart;
use crate::actor::messages::ActorEvent;

pub struct Reply<T> {
    tx: Option<oneshot::Sender<Result<T>>>,
    on_reply: Option<Box<dyn FnOnce(&Result<T>) + Send>>,
    on_cut_off: Option<Box<dyn FnOnce() + Send>>,
}

impl<T> Reply<T> {
    /// Runs `on_reply` with the result just before it is sent. A
    /// reply dropped unsent runs it with the dropped-reply error, so
    /// every outcome is seen.
    pub(crate) fn on_reply(
        mut self,
        on_reply: impl FnOnce(&Result<T>) + Send + 'static,
    ) -> Self {
        self.on_reply = Some(Box::new(on_reply));
        self
    }

    /// Runs `on_cut_off` just before the reply is answered for a
    /// handler that did not run to its end: by [`Reply::cut_off`], or
    /// by a drop with no panic of the handler's to report.
    pub(crate) fn on_cut_off(
        mut self,
        on_cut_off: impl FnOnce() + Send + 'static,
    ) -> Self {
        self.on_cut_off = Some(Box::new(on_cut_off));
        self
    }

    pub fn send(mut self, result: Result<T>) {
        self.deliver(result);
    }

    /// Answers `error` for a handler that was cut off before its end,
    /// as by its actor's stop.
    pub fn cut_off(mut self, error: anyhow::Error) {
        self.cut();
        self.deliver(Err(error));
    }

    fn cut(&mut self) {
        if let Some(on_cut_off) = self.on_cut_off.take() {
            on_cut_off();
        }
    }

    /// Runs cancellation-safe work and replies with its result. If
    /// the caller abandons the reply, drops the work instead. This
    /// does not roll back I/O or stop a JavaScript promise that has
    /// already started executing.
    pub async fn send_or_cancel<F>(mut self, work: F)
    where
        F: Future<Output = Result<T>>,
    {
        let Some(tx) = self.tx.as_mut() else {
            return;
        };
        tokio::select! {
            biased;
            _ = tx.closed() => {}
            result = work => self.deliver(result),
        }
    }

    fn deliver(&mut self, result: Result<T>) {
        if let Some(on_reply) = self.on_reply.take() {
            on_reply(&result);
        }
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(result);
        }
    }
}

/// A reply dropped unsent answers `actor.dropped_reply`, as a cut-off
/// of its handler, or `actor.panicked` when the thread that drops it
/// is unwinding from a panic. A handler that panics in the task
/// holding its reply, as each action of the authoring layer does,
/// drops the reply that way, so the panic reads as the handler's
/// outcome, like an error it returned, and a one-shot it ran is not
/// kept to run again.
///
/// A panic in an actor's event loop answers each reply the loop
/// received as panicked. A reply still queued in the loop's channel
/// never reached the actor's code, so [`ActorEvents`]' drop answers
/// it `actor.dropped_reply`, and its one-shot is kept. A panic in
/// another task than the reply's leaves the reply's holder to answer
/// it.
impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        if self.tx.is_some() {
            let error =
                if std::thread::panicking() && !UNREACHED.get() {
                    crate::error::ActorRuntime::Panicked {
                        operation: "a handler".to_owned(),
                    }
                    .build()
                } else {
                    self.cut();
                    crate::error::ActorLifecycle::DroppedReply.build()
                };
            self.deliver(Err(error));
        }
    }
}

impl<T> std::fmt::Debug for Reply<T> {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("Reply")
            .field("pending", &self.tx.is_some())
            .finish()
    }
}

impl<T> From<oneshot::Sender<Result<T>>> for Reply<T> {
    fn from(tx: oneshot::Sender<Result<T>>) -> Self {
        Self {
            tx: Some(tx),
            on_reply: None,
            on_cut_off: None,
        }
    }
}

pub struct ActorEvents {
    actor_id: String,
    inner: mpsc::UnboundedReceiver<ActorEvent>,
}

impl ActorEvents {
    pub(crate) fn new(
        actor_id: String,
        inner: mpsc::UnboundedReceiver<ActorEvent>,
    ) -> Self {
        Self { actor_id, inner }
    }

    pub async fn recv(&mut self) -> Option<ActorEvent> {
        let event = self.inner.recv().await;
        if let Some(event) = &event {
            tracing::debug!(
                actor_id = %self.actor_id,
                event = event.kind(),
                "actor event drained"
            );
        }
        event
    }

    pub fn try_recv(&mut self) -> Option<ActorEvent> {
        let event = self.inner.try_recv().ok();
        if let Some(event) = &event {
            tracing::debug!(
                actor_id = %self.actor_id,
                event = event.kind(),
                "actor event drained"
            );
        }
        event
    }
}

thread_local! {
    /// Set while [`ActorEvents`]' drop drops the events still queued
    /// in its channel, which never reached the actor's code.
    static UNREACHED: Cell<bool> = const { Cell::new(false) };
}

/// Closes the channel and drops the events still queued in it. A
/// reply among them answers `actor.dropped_reply`, also while a panic
/// of the loop that held the channel unwinds: the panic was not
/// theirs.
///
/// The drain goes on until the channel reports it is disconnected, as
/// tokio's `close` asks. Until then a send that began before the
/// close has yet to land, and the channel reads empty, so the drain
/// yields to let it land. Stopping at empty would leave its event to
/// drop after the drain, as panicked, or to wait in the channel
/// unanswered.
impl Drop for ActorEvents {
    fn drop(&mut self) {
        /// Clears [`UNREACHED`] however the drain ends.
        struct Reached;
        impl Drop for Reached {
            fn drop(&mut self) {
                UNREACHED.set(false);
            }
        }
        self.inner.close();
        UNREACHED.set(true);
        let _reached = Reached;
        loop {
            match self.inner.try_recv() {
                Ok(event) => drop(event),
                Err(mpsc::error::TryRecvError::Empty) => {
                    std::thread::yield_now();
                }
                Err(mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
    }
}

impl std::fmt::Debug for ActorEvents {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str("ActorEvents(..)")
    }
}

impl From<mpsc::UnboundedReceiver<ActorEvent>> for ActorEvents {
    fn from(value: mpsc::UnboundedReceiver<ActorEvent>) -> Self {
        Self::new("unknown".to_owned(), value)
    }
}

#[derive(Debug)]
pub struct ActorStart {
    pub ctx: ActorContext,
    pub input: Option<Vec<u8>>,
    pub is_new: bool,
    pub snapshot: Option<Vec<u8>>,
    pub hibernated: Vec<(ConnHandle, Vec<u8>)>,
    /// The individual actor fork whose hook runs in this start, at
    /// the first start of a forked actor. The runtime runs the hook
    /// once its state is loaded and before `onWake`.
    pub fork: Option<ActorForkStart>,
    pub events: ActorEvents,
    pub startup_ready: Option<oneshot::Sender<Result<()>>>,
}
