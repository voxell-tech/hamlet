use std::io::{self, PipeReader};
use std::thread;
use std::time::Instant;

use hamlet_testing::Scratch;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::*;
use crate::link::{AtBreak, FRAME_LIMIT, read};
use crate::local::feed::Closed;
use crate::route::Operation;
use crate::testing::code;
use crate::{Host, HostOptions, Item};

const TEN_S: Duration = Duration::from_secs(10);
const SHORT: Duration = Duration::from_millis(50);

/// The subscription id of the frames that fill a pipe.
const FILLER: u64 = u64::MAX;

fn at(kind: &str, key: &str) -> Address {
    Address::new(kind, key).unwrap()
}

fn reply(id: u64, output: &[u8]) -> Frame {
    Frame::Reply {
        id,
        result: Ok(ByteBuf::from(output)),
    }
}

fn refusal() -> WireError {
    WireError {
        group: "actor".to_owned(),
        code: "refused".to_owned(),
        message: "no".to_owned(),
        metadata: None,
    }
}

fn subscribe_request(id: u64, from: Option<Caller>) -> Frame {
    Frame::Request {
        id,
        from,
        to: at("agent", "a"),
        ask: Ask::Subscribe {
            event: "ticked".to_owned(),
        },
    }
}

/// The frames read from `input` once `go` is sent or dropped.
fn frames(
    mut input: PipeReader,
    go: std::sync::mpsc::Receiver<()>,
) -> mpsc::UnboundedReceiver<Frame> {
    let (sender, frames) = mpsc::unbounded_channel();
    thread::spawn(move || {
        let _ = go.recv();
        while let Ok(Some(frame)) = read(&mut input) {
            let _ = sender.send(frame);
        }
    });
    frames
}

fn subscribe(
    port: &Port,
    within: Duration,
) -> JoinHandle<Result<Routed, Unanswered>> {
    let port = port.clone();
    tokio::spawn(async move {
        port.subscribe(
            &Stamp::client(),
            &at("agent", "a"),
            "ticked",
            within,
        )
        .await
    })
}

async fn item(routed: &mut Routed) -> Result<Item, Closed> {
    tokio::time::timeout(TEN_S, routed.next()).await.unwrap()
}

fn ended(next: Result<Item, Closed>) -> String {
    match next {
        Err(Closed::Ended(why)) => code(&why.to_error()),
        Err(Closed::Dropped) => "dropped".to_owned(),
        Ok(item) => panic!("expected the end, got {item:?}"),
    }
}

fn call(
    port: &Port,
    to: Address,
    deadline: Duration,
) -> JoinHandle<Result<Vec<u8>>> {
    let port = port.clone();
    tokio::spawn(async move {
        port.call(&Stamp::PROGRAM, &to, "slow", Vec::new(), deadline)
            .await
    })
}

struct Linked {
    port: Port,
    out: Writer,
    frames: mpsc::UnboundedReceiver<Frame>,
    /// Holds back the reading of the pipe while it is set.
    unread: Option<std::sync::mpsc::Sender<()>>,
    host: Host,
    _home: Scratch,
}

impl Linked {
    fn new(ours: Role) -> Self {
        let mut linked = Self::unread(ours);
        linked.unread = None;
        linked
    }

    /// A port whose pipe is not read until [`Linked::after_fillers`].
    fn unread(ours: Role) -> Self {
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
        let (unread, go) = std::sync::mpsc::channel();
        Self {
            port: Port::new(ours, [theirs.to_owned()]),
            out: Writer::start(
                output,
                "test-link-output",
                AtBreak::Close,
            )
            .unwrap(),
            frames: frames(input, go),
            unread: Some(unread),
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

    fn following(&self) -> usize {
        self.session().pending.lock().following.len()
    }

    fn serving(&self) -> usize {
        self.session().pending.lock().serving.len()
    }

    async fn next(&mut self) -> Frame {
        tokio::time::timeout(TEN_S, self.frames.recv())
            .await
            .unwrap()
            .unwrap()
    }

    /// Waits first, so a frame a spawned task writes comes before the
    /// marker.
    async fn nothing_written(&mut self) {
        tokio::time::sleep(SHORT).await;
        self.out.write(&Frame::Goodbye).await.unwrap();
        assert_eq!(self.next().await, Frame::Goodbye);
    }

    /// A subscription this port asked for and the other host
    /// answered, and its id.
    async fn followed(&mut self) -> (Routed, u64) {
        let asked = subscribe(&self.port, TEN_S);
        let Frame::Request {
            id,
            ask: Ask::Subscribe { .. },
            ..
        } = self.next().await
        else {
            panic!("a subscribe");
        };
        assert!(matches!(
            self.port.take(reply(id, b"")),
            Ok(Taken::Handled)
        ));
        let Ok(routed) = asked.await.unwrap() else {
            panic!("a subscription");
        };
        (routed, id)
    }

    /// Fills the unread pipe and the writer's queue, so that the next
    /// write waits.
    async fn fill(&self) {
        let filler = Frame::Event {
            sub: FILLER,
            args: vec![0; 64 << 10],
        };
        while tokio::time::timeout(SHORT, self.out.write(&filler))
            .await
            .is_ok()
        {}
    }

    /// Reads the pipe, and returns what was written other than the
    /// fillers, up to a marker queued after what waits to be written.
    async fn after_fillers(&mut self) -> Vec<Frame> {
        tokio::time::sleep(SHORT).await;
        let out = self.out.clone();
        let marker =
            tokio::spawn(
                async move { out.write(&Frame::Goodbye).await },
            );
        tokio::time::sleep(SHORT).await;
        self.unread = None;
        let mut written = Vec::new();
        loop {
            match self.next().await {
                Frame::Goodbye => break,
                Frame::Event { sub: FILLER, .. } => {}
                frame => written.push(frame),
            }
        }
        marker.await.unwrap().unwrap();
        written
    }
}

async fn asked(port: &Port, to: &Address) -> [String; 2] {
    let called = port
        .call(&Stamp::PROGRAM, to, "who", Vec::new(), TEN_S)
        .await
        .unwrap_err();
    let created = port
        .create(&Stamp::PROGRAM, to, Vec::new())
        .await
        .unwrap_err();
    [code(&called), code(&created)]
}

#[tokio::test]
async fn the_node_hosts_program_asks_nothing_across_the_link() {
    let mut node = Linked::ready(Role::Node);
    let realm = at("realm", "r");
    let peer = Side::new(Role::Rust, ["realm".to_owned()])
        .vet(Some(Caller::Actor(at("realm", "x"))))
        .unwrap();
    for stamp in
        [Stamp::PROGRAM, Stamp::client(), Stamp::from_peer(peer)]
    {
        let called = node
            .port
            .call(&stamp, &realm, "who", Vec::new(), TEN_S)
            .await
            .unwrap_err();
        let created = node
            .port
            .create(&stamp, &realm, Vec::new())
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
        let port = node.port.clone();
        async move {
            let stamp = Stamp::actor(at("agent", "a"));
            port.call(&stamp, &realm, "who", Vec::new(), TEN_S).await
        }
    });
    let Frame::Request { id, from, .. } = node.next().await else {
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
async fn a_request_while_a_child_starts_waits_for_its_link() {
    let mut rust = Linked::new(Role::Rust);
    rust.port.starting();
    let called = call(&rust.port, at("agent", "a"), TEN_S);
    let created = tokio::spawn({
        let port = rust.port.clone();
        async move {
            port.create(
                &Stamp::PROGRAM,
                &at("agent", "b"),
                Vec::new(),
            )
            .await
        }
    });
    rust.nothing_written().await;
    rust.welcome();
    rust.nothing_written().await;
    assert!(!called.is_finished() && !created.is_finished());
    rust.port.ready();
    for _ in 0..2 {
        let Frame::Request { id, ask, .. } = rust.next().await else {
            panic!("a request");
        };
        if let Ask::Call { deadline_ms, .. } = ask {
            assert!(deadline_ms < 10_000, "{deadline_ms}");
        }
        assert!(matches!(
            rust.port.take(reply(id, b"r")),
            Ok(Taken::Handled)
        ));
    }
    assert_eq!(called.await.unwrap().unwrap(), b"r");
    created.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_request_waiting_for_a_starting_child_fails_as_its_link_does()
 {
    let mut rust = Linked::new(Role::Rust);
    let agent = at("agent", "a");
    rust.port.starting();
    let at_deadline = call(&rust.port, agent.clone(), SHORT);
    assert_eq!(
        code(&at_deadline.await.unwrap().unwrap_err()),
        "link.down"
    );
    let called = call(&rust.port, agent.clone(), TEN_S);
    rust.welcome();
    rust.port.down();
    assert_eq!(
        code(&called.await.unwrap().unwrap_err()),
        "link.down"
    );
    assert_eq!(asked(&rust.port, &agent).await, ["link.down"; 2]);
    rust.nothing_written().await;
}

#[tokio::test]
async fn a_request_on_a_lost_session_fails_with_link_down() {
    let mut rust = Linked::ready(Role::Rust);
    let session = rust.session();
    rust.port.down();
    let create = Ask::Create { input: Vec::new() };
    let asked = tokio::time::timeout(
        Duration::from_secs(2),
        session.ask(None, &at("agent", "a"), create, None),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(code(&asked), "link.down");
    rust.nothing_written().await;
}

#[tokio::test]
async fn a_dropped_call_leaves_nothing_pending() {
    let mut rust = Linked::ready(Role::Rust);
    let agent = at("agent", "a");
    let dropped = tokio::time::timeout(
        SHORT,
        rust.port.call(
            &Stamp::PROGRAM,
            &agent,
            "slow",
            Vec::new(),
            TEN_S,
        ),
    )
    .await;
    assert!(dropped.is_err());
    assert!(matches!(rust.next().await, Frame::Request { .. }));
    assert_eq!(rust.waiting(), 0);

    let timed_out = rust
        .port
        .call(&Stamp::PROGRAM, &agent, "slow", Vec::new(), SHORT)
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
        .call(&Stamp::PROGRAM, &agent, "slow", Vec::new(), SHORT)
        .await
        .unwrap_err();
    assert_eq!(code(&timed_out), "actor.action_timed_out");
    let Frame::Request { id: late, .. } = rust.next().await else {
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
async fn a_call_with_the_longest_deadline_crosses_and_is_answered() {
    let mut rust = Linked::ready(Role::Rust);
    let called = call(&rust.port, at("agent", "a"), Duration::MAX);
    let Frame::Request {
        id,
        ask: Ask::Call { deadline_ms, .. },
        ..
    } = rust.next().await
    else {
        panic!("a call");
    };
    assert_eq!(deadline_ms, u64::MAX);
    assert!(matches!(
        rust.port.take(reply(id, b"out")),
        Ok(Taken::Handled)
    ));
    assert_eq!(called.await.unwrap().unwrap(), b"out");
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
    assert_eq!(code(&out.await.unwrap().unwrap_err()), "link.lost");
    rust.port.down();
    assert!(!state.has_changed().unwrap());
}

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
    assert_eq!(out_of_turn(rust.port.take(reply(1, b""))), "reply");
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
    assert_eq!(out_of_turn(rust.port.take(Frame::Ready)), "ready");

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

#[tokio::test]
async fn a_peers_request_is_vetted_run_and_answered() {
    let mut rust = Linked::new(Role::Rust);
    rust.welcome();
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
            &Stamp::PROGRAM,
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

#[tokio::test]
async fn taking_an_event_never_waits_on_a_full_queue() {
    let mut rust = Linked::ready(Role::Rust);
    let (mut routed, sub) = rust.followed().await;
    let taken = Instant::now();
    for n in 0..30_000_u32 {
        let args = [&n.to_be_bytes()[..], &[0; 60]].concat();
        assert!(matches!(
            rust.port.take(Frame::Event { sub, args }),
            Ok(Taken::Handled)
        ));
    }
    let took = taken.elapsed();
    assert!(took < Duration::from_secs(1), "{took:?}");
    let mut events = Vec::new();
    loop {
        match item(&mut routed).await {
            Ok(Item::Event(args)) => events.push(u32::from_be_bytes(
                args[..4].try_into().unwrap(),
            )),
            Ok(Item::Gap) => break,
            next => panic!("{}", ended(next)),
        }
    }
    assert!(!events.is_empty());
    assert!(events.iter().copied().eq(0..events.len() as u32));
}

#[tokio::test]
async fn an_event_for_a_subscription_the_session_does_not_route_is_dropped()
 {
    let unknown = || {
        [
            Frame::Event {
                sub: 7,
                args: vec![1],
            },
            Frame::Gap { sub: 7 },
            Frame::SubscriptionEnded {
                sub: 7,
                error: refusal(),
            },
        ]
    };
    let mut rust = Linked::new(Role::Rust);
    rust.welcome();
    for frame in unknown() {
        assert!(matches!(rust.port.take(frame), Ok(Taken::Handled)));
    }
    rust.port.ready();
    for frame in unknown() {
        assert!(matches!(rust.port.take(frame), Ok(Taken::Handled)));
    }
    rust.nothing_written().await;

    let mut node = Linked::ready(Role::Node);
    assert!(matches!(
        node.port.take(Frame::Unsubscribe { sub: 7 }),
        Ok(Taken::Handled)
    ));
    node.nothing_written().await;
}

#[tokio::test]
async fn a_lost_session_forgets_its_subscriptions_and_writes_no_unsubscribe()
 {
    let mut rust = Linked::ready(Role::Rust);
    let session = rust.session();
    let (mut routed, _) = rust.followed().await;
    rust.port.down();
    assert_eq!(session.pending.lock().following.len(), 0);
    assert_eq!(ended(item(&mut routed).await), "dropped");
    drop(routed);
    rust.nothing_written().await;
}

#[tokio::test]
async fn a_subscribe_dropped_behind_a_full_writer_queue_writes_an_unsubscribe_after_it_or_nothing()
 {
    let mut rust = Linked::unread(Role::Rust);
    rust.welcome();
    rust.port.ready();
    rust.fill().await;
    let dropped = tokio::time::timeout(
        SHORT,
        rust.port.subscribe(
            &Stamp::client(),
            &at("agent", "a"),
            "ticked",
            TEN_S,
        ),
    )
    .await;
    assert!(dropped.is_err());
    assert_eq!(rust.following(), 0);
    let written = rust.after_fillers().await;

    let asked = written.iter().position(|frame| {
        matches!(
            frame,
            Frame::Request {
                ask: Ask::Subscribe { .. },
                ..
            }
        )
    });
    if let Some(asked) = asked {
        let Frame::Request { id, .. } = written[asked] else {
            unreachable!();
        };
        assert!(
            written[asked..]
                .contains(&Frame::Unsubscribe { sub: id }),
            "{written:?}"
        );
    }
}

/// The pump answers a subscribe the host refuses, and waits to write
/// that reply behind a full writer's queue.
#[tokio::test]
async fn an_unsubscribe_ends_a_subscribe_that_has_not_answered_yet() {
    for unsubscribed in [false, true] {
        let mut node = Linked::unread(Role::Node);
        node.welcome();
        node.port.ready();
        node.fill().await;
        assert!(matches!(
            node.port
                .take(subscribe_request(3, Some(Caller::Client))),
            Ok(Taken::Handled)
        ));
        tokio::time::sleep(SHORT).await;
        assert_eq!(node.serving(), 1);
        if unsubscribed {
            assert!(matches!(
                node.port.take(Frame::Unsubscribe { sub: 3 }),
                Ok(Taken::Handled)
            ));
            assert_eq!(node.serving(), 0);
        }
        let written = node.after_fillers().await;

        let replied = written
            .iter()
            .any(|frame| matches!(frame, Frame::Reply { id: 3, .. }));
        assert_eq!(replied, !unsubscribed, "{written:?}");
        assert_eq!(node.serving(), 0);
    }
}

#[tokio::test]
async fn a_refused_subscribe_leaves_no_route_and_writes_no_unsubscribe()
 {
    let mut rust = Linked::ready(Role::Rust);
    let asked = subscribe(&rust.port, TEN_S);
    let Frame::Request { id, .. } = rust.next().await else {
        panic!("a request");
    };
    assert!(matches!(
        rust.port.take(Frame::Reply {
            id,
            result: Err(refusal()),
        }),
        Ok(Taken::Handled)
    ));
    let Err(Unanswered::Refused(error)) = asked.await.unwrap() else {
        panic!("a refusal");
    };
    assert_eq!(code(&error), "actor.refused");
    assert_eq!(rust.following(), 0);
    rust.nothing_written().await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_subscription_ended_taken_before_the_asker_wakes_is_its_first_end()
 {
    let mut rust = Linked::ready(Role::Rust);
    let asked = subscribe(&rust.port, TEN_S);
    let Frame::Request { id, .. } = rust.next().await else {
        panic!("a request");
    };
    for frame in [
        reply(id, b""),
        Frame::SubscriptionEnded {
            sub: id,
            error: refusal(),
        },
    ] {
        assert!(matches!(rust.port.take(frame), Ok(Taken::Handled)));
    }
    let Ok(mut routed) = asked.await.unwrap() else {
        panic!("a subscription");
    };
    assert_eq!(ended(item(&mut routed).await), "actor.refused");
    drop(routed);
    rust.nothing_written().await;
}

#[tokio::test]
async fn a_subscription_frame_from_the_wrong_host_breaks_the_link() {
    let out_of_turn = |taken| match taken {
        Err(Broken::OutOfTurn(frame)) => frame,
        _ => panic!("a frame out of turn"),
    };
    let agent = Some(Caller::Actor(at("agent", "a")));

    let rust = Linked::new(Role::Rust);
    rust.welcome();
    assert_eq!(
        out_of_turn(
            rust.port.take(subscribe_request(1, agent.clone()))
        ),
        "request"
    );
    rust.port.ready();
    assert_eq!(
        out_of_turn(rust.port.take(subscribe_request(2, agent))),
        "request"
    );
    assert_eq!(
        out_of_turn(rust.port.take(Frame::Unsubscribe { sub: 1 })),
        "unsubscribe"
    );

    let node = Linked::ready(Role::Node);
    for (frame, name) in [
        (
            Frame::Event {
                sub: 1,
                args: Vec::new(),
            },
            "event",
        ),
        (Frame::Gap { sub: 1 }, "gap"),
        (
            Frame::SubscriptionEnded {
                sub: 1,
                error: refusal(),
            },
            "subscription_ended",
        ),
    ] {
        assert_eq!(out_of_turn(node.port.take(frame)), name);
    }
}
