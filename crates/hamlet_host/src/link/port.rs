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
        let mut link = self.0.link.lock();
        if let Link::Down = *link {
            *link = Link::Welcomed(Session::new(out, host));
            self.0.state.send_replace(LinkState::Welcomed);
        }
    }

    /// Welcomed to Ready: this host's requests leave on the session.
    /// The Rust host calls it at the child's `Ready`. The Node host
    /// calls it right after `welcomed`, before boot.
    pub(crate) fn ready(&self) {
        let mut link = self.0.link.lock();
        if let Link::Welcomed(session) = &*link {
            *link = Link::Ready(session.clone());
            self.0.state.send_replace(LinkState::Ready);
        }
    }

    /// Any state to Down. The session's requests out fail with
    /// `link.lost`, and its later ones with `link.down`. Only the
    /// Rust host comes here, because the Node host ends with its
    /// link. Drops the port's hold on the session's writer; the
    /// supervisor keeps its own clone for the goodbye.
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
    /// [`Port::call`] does. It has no deadline of its own: the asking
    /// host's start path bounds it with its start deadline.
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

    /// The session this host's requests leave on, or `link.down`
    /// unless the link is Ready.
    fn session(&self) -> Result<Arc<Session>> {
        match &*self.0.link.lock() {
            Link::Ready(session) => Ok(session.clone()),
            Link::Down | Link::Welcomed(_) => Err(Down.build()),
        }
    }

    /// Fails with `actor.invalid_operation`, before anything is
    /// written, for a caller this host does not ask for. An actor's
    /// stamp here was made by `HostRef::current` for a generation of
    /// this host, so its kind is this host's. Only none and a client
    /// depend on which host this is.
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

    /// Takes one frame the other host wrote after the handshake. This
    /// is the one place a peer's frame is judged: an `Err` is the
    /// link's end.
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

    /// Runs the other host's request on this host's runtime, never on
    /// the link's thread, and writes its reply.
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

    /// Writes the reply to the request `id`, with `WireError::of` for
    /// an error. An answer too large for a frame is answered instead
    /// with an uncoded error that names its length and the limit, so
    /// the asker learns at once and the link stays up. A reply that
    /// cannot be written is dropped, and the asking side learns of
    /// the link's end by itself.
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

    /// Settles the request `id`. A reply to an id that is not out
    /// (its call hit its deadline, or was dropped) is dropped, not a
    /// fault.
    fn settle(&self, id: u64, result: Result<ByteBuf, WireError>) {
        let waiting = self.pending.lock().waiting.remove(&id);
        if let Some(waiting) = waiting {
            let _ = waiting.send(result.map(ByteBuf::into_vec));
        }
    }

    /// Passes an event of the Node host's to this host's
    /// `Host::events`.
    fn report(&self, event: HostEvent) {
        self.host.report(event);
    }

    /// Fails each request out with `link.lost`, and every later one
    /// with `link.down`. Only [`Port::down`] calls it.
    fn lose(&self) {
        let mut pending = self.pending.lock();
        pending.lost = true;
        pending.waiting.clear();
    }
}

/// Forgets the request `id` when its wait ends, however it ends.
struct Forget<'a> {
    pending: &'a Mutex<Pending>,
    id: u64,
}

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        self.pending.lock().waiting.remove(&self.id);
    }
}

/// The name `frame` has on the wire.
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
mod tests {
    use std::io::{self, PipeReader};
    use std::thread;

    use hamlet_testing::Scratch;
    use tokio::sync::mpsc;
    use tokio::task::JoinHandle;

    use super::*;
    use crate::link::{AtBreak, FRAME_LIMIT, read};
    use crate::route::Operation;
    use crate::{Host, HostOptions};

    const TEN_S: Duration = Duration::from_secs(10);
    const SHORT: Duration = Duration::from_millis(50);

    fn at(kind: &str, key: &str) -> Address {
        Address::new(kind, key).unwrap()
    }

    /// The error's `group.code`.
    fn code(error: &anyhow::Error) -> String {
        let error = ActorError::extract(error);
        format!("{}.{}", error.group(), error.code())
    }

    fn reply(id: u64, output: &[u8]) -> Frame {
        Frame::Reply {
            id,
            result: Ok(ByteBuf::from(output)),
        }
    }

    /// The frames read from `input`, until it ends.
    fn frames(
        mut input: PipeReader,
    ) -> mpsc::UnboundedReceiver<Frame> {
        let (sender, frames) = mpsc::unbounded_channel();
        thread::spawn(move || {
            while let Ok(Some(frame)) = read(&mut input) {
                let _ = sender.send(frame);
            }
        });
        frames
    }

    /// Calls `slow` at `to` through `port`, in a task of its own.
    fn call(
        port: &Port,
        to: Address,
        deadline: Duration,
    ) -> JoinHandle<Result<Vec<u8>>> {
        let port = port.clone();
        tokio::spawn(async move {
            port.call(None, &to, "slow", Vec::new(), deadline).await
        })
    }

    /// A port of role `ours`, on a host that runs no kind, with the
    /// pipe its session writes to. The other host runs `agent` when
    /// it is the Node host, and `realm` when it is the Rust host.
    struct Linked {
        port: Port,
        out: Writer,
        frames: mpsc::UnboundedReceiver<Frame>,
        host: Host,
        _home: Scratch,
    }

    impl Linked {
        fn new(ours: Role) -> Self {
            let home = Scratch::new("link-port");
            let host = Host::open_with(
                &home,
                HashMap::new(),
                HostOptions::default(),
            )
            .unwrap();
            let (input, output) = io::pipe().unwrap();
            let theirs = match ours {
                Role::Rust => "agent",
                Role::Node => "realm",
            };
            Self {
                port: Port::new(ours, [theirs.to_owned()]),
                out: Writer::start(
                    output,
                    "test-link-output",
                    AtBreak::Close,
                )
                .unwrap(),
                frames: frames(input),
                host,
                _home: home,
            }
        }

        fn ready(ours: Role) -> Self {
            let linked = Self::new(ours);
            linked.welcome();
            linked.port.ready();
            linked
        }

        fn welcome(&self) {
            self.port.welcomed(
                self.out.clone(),
                Inbound::of(&self.host).unwrap(),
            );
        }

        fn session(&self) -> Arc<Session> {
            match &*self.port.0.link.lock() {
                Link::Welcomed(session) | Link::Ready(session) => {
                    session.clone()
                }
                Link::Down => panic!("the link is down"),
            }
        }

        fn waiting(&self) -> usize {
            self.session().pending.lock().waiting.len()
        }

        async fn next(&mut self) -> Frame {
            tokio::time::timeout(TEN_S, self.frames.recv())
                .await
                .unwrap()
                .unwrap()
        }

        /// Asserts that nothing was written since the last frame
        /// read, by writing a frame behind it.
        async fn nothing_written(&mut self) {
            self.out.write(&Frame::Goodbye).await.unwrap();
            assert_eq!(self.next().await, Frame::Goodbye);
        }
    }

    /// The codes of a call and of a create with no caller, each of
    /// which fails.
    async fn asked(port: &Port, to: &Address) -> [String; 2] {
        let called = port
            .call(None, to, "who", Vec::new(), TEN_S)
            .await
            .unwrap_err();
        let created =
            port.create(None, to, Vec::new()).await.unwrap_err();
        [code(&called), code(&created)]
    }

    #[tokio::test]
    async fn the_node_hosts_program_asks_nothing_across_the_link() {
        let mut node = Linked::ready(Role::Node);
        let realm = at("realm", "r");
        for caller in [None, Some(Caller::Client)] {
            let called = node
                .port
                .call(
                    caller.as_ref(),
                    &realm,
                    "who",
                    Vec::new(),
                    TEN_S,
                )
                .await
                .unwrap_err();
            let created = node
                .port
                .create(caller.as_ref(), &realm, Vec::new())
                .await
                .unwrap_err();
            assert_eq!(
                [code(&called), code(&created)],
                ["actor.invalid_operation"; 2]
            );
        }
        node.nothing_written().await;

        let agent = Caller::Actor(at("agent", "a"));
        let called = tokio::spawn({
            let (port, agent) = (node.port.clone(), agent.clone());
            async move {
                port.call(
                    Some(&agent),
                    &realm,
                    "who",
                    Vec::new(),
                    TEN_S,
                )
                .await
            }
        });
        let Frame::Request { id, from, .. } = node.next().await
        else {
            panic!("a request");
        };
        assert_eq!(from, Some(agent));
        assert!(matches!(
            node.port.take(reply(id, b"r")),
            Ok(Taken::Handled)
        ));
        assert_eq!(called.await.unwrap().unwrap(), b"r");
    }

    #[tokio::test]
    async fn a_request_never_written_fails_with_link_down() {
        let mut rust = Linked::new(Role::Rust);
        let agent = at("agent", "a");
        assert_eq!(asked(&rust.port, &agent).await, ["link.down"; 2]);
        rust.welcome();
        assert_eq!(asked(&rust.port, &agent).await, ["link.down"; 2]);
        rust.port.ready();
        rust.port.down();
        assert_eq!(asked(&rust.port, &agent).await, ["link.down"; 2]);
        rust.nothing_written().await;

        // Ready, on a pipe whose writer has stopped.
        let (input, output) = io::pipe().unwrap();
        drop(input);
        let out =
            Writer::start(output, "test-link-output", AtBreak::Close)
                .unwrap();
        tokio::time::timeout(TEN_S, async {
            while out.write(&Frame::Goodbye).await.is_ok() {}
        })
        .await
        .unwrap();
        let broken = Port::new(Role::Rust, ["agent".to_owned()]);
        broken.welcomed(out, Inbound::of(&rust.host).unwrap());
        broken.ready();
        assert_eq!(asked(&broken, &agent).await, ["link.down"; 2]);
    }

    #[tokio::test]
    async fn a_dropped_call_leaves_nothing_pending() {
        let mut rust = Linked::ready(Role::Rust);
        let agent = at("agent", "a");
        let dropped = tokio::time::timeout(
            SHORT,
            rust.port.call(None, &agent, "slow", Vec::new(), TEN_S),
        )
        .await;
        assert!(dropped.is_err());
        assert!(matches!(rust.next().await, Frame::Request { .. }));
        assert_eq!(rust.waiting(), 0);

        let timed_out = rust
            .port
            .call(None, &agent, "slow", Vec::new(), SHORT)
            .await
            .unwrap_err();
        assert_eq!(code(&timed_out), "actor.action_timed_out");
        assert!(matches!(rust.next().await, Frame::Request { .. }));
        assert_eq!(rust.waiting(), 0);
    }

    #[tokio::test]
    async fn a_reply_to_an_id_that_is_not_out_is_dropped() {
        let mut rust = Linked::ready(Role::Rust);
        let agent = at("agent", "a");
        let timed_out = rust
            .port
            .call(None, &agent, "slow", Vec::new(), SHORT)
            .await
            .unwrap_err();
        assert_eq!(code(&timed_out), "actor.action_timed_out");
        let Frame::Request { id: late, .. } = rust.next().await
        else {
            panic!("a request");
        };
        for id in [late, late + 100] {
            assert!(matches!(
                rust.port.take(reply(id, b"late")),
                Ok(Taken::Handled)
            ));
        }

        let next = call(&rust.port, agent, TEN_S);
        let Frame::Request { id, .. } = rust.next().await else {
            panic!("a request");
        };
        assert_ne!(id, late);
        assert!(matches!(
            rust.port.take(reply(id, b"next")),
            Ok(Taken::Handled)
        ));
        assert_eq!(next.await.unwrap().unwrap(), b"next");
    }

    #[tokio::test]
    async fn the_state_follows_the_link() {
        let mut rust = Linked::new(Role::Rust);
        let mut state = rust.port.state();
        assert_eq!(*state.borrow_and_update(), LinkState::Down);
        rust.welcome();
        assert!(state.has_changed().unwrap());
        assert_eq!(*state.borrow_and_update(), LinkState::Welcomed);
        rust.port.ready();
        assert_eq!(*state.borrow_and_update(), LinkState::Ready);
        rust.port.ready();
        assert!(!state.has_changed().unwrap());
        assert_eq!(*rust.port.state().borrow(), LinkState::Ready);

        let out = call(&rust.port, at("agent", "a"), TEN_S);
        assert!(matches!(rust.next().await, Frame::Request { .. }));
        rust.port.down();
        assert_eq!(*state.borrow_and_update(), LinkState::Down);
        assert_eq!(
            code(&out.await.unwrap().unwrap_err()),
            "link.lost"
        );
        rust.port.down();
        assert!(!state.has_changed().unwrap());
    }

    /// The rows of the tables of who may write which frame that the
    /// other host's role decides.
    #[tokio::test]
    async fn a_frame_is_taken_only_from_the_host_that_may_write_it() {
        let out_of_turn = |taken| match taken {
            Err(Broken::OutOfTurn(frame)) => frame,
            _ => panic!("a frame out of turn"),
        };
        let event = HostEvent::Refused {
            from: Caller::Actor(at("agent", "a")),
            to: at("agent", "b"),
            operation: Operation::Create,
        };

        let rust = Linked::new(Role::Rust);
        assert_eq!(
            out_of_turn(rust.port.take(reply(1, b""))),
            "reply"
        );
        rust.welcome();
        let mut events = rust.host.events();
        assert!(matches!(
            rust.port.take(Frame::HostEvent(event.clone())),
            Ok(Taken::Handled)
        ));
        assert_eq!(events.try_recv().unwrap(), event);
        assert_eq!(
            out_of_turn(rust.port.take(Frame::Goodbye)),
            "goodbye"
        );
        rust.port.ready();
        assert_eq!(
            out_of_turn(rust.port.take(Frame::Ready)),
            "ready"
        );

        let node = Linked::ready(Role::Node);
        assert_eq!(
            out_of_turn(node.port.take(Frame::HostEvent(event))),
            "host_event"
        );
        assert!(matches!(
            node.port.take(Frame::Goodbye),
            Ok(Taken::Goodbye)
        ));
    }

    /// This host runs no kind, so each request it runs answers
    /// `actor.not_registered`.
    #[tokio::test]
    async fn a_peers_request_is_vetted_run_and_answered() {
        let mut rust = Linked::ready(Role::Rust);
        let request = |id, from, ask| Frame::Request {
            id,
            from,
            to: at("realm", "r"),
            ask,
        };
        let agent = Some(Caller::Actor(at("agent", "a")));
        let asks = [
            Ask::Call {
                name: "who".to_owned(),
                args: Vec::new(),
                deadline_ms: u64::MAX,
            },
            Ask::Create { input: Vec::new() },
        ];
        for (id, ask) in (1..).zip(asks) {
            assert!(matches!(
                rust.port.take(request(id, agent.clone(), ask)),
                Ok(Taken::Handled)
            ));
            let Frame::Reply {
                id: answered,
                result: Err(error),
            } = rust.next().await
            else {
                panic!("an error");
            };
            assert_eq!(
                (answered, error.group.as_str(), error.code.as_str()),
                (id, "actor", "not_registered")
            );
        }
        for from in [
            Some(Caller::Actor(at("realm", "x"))),
            Some(Caller::Client),
            None,
        ] {
            let ask = Ask::Create { input: Vec::new() };
            assert!(matches!(
                rust.port.take(request(9, from.clone(), ask)),
                Err(Broken::Stamp(stamp)) if stamp == from
            ));
        }
        rust.nothing_written().await;
    }

    #[tokio::test]
    async fn a_request_or_reply_over_the_limit_is_an_uncoded_error() {
        let mut rust = Linked::ready(Role::Rust);
        let over = format!(" bytes, over the limit of {FRAME_LIMIT}");
        let asked = rust
            .port
            .call(
                None,
                &at("agent", "a"),
                "big",
                vec![0; FRAME_LIMIT],
                TEN_S,
            )
            .await
            .unwrap_err();
        assert_eq!(code(&asked), "core.internal_error");
        assert!(format!("{asked:#}").contains(&over));
        assert_eq!(rust.waiting(), 0);
        rust.nothing_written().await;

        rust.session().reply(4, Ok(vec![0; FRAME_LIMIT])).await;
        let Frame::Reply {
            id: 4,
            result: Err(error),
        } = rust.next().await
        else {
            panic!("an error");
        };
        assert_eq!(
            (error.group.as_str(), error.code.as_str()),
            ("core", "internal_error")
        );
        assert!(error.message.contains(&over));
    }
}
