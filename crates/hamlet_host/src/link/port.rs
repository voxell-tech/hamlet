//! The host's port to the host at the other end of the link: the
//! link's state as one value, and the session behind it, one child's
//! pipe with the requests in flight on it.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hamlet_actor_core::error::ActorRuntime;
use hamlet_actor_core::host::ActionTimedOut;
use hamlet_actor_error::ActorError;
use parking_lot::Mutex;
use serde_bytes::ByteBuf;
use tokio::sync::{oneshot, watch};

use super::frame::encode;
use super::{
    Ask, Broken, Frame, Role, Side, Vetted, WireError, Writer,
};
use crate::local::Inbound;
use crate::{Address, Caller, HostEvent};

/// The link's state, as the Rust host sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// No child has been welcomed, or the welcomed one is gone.
    Down,
    /// The Rust host has accepted the child's hello.
    Welcomed,
    /// The child has said it is ready.
    Ready,
}

/// A request of which nothing was written: the link was not up, or
/// went down before the request was queued. Asking again is safe.
#[derive(ActorError)]
#[error("link", "down", "The link to the other host is not up.")]
struct Down;

/// A request that was queued or written when the link went down.
/// Whether it ran is unknown.
#[derive(ActorError)]
#[error(
    "link",
    "lost",
    "The link to the other host went down before the answer came."
)]
struct Lost;

/// The host's way to the host at the other end of the link.
#[derive(Clone)]
pub(crate) struct Port(Arc<Shared>);

struct Shared {
    ours: Role,
    theirs: Side,
    link: Mutex<Link>,
    state: watch::Sender<LinkState>,
}

/// The link, as one value.
enum Link {
    Down,
    /// Requests come in on this session, and requests out fail with
    /// `link.down`.
    Welcomed(Arc<Session>),
    /// Requests go out on this session and come in on it.
    Ready(Arc<Session>),
}

pub(crate) enum Taken {
    Handled,
    Goodbye,
}

impl Port {
    /// A port that is down, for a host of role `ours` whose other
    /// host runs `kinds`.
    pub(crate) fn new(
        ours: Role,
        kinds: impl IntoIterator<Item = String>,
    ) -> Self {
        let theirs = match ours {
            Role::Rust => Role::Node,
            Role::Node => Role::Rust,
        };
        Self(Arc::new(Shared {
            ours,
            theirs: Side::new(theirs, kinds),
            link: Mutex::new(Link::Down),
            state: watch::Sender::new(LinkState::Down),
        }))
    }

    pub(crate) fn theirs(&self) -> &Side {
        &self.0.theirs
    }

    pub(crate) fn state(&self) -> watch::Receiver<LinkState> {
        self.0.state.subscribe()
    }

    /// Down to Welcomed: a new session on `out`, whose requests from
    /// the other host reach `host`.
    pub(crate) fn welcomed(&self, out: Writer, host: Inbound) {
        let mut link = self.0.link.lock();
        if let Link::Down = *link {
            *link = Link::Welcomed(Session::new(out, host));
            self.0.state.send_replace(LinkState::Welcomed);
        }
    }

    /// Welcomed to Ready: this host's requests leave on the session.
    pub(crate) fn ready(&self) {
        let mut link = self.0.link.lock();
        if let Link::Welcomed(session) = &*link {
            *link = Link::Ready(session.clone());
            self.0.state.send_replace(LinkState::Ready);
        }
    }

    /// Any state to Down. The session's requests out fail with
    /// `link.lost`, and its later ones with `link.down`.
    pub(crate) fn down(&self) {
        let mut link = self.0.link.lock();
        if let Link::Welcomed(session) | Link::Ready(session) =
            std::mem::replace(&mut *link, Link::Down)
        {
            session.lose();
            self.0.state.send_replace(LinkState::Down);
        }
    }

    /// The action `name` of the actor at `to`, for `caller`, within
    /// `deadline`. It fails:
    /// - at once with `link.down` unless the link is Ready;
    /// - with `link.lost` when the link goes down with the call out;
    /// - with `actor.action_timed_out` at the deadline;
    /// - otherwise with the other host's error, as it gave it.
    pub(crate) async fn call(
        &self,
        caller: Option<&Caller>,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        self.may_ask(caller)?;
        let deadline_ms =
            u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX);
        let ask = Ask::Call {
            name: name.to_owned(),
            args,
            deadline_ms,
        };
        self.session()?.ask(caller, to, ask, Some(deadline)).await
    }

    /// Makes the actor at `at` with `input`, for `caller`. Fails as
    /// [`Port::call`] does. It has no deadline of its own.
    pub(crate) async fn create(
        &self,
        caller: Option<&Caller>,
        at: &Address,
        input: Vec<u8>,
    ) -> Result<()> {
        self.may_ask(caller)?;
        let ask = Ask::Create { input };
        self.session()?.ask(caller, at, ask, None).await.map(drop)
    }

    fn session(&self) -> Result<Arc<Session>> {
        match &*self.0.link.lock() {
            Link::Ready(session) => Ok(session.clone()),
            Link::Down | Link::Welcomed(_) => Err(Down.build()),
        }
    }

    /// Fails with `actor.invalid_operation`, before anything is
    /// written, for a caller this host does not ask for.
    fn may_ask(&self, caller: Option<&Caller>) -> Result<()> {
        if matches!(caller, Some(Caller::Actor(_)))
            || self.0.ours.has_a_program()
        {
            return Ok(());
        }
        Err(ActorRuntime::InvalidOperation {
            operation: "ask across the link".to_owned(),
            reason: "the Node host asks only as one of its actors"
                .to_owned(),
        }
        .build())
    }

    /// Takes one frame the other host wrote after the handshake. An
    /// `Err` is the link's end.
    pub(crate) fn take(&self, frame: Frame) -> Result<Taken, Broken> {
        let session = match &*self.0.link.lock() {
            Link::Welcomed(session) | Link::Ready(session) => {
                session.clone()
            }
            Link::Down => {
                return Err(Broken::OutOfTurn(name(&frame)));
            }
        };
        match frame {
            Frame::Request { id, from, to, ask } => {
                session.run(id, self.0.theirs.vet(from)?, to, ask);
            }
            Frame::Reply { id, result } => session.settle(id, result),
            Frame::HostEvent(event)
                if self.0.theirs.role == Role::Node =>
            {
                session.report(event);
            }
            Frame::Goodbye if self.0.theirs.role == Role::Rust => {
                return Ok(Taken::Goodbye);
            }
            frame => return Err(Broken::OutOfTurn(name(&frame))),
        }
        Ok(Taken::Handled)
    }
}

/// One child's pipe, as one side sees it.
struct Session {
    out: Writer,
    host: Inbound,
    pending: Mutex<Pending>,
}

/// The requests this side has out, by id.
struct Pending {
    next: u64,
    waiting:
        HashMap<u64, oneshot::Sender<Result<Vec<u8>, WireError>>>,
    lost: bool,
}

impl Session {
    fn new(out: Writer, host: Inbound) -> Arc<Self> {
        Arc::new(Self {
            out,
            host,
            pending: Mutex::new(Pending {
                next: 0,
                waiting: HashMap::new(),
                lost: false,
            }),
        })
    }

    /// Asks the other host, and waits for its reply, within `within`
    /// when there is one.
    async fn ask(
        &self,
        caller: Option<&Caller>,
        to: &Address,
        ask: Ask,
        within: Option<Duration>,
    ) -> Result<Vec<u8>> {
        let id = {
            let mut pending = self.pending.lock();
            let id = pending.next;
            pending.next += 1;
            id
        };
        let bytes = encode(&Frame::Request {
            id,
            from: caller.cloned(),
            to: to.clone(),
            ask,
        })?;
        let (answer, answered) = oneshot::channel();
        {
            let mut pending = self.pending.lock();
            if pending.lost {
                return Err(Down.build());
            }
            pending.waiting.insert(id, answer);
        }
        let _forget = Forget {
            pending: &self.pending,
            id,
        };
        let asked = async {
            self.out.queue(bytes).await.map_err(|_| Down.build())?;
            match answered.await {
                Ok(Ok(output)) => Ok(output),
                Ok(Err(error)) => Err(error.into_error()),
                Err(_) => Err(Lost.build()),
            }
        };
        match within {
            Some(within) => tokio::time::timeout(within, asked)
                .await
                .map_err(|_| ActionTimedOut.build())?,
            None => asked.await,
        }
    }

    /// Runs the other host's request on this host's runtime, and
    /// writes its reply.
    fn run(
        self: &Arc<Self>,
        id: u64,
        from: Vetted,
        to: Address,
        ask: Ask,
    ) {
        let session = self.clone();
        self.host.runtime().spawn(async move {
            let result = match ask {
                Ask::Call {
                    name,
                    args,
                    deadline_ms,
                } => {
                    let deadline = Duration::from_millis(deadline_ms);
                    session
                        .host
                        .call(from, &to, &name, args, deadline)
                        .await
                }
                Ask::Create { input } => session
                    .host
                    .create(from, &to, input)
                    .await
                    .map(|()| Vec::new()),
            };
            session.reply(id, result).await;
        });
    }

    async fn reply(&self, id: u64, result: Result<Vec<u8>>) {
        let result = result
            .map(ByteBuf::from)
            .map_err(|error| WireError::of(&error));
        if let Err(error) =
            self.out.write(&Frame::Reply { id, result }).await
            && error.kind() == ErrorKind::InvalidInput
        {
            let error = WireError::of(&anyhow::Error::new(error));
            let reply = Frame::Reply {
                id,
                result: Err(error),
            };
            let _ = self.out.write(&reply).await;
        }
    }

    fn settle(&self, id: u64, result: Result<ByteBuf, WireError>) {
        let waiting = self.pending.lock().waiting.remove(&id);
        if let Some(waiting) = waiting {
            let _ = waiting.send(result.map(ByteBuf::into_vec));
        }
    }

    fn report(&self, event: HostEvent) {
        self.host.report(event);
    }

    /// Fails each request out with `link.lost`, and every later one
    /// with `link.down`.
    fn lose(&self) {
        let mut pending = self.pending.lock();
        pending.lost = true;
        pending.waiting.clear();
    }
}

struct Forget<'a> {
    pending: &'a Mutex<Pending>,
    id: u64,
}

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        self.pending.lock().waiting.remove(&self.id);
    }
}

fn name(frame: &Frame) -> String {
    match frame {
        Frame::Hello { .. } => "hello",
        Frame::Welcome { .. } => "welcome",
        Frame::Refused { .. } => "refused",
        Frame::Ready => "ready",
        Frame::Goodbye => "goodbye",
        Frame::Request { .. } => "request",
        Frame::Reply { .. } => "reply",
        Frame::HostEvent(_) => "host_event",
    }
    .to_owned()
}

#[cfg(test)]
mod tests;
