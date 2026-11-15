//! A client's subscription to an actor of the stand-in Node host.

use std::path::Path;
use std::time::{Duration, Instant};

use at::at;
use call::call;
use calls::{
    DEADLINE, Exit, KINDS, OwnStandIn, Seen, WAIT, Who, agent_input,
    close, eventually, frames_read, made, open, ready, relay, starts,
};
use code::code;
use files::any_file_of;
use hamlet_actor::action::encode_positional;
use hamlet_actor::{Action, Event};
use hamlet_actor_error::ActorError;
use hamlet_host::child::LinkState;
use hamlet_host::link::{Ask, FRAME_LIMIT, Frame};
use hamlet_host::route::{CallerKind, Operation, Routes};
use hamlet_host::{
    Address, Caller, Host, HostEvent, HostOptions, Item, Subscription,
};
use hamlet_testing::Scratch;
use linked::{GONE_WAIT, has_line, wait_for};
use logs::Logs;
use ticks::{
    Close, Count, Crash, Destroy, Hang, SECRET, SLOW, Sleep, Tick,
    Ticked, Watchers, forbidden,
};
use tracing_subscriber::filter::LevelFilter;

#[path = "support/actions.rs"]
#[expect(dead_code, reason = "link_calls.rs uses the rest")]
mod actions;
#[path = "support/at.rs"]
mod at;
#[path = "support/call.rs"]
mod call;
#[path = "support/calls.rs"]
#[expect(dead_code, reason = "link_calls.rs uses the rest")]
mod calls;
#[path = "support/code.rs"]
mod code;
#[path = "support/files.rs"]
mod files;
#[path = "support/linked.rs"]
mod linked;
#[path = "support/logs.rs"]
mod logs;
#[path = "support/ticks.rs"]
mod ticks;

fn arg(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The count a [`Ticked`] carries.
fn value(args: &[u8]) -> u64 {
    let Ticked(n, _) = ciborium::from_reader(args).unwrap();
    n
}

/// The client's subscription to `event` of `to`, or its error, within
/// the test's wait.
async fn subscribe(
    host: &Host,
    to: &Address,
    event: &str,
) -> anyhow::Result<Subscription> {
    tokio::time::timeout(WAIT, host.subscribe_for_client(to, event))
        .await
        .expect("the subscribe was not answered")
}

async fn subscribed(host: &Host, to: &Address) -> Subscription {
    subscribe(host, to, Ticked::NAME)
        .await
        .unwrap_or_else(|error| {
            panic!("the subscribe to {to} failed: {error:#}")
        })
}

fn refusal(
    subscribed: anyhow::Result<Subscription>,
) -> anyhow::Error {
    let Err(error) = subscribed else {
        panic!("the subscribe succeeded");
    };
    error
}

/// The stream's next item, within the test's wait.
async fn next(stream: &mut Subscription) -> Item {
    tokio::time::timeout(WAIT, stream.next())
        .await
        .expect("the stream gave nothing")
        .expect("the stream ended")
}

/// The code of the stream's end, within the test's wait.
async fn end(stream: &mut Subscription) -> String {
    let next = tokio::time::timeout(WAIT, stream.next())
        .await
        .expect("the stream did not end");
    let Err(error) = next else {
        panic!("the stream gave {next:?}");
    };
    code(&error)
}

/// Whether `agent` holds no connection but the call's own within
/// `within`.
async fn unwatched(
    host: &Host,
    agent: &Address,
    within: Duration,
) -> bool {
    let polling = async {
        while !call(host, agent, &Watchers).await.unwrap().is_empty()
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(within, polling).await.is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_follows_a_node_actors_event_across_the_link() {
    let home = Scratch::new("link-follow");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let mut ticks = subscribed(&host, &agent).await;
    let mut answered = Vec::new();
    for _ in 0..5 {
        answered.push(call(&host, &agent, &Tick { pad: 0 }).await);
    }
    let mut seen = Vec::new();
    for _ in 0..5 {
        seen.push(next(&mut ticks).await);
    }
    let more = tokio::time::timeout(
        Duration::from_millis(200),
        ticks.next(),
    )
    .await;
    let watchers = call(&host, &agent, &Watchers).await;
    drop(ticks);
    close(host, node).await;

    let answered =
        answered.into_iter().map(Result::unwrap).collect::<Vec<_>>();
    assert_eq!(answered, [1, 2, 3, 4, 5]);
    let seen = seen
        .iter()
        .map(|item| match item {
            Item::Event(args) => value(args),
            Item::Gap => panic!("a gap while the link is up"),
        })
        .collect::<Vec<_>>();
    assert_eq!(seen, answered);
    assert!(more.is_err(), "{more:?}");
    assert_eq!(watchers.unwrap(), [Some(Caller::Client)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscription_gets_one_gap_when_the_node_host_dies_and_no_value_twice()
 {
    let home = Scratch::new("link-follow-kill");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let mut ticks = subscribed(&host, &agent).await;

    // A tick that meets the dead link fails, and is not repeated.
    let ticking = tokio::spawn({
        let (host, agent) = (host.clone(), agent.clone());
        async move {
            loop {
                let _ = call(&host, &agent, &Tick { pad: 0 }).await;
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }
    });

    let mut before = Vec::new();
    while before.len() < 20 {
        match next(&mut ticks).await {
            Item::Event(args) => before.push(value(&args)),
            Item::Gap => panic!("a gap while the link is up"),
        }
    }
    let _ = call(&host, &agent, &Exit).await;

    let mut gaps = 0;
    let mut read = None;
    let mut after = Vec::new();
    while after.len() < 20 {
        match next(&mut ticks).await {
            // What the old Node host wrote before it died.
            Item::Event(args) if gaps == 0 => {
                before.push(value(&args));
            }
            Item::Event(args) => after.push(value(&args)),
            Item::Gap => {
                gaps += 1;
                let count = host
                    .call_for_client(
                        &agent,
                        Count::NAME,
                        encode_positional(&Count).unwrap(),
                        DEADLINE,
                    )
                    .await
                    .expect("the read after the gap");
                read = Some(
                    ciborium::from_reader::<u64, _>(count.as_slice())
                        .unwrap(),
                );
            }
        }
    }
    ticking.abort();
    drop(ticks);
    close(host, node).await;

    assert_eq!(gaps, 1, "{before:?} {after:?}");
    assert!(
        after.iter().min() > before.iter().max(),
        "{before:?} {after:?}"
    );
    let all = before.iter().chain(&after).collect::<Vec<_>>();
    assert!(
        all.windows(2).all(|pair| pair[0] < pair[1]),
        "a value twice, or out of order: {all:?}"
    );
    let read = read.unwrap();
    assert!(after[0] <= read + 1, "{read} {after:?}");
    assert!(
        after.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "{after:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_while_the_link_is_down_fails_with_link_down_and_writes_nothing()
 {
    for (mode, state, read) in [
        ("--hold", LinkState::Down, &["goodbye"][..]),
        (
            "--welcomed",
            LinkState::Welcomed,
            &["welcome", "goodbye"][..],
        ),
    ] {
        let home = Scratch::new("link-follow-down");
        let (record, frames) =
            (home.join("starts"), home.join("frames"));
        let (host, node) = open(
            &home,
            HostOptions::default(),
            &[
                mode,
                "--starts",
                arg(&record),
                "--frames",
                arg(&frames),
            ],
        );
        eventually("the child's start", || {
            (starts(&record) == 1).then_some(())
        })
        .await;
        wait_for(&mut node.state(), state).await;
        let asked = Instant::now();
        let followed =
            subscribe(&host, &at("agent", "a"), Ticked::NAME).await;
        let took = asked.elapsed();
        close(host, node).await;

        assert_eq!(code(&refusal(followed)), "link.down", "{mode}");
        assert!(took < Duration::from_secs(1), "{mode}: {took:?}");
        assert_eq!(frames_read(&frames), read, "{mode}");
    }
}

/// The subscribe registers before the exit's call, which `agent/b`
/// takes while `agent/a` lets its subscriber in.
#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_in_flight_fails_with_link_lost_when_the_node_host_dies()
 {
    let home = Scratch::new("link-follow-lost");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let (a, b) = (at("agent", "a"), at("agent", "b"));
    host.create(&a, agent_input(None)).await.unwrap();
    host.create(&b, agent_input(None)).await.unwrap();
    let mut state = node.state();
    let asks = async {
        tokio::join!(
            biased;
            host.subscribe_for_client(&a, SLOW),
            call(&host, &b, &Exit),
        )
    };
    let (followed, exit) = tokio::time::timeout(GONE_WAIT, asks)
        .await
        .expect("the subscribe in flight outlived the Node host");
    wait_for(&mut state, LinkState::Ready).await;
    close(host, node).await;

    assert_eq!(code(&refusal(followed)), "link.lost");
    assert_eq!(code(&exit.unwrap_err()), "link.lost");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_to_a_key_with_no_file_fails_with_not_found_and_the_node_host_makes_none()
 {
    let home = Scratch::new("link-follow-not-found");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let followed =
        subscribe(&host, &at("agent", "missing"), Ticked::NAME).await;
    close(host, node).await;

    assert_eq!(code(&refusal(followed)), "actor.not_found");
    assert!(!any_file_of(&home, "agent", "missing"));
}

fn clients_kept_from_agents() -> Routes {
    Routes::only(&[(CallerKind::Client, &["realm"])])
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_the_route_table_refuses_writes_no_frame_and_the_node_host_refuses_it_again()
 {
    let home = Scratch::new("link-follow-refused");
    let frames = home.join("frames");
    let (host, node) = ready(
        &home,
        clients_kept_from_agents(),
        &["--silent", "--frames", arg(&frames)],
    )
    .await;
    let agent = at("agent", "a");
    let followed = subscribe(&host, &agent, Ticked::NAME).await;
    close(host, node).await;
    assert_eq!(code(&refusal(followed)), "actor.refused");
    assert_eq!(frames_read(&frames), ["welcome", "goodbye"]);

    let home = Scratch::new("link-follow-refused-node");
    let mut node = OwnStandIn::start(&home, &[]);
    node.welcome(&KINDS, clients_kept_from_agents()).await;
    node.write(&Frame::Request {
        id: 1,
        from: Some(Caller::Client),
        to: agent.clone(),
        ask: Ask::Subscribe {
            event: Ticked::NAME.to_owned(),
        },
    });
    let frames = [node.read().await, node.read().await];
    let refused = frames.iter().find_map(|frame| match frame {
        Frame::Reply {
            id: 1,
            result: Err(error),
        } => Some((error.group.as_str(), error.code.as_str())),
        _ => None,
    });
    assert_eq!(refused, Some(("actor", "refused")), "{frames:?}");
    let event = Frame::HostEvent(HostEvent::Refused {
        from: Caller::Client,
        to: agent,
        operation: Operation::Subscribe {
            event: Ticked::NAME.to_owned(),
        },
    });
    assert!(frames.contains(&event), "{frames:?}");
    node.write(&Frame::Goodbye);
    let (status, errors) = node.ended(WAIT).await;
    assert!(status.success(), "{status}: {errors}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_the_node_actor_refuses_fails_with_its_error() {
    let home = Scratch::new("link-follow-forbidden");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let followed = subscribe(&host, &agent, SECRET).await;
    let watchers = call(&host, &agent, &Watchers).await;
    close(host, node).await;

    let error = ActorError::extract(&refusal(followed));
    let actions::Fail::Coded {
        group,
        code,
        message,
        ..
    } = forbidden()
    else {
        unreachable!();
    };
    assert_eq!(
        (error.group(), error.code(), error.message()),
        (group.as_str(), code.as_str(), message.as_str())
    );
    assert_eq!(watchers.unwrap(), []);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_with_the_node_actors_end_and_is_not_asked_again()
 {
    let home = Scratch::new("link-follow-end");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let mut ended = Vec::new();
    for key in ["sleep", "destroy", "crash", "close"] {
        let agent = at("agent", key);
        host.create(&agent, agent_input(None)).await.unwrap();
        let mut ticks = subscribed(&host, &agent).await;
        let _ = match key {
            "sleep" => call(&host, &agent, &Sleep).await,
            "destroy" => call(&host, &agent, &Destroy).await,
            "crash" => call(&host, &agent, &Crash).await,
            _ => call(&host, &agent, &Close).await,
        };
        let first = end(&mut ticks).await;
        let again = end(&mut ticks).await;
        ended.push((key, first, again));
    }
    close(host, node).await;
    let ended = ended
        .iter()
        .map(|(key, first, again)| {
            (*key, first.as_str(), again.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ended,
        [
            ("sleep", "actor.stopping", "actor.stopping"),
            ("destroy", "actor.destroyed", "actor.destroyed"),
            ("crash", "actor.stopping", "actor.stopping"),
            ("close", "actor.disconnected", "actor.disconnected"),
        ]
    );

    let home = Scratch::new("link-follow-ended");
    let frames = home.join("frames");
    let (host, node) = ready(
        &home,
        Routes::Any,
        &["--end-subscription", "--frames", arg(&frames)],
    )
    .await;
    let mut ticks = subscribed(&host, &at("agent", "a")).await;
    let first = end(&mut ticks).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(ticks);
    close(host, node).await;
    assert_eq!(first, "actor.destroyed");
    assert_eq!(
        frames_read(&frames),
        ["welcome", "request", "goodbye"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_subscription_unsubscribes_and_the_node_actor_may_sleep()
 {
    let home = Scratch::new("link-follow-drop");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let ticks = subscribed(&host, &agent).await;
    let watched = call(&host, &agent, &Watchers).await;
    drop(ticks);
    let unwatched =
        unwatched(&host, &agent, Duration::from_secs(2)).await;
    close(host, node).await;

    assert_eq!(watched.unwrap(), [Some(Caller::Client)]);
    assert!(
        unwatched,
        "the dropped subscription kept its connection"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscription_dropped_while_its_subscribe_is_in_flight_leaves_no_connection()
 {
    let home = Scratch::new("link-follow-drop-early");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let dropped = tokio::time::timeout(
        Duration::from_millis(50),
        host.subscribe_for_client(&agent, SLOW),
    )
    .await
    .is_err();
    let unwatched =
        unwatched(&host, &agent, Duration::from_secs(2)).await;
    close(host, node).await;

    assert!(dropped, "the subscribe was answered within 50 ms");
    assert!(unwatched, "the dropped subscribe left its connection");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_event_over_the_frame_limit_becomes_a_gap_and_the_link_stays_up()
 {
    let home = Scratch::new("link-follow-limit");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let (realm, agent) = (at("realm", "r"), at("agent", "a"));
    host.create(&realm, Vec::new()).await.unwrap();
    host.create(&agent, agent_input(None)).await.unwrap();
    let mut ticks = subscribed(&host, &agent).await;
    let mut state = node.state();
    state.borrow_and_update();
    let over = call(
        &host,
        &agent,
        &Tick {
            pad: FRAME_LIMIT + (1 << 20),
        },
    )
    .await;
    let gap = next(&mut ticks).await;
    let after = call(&host, &agent, &Tick { pad: 0 }).await;
    let then = next(&mut ticks).await;
    let answered = call(&host, &agent, &relay(&realm, &Who)).await;
    let stayed = !state.has_changed().unwrap();
    drop(ticks);
    close(host, node).await;

    assert_eq!(over.unwrap(), 1);
    assert_eq!(gap, Item::Gap);
    assert_eq!(after.unwrap(), 2);
    assert!(
        matches!(&then, Item::Event(args) if value(args) == 2),
        "{then:?}"
    );
    assert_eq!(
        answered.unwrap(),
        Seen::output(Some(Caller::Actor(agent)))
    );
    assert!(stayed, "the link went down");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reader_that_stalls_gets_a_gap_and_the_host_keeps_answering()
 {
    let home = Scratch::new("link-follow-stall");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let mut ticks = subscribed(&host, &agent).await;
    // 4 MiB of ticks, none read.
    let mut slowest = Duration::ZERO;
    for _ in 0..64 {
        let asked = Instant::now();
        call(&host, &agent, &Tick { pad: 64 << 10 }).await.unwrap();
        slowest = slowest.max(asked.elapsed());
    }
    let asked = Instant::now();
    let who = call(&host, &agent, &Who).await;
    let answered_in = asked.elapsed();
    let later = call(&host, &agent, &Tick { pad: 0 }).await.unwrap();
    let mut values = Vec::new();
    let mut gaps = 0;
    while values.last() != Some(&later) {
        match next(&mut ticks).await {
            Item::Event(args) => values.push(value(&args)),
            Item::Gap => gaps += 1,
        }
    }
    drop(ticks);
    close(host, node).await;

    assert!(slowest < Duration::from_secs(1), "{slowest:?}");
    assert_eq!(who.unwrap(), None);
    assert!(answered_in < Duration::from_secs(1), "{answered_in:?}");
    assert!(gaps >= 1, "{values:?}");
    assert!(
        values.windows(2).all(|pair| pair[0] < pair[1]),
        "{values:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_at_the_rust_hosts_shutdown_after_its_supervisors()
 {
    let home = Scratch::new("link-follow-shutdown");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let mut ticks = subscribed(&host, &agent).await;
    let status = node.shutdown().await.expect("a child ran");
    let quiet = tokio::time::timeout(
        Duration::from_millis(300),
        ticks.next(),
    )
    .await;
    host.shutdown().await.unwrap();
    let ended =
        tokio::time::timeout(Duration::from_secs(1), ticks.next())
            .await
            .expect("the stream outlived the host's shutdown");

    assert!(status.success(), "{status}");
    assert!(quiet.is_err(), "{quiet:?}");
    let Err(error) = ended else {
        panic!("the stream gave {ended:?}");
    };
    assert_eq!(code(&error), "actor.stopping");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_the_node_host_never_answers_fails_with_not_ready_and_unsubscribes()
 {
    let home = Scratch::new("link-follow-silent");
    let frames = home.join("frames");
    let options = HostOptions {
        start_deadline: Duration::from_secs(1),
        ..HostOptions::default()
    };
    let (host, node) =
        open(&home, options, &["--silent", "--frames", arg(&frames)]);
    let mut state = node.state();
    wait_for(&mut state, LinkState::Ready).await;
    let asked = Instant::now();
    let followed =
        subscribe(&host, &at("agent", "a"), Ticked::NAME).await;
    let took = asked.elapsed();
    eventually("the unsubscribe", || {
        (frames_read(&frames).len() == 3).then_some(())
    })
    .await;
    let stayed = !state.has_changed().unwrap();
    close(host, node).await;

    assert_eq!(code(&refusal(followed)), "actor.not_ready");
    assert!(took < Duration::from_secs(2), "{took:?}");
    assert_eq!(
        frames_read(&frames),
        ["welcome", "request", "unsubscribe", "goodbye"]
    );
    assert!(stayed, "the link went down");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_the_node_host_writes_breaks_the_link() {
    let home = Scratch::new("link-follow-asked");
    let realm = at("realm", "r");
    made(&home, &[&realm]).await;
    let record = home.join("starts");
    let logs = Logs::default();
    let _recording = logs.record(LevelFilter::WARN);
    let (host, node) = open(
        &home,
        HostOptions::default(),
        &["--ask-subscribe", "--starts", arg(&record)],
    );
    eventually("the next child's start", || {
        (starts(&record) == 2).then_some(())
    })
    .await;
    let started = host.is_running(&realm);
    node.shutdown().await;
    host.shutdown().await.unwrap();

    assert!(
        has_line(&logs, &["broke the link", "request out of turn"]),
        "{}",
        logs.text()
    );
    assert!(!started);
}

/// The agent's stop hangs, so the stand-in is ended by another
/// agent's `exit` rather than kept to its stop deadline at the close.
#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_when_a_node_actors_stop_misses_its_deadline()
{
    let home = Scratch::new("link-follow-hang");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let (agent, other) = (at("agent", "a"), at("agent", "b"));
    host.create(&agent, agent_input(None)).await.unwrap();
    host.create(&other, agent_input(None)).await.unwrap();
    let mut ticks = subscribed(&host, &agent).await;
    call(&host, &agent, &Hang).await.unwrap();
    let slept = Instant::now();
    let sleep = call(&host, &agent, &Sleep).await;
    let ended = end(&mut ticks).await;
    let took = slept.elapsed();
    let mut state = node.state();
    let exit = call(&host, &other, &Exit).await;
    wait_for(&mut state, LinkState::Ready).await;
    drop(ticks);
    close(host, node).await;

    sleep.unwrap();
    assert_eq!(ended, "actor.stopping");
    assert!(took < Duration::from_millis(500), "{took:?}");
    assert_eq!(code(&exit.unwrap_err()), "link.lost");
}
