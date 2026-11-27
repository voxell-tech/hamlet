//! The host's port to the host at the other end of the link: the
//! link's state as one value, and the session behind it, one child's
//! pipe with the requests in flight on it.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hamlet_actor_core::error::ActorRuntime;
use hamlet_actor_core::host::ActionTimedOut;
use hamlet_actor_error::ActorError;
use parking_lot::Mutex;
use serde_bytes::ByteBuf;
use tokio::sync::{oneshot, watch};

pub(crate) use self::follow::{Routed, Unanswered};
use super::frame::encode;
use super::{
    Ask, Broken, Frame, Role, Side, Vetted, WireError, Writer,
};
use crate::local::feed::Sink;
use crate::local::{Inbound, Origin, Stamp};
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
    /// Set from a child's start until its link goes down. While it
    /// is, a call or create out waits for the link to be Ready.
    starting: watch::Sender<bool>,
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

impl Link {
    fn state(&self) -> LinkState {
        match self {
            Self::Down => LinkState::Down,
            Self::Welcomed(_) => LinkState::Welcomed,
            Self::Ready(_) => LinkState::Ready,
        }
    }
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
            starting: watch::Sender::new(false),
        }))
    }

    pub(crate) fn theirs(&self) -> &Side {
        &self.0.theirs
    }

    pub(crate) fn state(&self) -> watch::Receiver<LinkState> {
        self.0.state.subscribe()
    }

    /// A child has started: calls and creates out wait for its link
    /// to be Ready, until it goes down.
    pub(crate) fn starting(&self) {
        self.0.starting.send_replace(true);
    }

    /// Down to Welcomed: a new session on `out`, whose requests from
    /// the other host reach `host`.
    pub(crate) fn welcomed(&self, out: Writer, host: Inbound) {
        self.shift(|link| {
            matches!(link, Link::Down)
                .then(|| Link::Welcomed(Session::new(out, host)))
        });
    }

    /// Welcomed to Ready: this host's requests leave on the session.
    pub(crate) fn ready(&self) {
        self.shift(|link| match link {
            Link::Welcomed(session) => {
                Some(Link::Ready(session.clone()))
            }
            Link::Down | Link::Ready(_) => None,
        });
    }

    /// Any state to Down. The session's requests out fail with
    /// `link.lost`, and its later ones with `link.down`.
    pub(crate) fn down(&self) {
        debug_assert_eq!(self.0.ours, Role::Rust);
        self.0.starting.send_replace(false);
        let lost = self.shift(|link| {
            (!matches!(link, Link::Down)).then_some(Link::Down)
        });
        if let Some(Link::Welcomed(session) | Link::Ready(session)) =
            lost
        {
            session.lose();
        }
    }

    fn shift(
        &self,
        next: impl FnOnce(&Link) -> Option<Link>,
    ) -> Option<Link> {
        let mut link = self.0.link.lock();
        let next = next(&link)?;
        let replaced = std::mem::replace(&mut *link, next);
        self.0.state.send_replace(link.state());
        Some(replaced)
    }

    /// The action `name` of the actor at `to`, for `stamp`, within
    /// `deadline`. It fails:
    /// - with `link.down` unless the link is Ready, or becomes Ready
    ///   within `deadline` while a child starts;
    /// - with `link.lost` when the link goes down with the call out;
    /// - with `actor.action_timed_out` at the deadline;
    /// - otherwise with the other host's error, as it gave it.
    pub(crate) async fn call(
        &self,
        stamp: &Stamp,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        let caller = self.caller_of(stamp)?;
        let asked = tokio::time::Instant::now();
        let session = self.session_within(Some(deadline)).await?;
        let deadline = deadline.saturating_sub(asked.elapsed());
        let deadline_ms =
            u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX);
        let ask = Ask::Call {
            name: name.to_owned(),
            args,
            deadline_ms,
        };
        session.ask(caller, to, ask, Some(deadline)).await
    }

    /// Makes the actor at `at` with `input`, for `stamp`. Fails as
    /// [`Port::call`] does. It has no deadline of its own.
    pub(crate) async fn create(
        &self,
        stamp: &Stamp,
        at: &Address,
        input: Vec<u8>,
    ) -> Result<()> {
        let caller = self.caller_of(stamp)?;
        let ask = Ask::Create { input };
        self.session_within(None)
            .await?
            .ask(caller, at, ask, None)
            .await
            .map(drop)
    }

    /// Asks the other host to follow `event` of the actor at `to`,
    /// for `stamp`, on the session that is Ready, and waits for its
    /// answer within `within`. What happens when that session is lost
    /// is the caller's.
    ///
    /// - [`Unanswered::Link`] with `link.down`, at once, unless the
    ///   link is Ready, and with `link.lost` when the link goes down
    ///   with the ask out.
    /// - [`Unanswered::Refused`] with the other host's error, with
    ///   `actor.not_ready` at `within`, and with
    ///   `actor.invalid_operation` for a stamp this host may not ask
    ///   with.
    pub(crate) async fn subscribe(
        &self,
        stamp: &Stamp,
        to: &Address,
        event: &str,
        within: Duration,
    ) -> Result<Routed, Unanswered> {
        let caller =
            self.caller_of(stamp).map_err(Unanswered::Refused)?;
        let session = self.session().map_err(Unanswered::Link)?;
        session.subscribe(caller, to, event, within).await
    }

    /// The Ready session, waited for within `within` while a child
    /// starts. Fails with `link.down` when no child starts, when the
    /// starting one's link goes down, and at `within`.
    async fn session_within(
        &self,
        within: Option<Duration>,
    ) -> Result<Arc<Session>> {
        let mut state = self.state();
        let mut starting = self.0.starting.subscribe();
        let waited = async {
            loop {
                if let Ok(session) = self.session() {
                    return Ok(session);
                }
                if !*starting.borrow_and_update() {
                    return Err(Down.build());
                }
                tokio::select! {
                    _ = state.changed() => {}
                    _ = starting.changed() => {}
                }
            }
        };
        match within {
            Some(within) => tokio::time::timeout(within, waited)
                .await
                .unwrap_or_else(|_| Err(Down.build())),
            None => waited.await,
        }
    }

    fn session(&self) -> Result<Arc<Session>> {
        match &*self.0.link.lock() {
            Link::Ready(session) => Ok(session.clone()),
            Link::Down | Link::Welcomed(_) => Err(Down.build()),
        }
    }

    fn caller_of<'a>(
        &self,
        stamp: &'a Stamp,
    ) -> Result<Option<&'a Caller>> {
        let caller = stamp.caller();
        let reason = match (stamp.origin(), caller) {
            (Origin::Here, Some(Caller::Actor(_))) => {
                return Ok(caller);
            }
            (Origin::Here, _) if self.0.ours.has_a_program() => {
                return Ok(caller);
            }
            (Origin::Here, _) => {
                "the Node host asks only as one of its actors"
            }
            (Origin::Peer, _) => {
                "an operation the other host asked for runs here"
            }
        };
        Err(ActorRuntime::InvalidOperation {
            operation: "ask across the link".to_owned(),
            reason: reason.to_owned(),
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
                return Err(Broken::OutOfTurn(frame.name()));
            }
        };
        let theirs = self.0.theirs.role;
        match frame {
            frame @ Frame::Request {
                ask: Ask::Subscribe { .. },
                ..
            } if !theirs.has_a_program() => {
                return Err(Broken::OutOfTurn(frame.name()));
            }
            Frame::Request { id, from, to, ask } => {
                session.run(id, self.0.theirs.vet(from)?, to, ask);
            }
            Frame::Reply { id, result } => session.settle(id, result),
            Frame::HostEvent(event) if theirs == Role::Node => {
                session.report(event);
            }
            Frame::Event { sub, args } if theirs == Role::Node => {
                session.deliver(sub, args);
            }
            Frame::Gap { sub } if theirs == Role::Node => {
                session.gap(sub);
            }
            Frame::SubscriptionEnded { sub, error }
                if theirs == Role::Node =>
            {
                session.end(sub, error);
            }
            Frame::Unsubscribe { sub } if theirs == Role::Rust => {
                session.unsubscribe(sub);
            }
            Frame::Goodbye if theirs == Role::Rust => {
                return Ok(Taken::Goodbye);
            }
            frame => return Err(Broken::OutOfTurn(frame.name())),
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

/// The requests this side has out, the subscriptions it follows and
/// those it serves, by id.
struct Pending {
    next: u64,
    waiting:
        HashMap<u64, oneshot::Sender<Result<Vec<u8>, WireError>>>,
    /// By this side's request id: where the other host's frames for
    /// that subscription go.
    following: HashMap<u64, Sink>,
    /// By the other side's request id: dropped, the sender ends the
    /// task that serves that subscription.
    serving: HashMap<u64, oneshot::Sender<()>>,
    lost: bool,
}

/// The reply to one request out, once it comes.
type Answered = oneshot::Receiver<Result<Vec<u8>, WireError>>;

impl Pending {
    /// The waiter for the reply to the request `id`, registered at
    /// once. Fails with `link.down` on a lost session.
    fn expect(&mut self, id: u64) -> Result<Answered> {
        if self.lost {
            return Err(Down.build());
        }
        let (answer, answered) = oneshot::channel();
        self.waiting.insert(id, answer);
        Ok(answered)
    }
}

impl Session {
    fn new(out: Writer, host: Inbound) -> Arc<Self> {
        Arc::new(Self {
            out,
            host,
            pending: Mutex::new(Pending {
                next: 0,
                waiting: HashMap::new(),
                following: HashMap::new(),
                serving: HashMap::new(),
                lost: false,
            }),
        })
    }

    /// A new request's id, and its frame's bytes.
    fn request(
        &self,
        caller: Option<&Caller>,
        to: &Address,
        ask: Ask,
    ) -> std::io::Result<(u64, Vec<u8>)> {
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
        Ok((id, bytes))
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
        let (id, bytes) = self.request(caller, to, ask)?;
        let answered = self.pending.lock().expect(id)?;
        let _forget = Forget {
            pending: &self.pending,
            id,
        };
        let asked = async {
            self.exchange(bytes, answered)
                .await?
                .map_err(WireError::into_error)
        };
        match within {
            Some(within) => tokio::time::timeout(within, asked)
                .await
                .map_err(|_| ActionTimedOut.build())?,
            None => asked.await,
        }
    }

    /// Queues a request and waits for its reply. Fails with
    /// `link.down` when nothing was queued, and with `link.lost` when
    /// the session is lost with the request out.
    async fn exchange(
        &self,
        bytes: Vec<u8>,
        answered: Answered,
    ) -> Result<Result<Vec<u8>, WireError>> {
        self.out.queue(bytes).await.map_err(|_| Down.build())?;
        answered.await.map_err(|_| Lost.build())
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
        match ask {
            Ask::Call {
                name,
                args,
                deadline_ms,
            } => {
                let host = self.host.clone();
                let deadline = Duration::from_millis(deadline_ms);
                self.answer(id, async move {
                    host.call(from, &to, &name, args, deadline).await
                });
            }
            Ask::Create { input } => {
                let host = self.host.clone();
                self.answer(id, async move {
                    host.create(from, &to, input)
                        .await
                        .map(|()| Vec::new())
                });
            }
            Ask::Subscribe { event } => {
                self.serve(id, from, to, event);
            }
        }
    }

    fn answer(
        self: &Arc<Self>,
        id: u64,
        work: impl Future<Output = Result<Vec<u8>>> + Send + 'static,
    ) {
        let session = self.clone();
        self.host.runtime().spawn(async move {
            let result = work.await;
            session.reply(id, result).await;
        });
    }

    async fn reply(&self, id: u64, result: Result<Vec<u8>>) {
        let result = result
            .map(ByteBuf::from)
            .map_err(|error| WireError::of(&error));
        let bytes =
            encode(&Frame::Reply { id, result }).or_else(|error| {
                encode(&Frame::Reply {
                    id,
                    result: Err(WireError::of(&error.into())),
                })
            });
        if let Ok(bytes) = bytes {
            let _ = self.out.queue(bytes).await;
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
    /// with `link.down`, and forgets the subscriptions it follows and
    /// serves.
    fn lose(&self) {
        let mut pending = self.pending.lock();
        pending.lost = true;
        pending.waiting.clear();
        pending.following.clear();
        pending.serving.clear();
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

mod follow;
#[cfg(test)]
mod tests;
