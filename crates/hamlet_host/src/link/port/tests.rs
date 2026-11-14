use std::io::{self, PipeReader};
use std::thread;

use hamlet_testing::Scratch;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::*;
use crate::link::{AtBreak, FRAME_LIMIT, read};
use crate::route::Operation;
use crate::testing::code;
use crate::{Host, HostOptions};

const TEN_S: Duration = Duration::from_secs(10);
const SHORT: Duration = Duration::from_millis(50);

fn at(kind: &str, key: &str) -> Address {
    Address::new(kind, key).unwrap()
}

fn reply(id: u64, output: &[u8]) -> Frame {
    Frame::Reply {
        id,
        result: Ok(ByteBuf::from(output)),
    }
}

fn frames(mut input: PipeReader) -> mpsc::UnboundedReceiver<Frame> {
    let (sender, frames) = mpsc::unbounded_channel();
    thread::spawn(move || {
        while let Ok(Some(frame)) = read(&mut input) {
            let _ = sender.send(frame);
        }
    });
    frames
}

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

    async fn nothing_written(&mut self) {
        self.out.write(&Frame::Goodbye).await.unwrap();
        assert_eq!(self.next().await, Frame::Goodbye);
    }
}

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
            .call(caller.as_ref(), &realm, "who", Vec::new(), TEN_S)
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
            port.call(Some(&agent), &realm, "who", Vec::new(), TEN_S)
                .await
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
