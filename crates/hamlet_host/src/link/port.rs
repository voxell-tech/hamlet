//! The host's port to the host at the other end of the link: the
//! link's state as one value, and the session behind it, one child's
//! pipe with the requests in flight on it.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hamlet_actor_error::ActorError;
use parking_lot::Mutex;
use serde_bytes::ByteBuf;
use tokio::sync::{oneshot, watch};

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

/// The host's way to the host at the other end of the link. It
/// outlives each child: the directory holds it from the host's open,
/// and the side's loop moves it as children come and go. It is the
/// one holder of the link's state and of what this host knows of the
/// other.
#[derive(Clone)]
pub(crate) struct Port(Arc<Shared>);

struct Shared {
    /// Which host this is.
    ours: Role,
    /// The other host: its role and the kinds it runs, fixed for
    /// this host's life. The directory maps these kinds to this
    /// port, `answer` checks a hello against them, and every request
    /// the other host writes is vetted against them.
    theirs: Side,
    link: Mutex<Link>,
    /// Told of each move of `link`, under its lock, so it never
    /// states a state `link` is not in.
    state: watch::Sender<LinkState>,
}

/// The link, as one value. A state that answers or asks always has
/// its session.
enum Link {
    /// No session. A request out fails with `link.down`.
    Down,
    /// The Rust host has welcomed its child: the child's requests are
    /// answered on this session, and this host's own still fail with
    /// `link.down`.
    Welcomed(Arc<Session>),
    /// Requests go out on this session and come in on it.
    Ready(Arc<Session>),
}

/// What became of a frame.
pub(crate) enum Taken {
    Handled,
    /// The Rust host said goodbye.
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

    /// The link's state, which changes only when it moves.
    pub(crate) fn state(&self) -> watch::Receiver<LinkState> {
        self.0.state.subscribe()
    }

    /// Down to Welcomed: a new session on `out`, whose requests from
    /// the other host reach `host`. The Rust host calls it at a hello
    /// it welcomes. The Node host calls it at the welcome, once its
    /// folder is open.
    pub(crate) fn welcomed(&self, out: Writer, host: Inbound) {
        let _ = (out, host);
        todo!("U5, the link unit: a session at the welcome")
    }

    /// Welcomed to Ready: this host's requests leave on the session.
    /// The Rust host calls it at the child's `Ready`. The Node host
    /// calls it right after `welcomed`, before boot.
    pub(crate) fn ready(&self) {
        todo!("U5, the link unit: the link is ready")
    }

    /// Any state to Down. The session's requests out fail with
    /// `link.lost`, and its later ones with `link.down`. Only the
    /// Rust host comes here, because the Node host ends with its
    /// link. Drops the port's hold on the session's writer; the
    /// supervisor keeps its own clone for the goodbye.
    pub(crate) fn down(&self) {
        todo!("U5, the link unit: the link goes down")
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
        let _ = (caller, to, name, args, deadline);
        todo!("U5, the link unit: a call across the link")
    }

    /// Makes the actor at `at` with `input`, for `caller`. Fails as
    /// [`Port::call`] does. It has no deadline of its own: the asking
    /// host's start path bounds it with its start deadline.
    pub(crate) async fn create(
        &self,
        caller: Option<&Caller>,
        at: &Address,
        input: Vec<u8>,
    ) -> Result<()> {
        let _ = (caller, at, input);
        todo!("U5, the link unit: a create across the link")
    }

    /// Fails with `actor.invalid_operation`, before anything is
    /// written, for a caller this host does not ask for. An actor's
    /// stamp here was made by `HostRef::current` for a generation of
    /// this host, so its kind is this host's. Only none and a client
    /// depend on which host this is.
    fn may_ask(&self, caller: Option<&Caller>) -> Result<()> {
        let _ = caller;
        todo!("U5, the link unit: who this host asks for")
    }

    /// Takes one frame the other host wrote after the handshake. This
    /// is the one place a peer's frame is judged: an `Err` is the
    /// link's end.
    pub(crate) fn take(&self, frame: Frame) -> Result<Taken, Broken> {
        let _ = frame;
        todo!("U5, the link unit: a frame after the handshake")
    }
}

/// One child's pipe, as one side sees it: the pipe's one writer, the
/// host that the other side's requests reach, and this side's
/// requests out.
struct Session {
    out: Writer,
    host: Inbound,
    pending: Mutex<Pending>,
}

/// The requests this side has out, by id. An id belongs to this side
/// and is never used twice in a session.
struct Pending {
    next: u64,
    waiting:
        HashMap<u64, oneshot::Sender<Result<Vec<u8>, WireError>>>,
    /// Set by [`Session::lose`]: no request registers after it.
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
        let _ = (caller, to, ask, within);
        todo!("U5, the link unit: a request and its reply")
    }

    /// Runs the other host's request on this host's runtime, never on
    /// the link's thread, and writes its reply.
    fn run(
        self: &Arc<Self>,
        id: u64,
        from: Vetted,
        to: Address,
        ask: Ask,
    ) {
        let _ = (id, from, to, ask);
        todo!("U5, the link unit: a peer's request runs here")
    }

    /// Writes the reply to the request `id`, with `WireError::of` for
    /// an error. An answer too large for a frame is answered instead
    /// with an uncoded error that names its length and the limit, so
    /// the asker learns at once and the link stays up. A reply that
    /// cannot be written is dropped, and the asking side learns of
    /// the link's end by itself.
    async fn reply(&self, id: u64, result: Result<Vec<u8>>) {
        let _ = (id, result);
        todo!("U5, the link unit: a reply")
    }

    /// Settles the request `id`. A reply to an id that is not out
    /// (its call hit its deadline, or was dropped) is dropped, not a
    /// fault.
    fn settle(&self, id: u64, result: Result<ByteBuf, WireError>) {
        let _ = (id, result);
        todo!("U5, the link unit: a reply settles its request")
    }

    /// Passes an event of the Node host's to this host's
    /// `Host::events`.
    fn report(&self, event: HostEvent) {
        let _ = event;
        todo!("U5, the link unit: the Node host's event")
    }

    /// Fails each request out with `link.lost`, and every later one
    /// with `link.down`. Only [`Port::down`] calls it.
    fn lose(&self) {
        todo!("U5, the link unit: the session is lost")
    }
}
