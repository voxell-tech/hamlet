#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::time::{Duration, Instant};

use at::at;
use call::call;
use calls::{
    DEADLINE, Exit, Fail, Input, KINDS, Large, Make, OwnStandIn,
    Seen, Slow, Started, Thread, WAIT, Who, agent_input, close,
    eventually, factories, frames_read, made, open, ready, relay,
    starts,
};
use code::code;
use files::any_file_of;
use hamlet_actor::Action;
use hamlet_actor::action::encode_positional;
use hamlet_actor_core::ShutdownKind;
use hamlet_actor_error::ActorError;
use hamlet_host::child::{self, LinkState};
use hamlet_host::link::{Ask, FRAME_LIMIT, Frame, WireError};
use hamlet_host::route::{CallerKind, Operation, Routes};
use hamlet_host::{Address, Caller, HostEvent, HostOptions};
use hamlet_testing::Scratch;
use linked::{GONE_WAIT, has_line, wait_for};
use logs::Logs;
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing_subscriber::filter::LevelFilter;

#[path = "support/actions.rs"]
mod actions;
#[path = "support/at.rs"]
mod at;
#[path = "support/call.rs"]
mod call;
#[path = "support/calls.rs"]
mod calls;
#[path = "support/code.rs"]
mod code;
#[path = "support/files.rs"]
mod files;
#[path = "support/linked.rs"]
mod linked;
#[path = "support/logs.rs"]
mod logs;

fn args<M: Action>(action: &M) -> Vec<u8> {
    encode_positional(action).unwrap()
}

fn decoded<T: DeserializeOwned>(output: &[u8]) -> T {
    ciborium::from_reader(output).unwrap()
}

fn arg(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn request<M: Action>(
    id: u64,
    from: Option<Caller>,
    to: &Address,
    action: &M,
) -> Frame {
    Frame::Request {
        id,
        from,
        to: to.clone(),
        ask: Ask::Call {
            name: M::NAME.to_owned(),
            args: args(action),
            deadline_ms: u64::try_from(DEADLINE.as_millis()).unwrap(),
        },
    }
}

fn create(id: u64, from: Option<Caller>, at: &Address) -> Frame {
    Frame::Request {
        id,
        from,
        to: at.clone(),
        ask: Ask::Create {
            input: agent_input(None),
        },
    }
}

fn coded() -> Fail {
    Fail::Coded {
        group: "message".to_owned(),
        code: "wrong_caller".to_owned(),
        message: "client may not send 'end'.".to_owned(),
        metadata: json!({ "action": "end", "caller": "client" }),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rust_actors_call_reaches_a_node_actor_stamped_with_its_address()
 {
    let home = Scratch::new("link-out");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let (realm, agent) = (at("realm", "r"), at("agent", "a"));
    host.create(&realm, Vec::new()).await.unwrap();
    host.create(&agent, agent_input(None)).await.unwrap();

    let by_realm = call(&host, &realm, &relay(&agent, &Who)).await;
    let by_program = call(&host, &agent, &Who).await;
    let by_client = host
        .call_for_client(&agent, Who::NAME, args(&Who), DEADLINE)
        .await;
    let unbounded = host
        .call(&agent, Who::NAME, args(&Who), Duration::MAX)
        .await;
    close(host, node).await;

    assert_eq!(
        by_realm.unwrap(),
        Seen::output(Some(Caller::Actor(realm)))
    );
    assert_eq!(by_program.unwrap(), None);
    assert_eq!(
        decoded::<Option<Caller>>(&by_client.unwrap()),
        Some(Caller::Client)
    );
    assert_eq!(decoded::<Option<Caller>>(&unbounded.unwrap()), None);
}

#[tokio::test(flavor = "current_thread")]
async fn a_node_actors_call_reaches_a_rust_actor_stamped_with_its_address()
 {
    let home = Scratch::new("link-in");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let (realm, agent) = (at("realm", "r"), at("agent", "a"));
    host.create(&realm, Vec::new()).await.unwrap();
    host.create(&agent, agent_input(None)).await.unwrap();
    let seen = call(&host, &agent, &relay(&realm, &Who)).await;
    close(host, node).await;
    assert_eq!(
        seen.unwrap(),
        Seen::output(Some(Caller::Actor(agent)))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_a_node_host_that_starts_and_is_never_ready_fails_at_its_deadline()
 {
    let wait = Duration::from_millis(500);
    for (mode, state, read) in [
        ("--hold", LinkState::Down, &["goodbye"][..]),
        (
            "--welcomed",
            LinkState::Welcomed,
            &["welcome", "goodbye"][..],
        ),
    ] {
        let home = Scratch::new("link-not-ready");
        let (record, frames) =
            (home.join("starts"), home.join("frames"));
        let (host, node) = open(
            &home,
            HostOptions {
                start_deadline: wait,
                ..HostOptions::default()
            },
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
        let agent = at("agent", "a");
        let asked = Instant::now();
        let called =
            host.call(&agent, Who::NAME, args(&Who), wait).await;
        let created = host.create(&agent, agent_input(None)).await;
        let took = asked.elapsed();
        close(host, node).await;

        assert_eq!(code(&called.unwrap_err()), "link.down", "{mode}");
        assert_eq!(
            code(&created.unwrap_err()),
            "actor.not_ready",
            "{mode}"
        );
        assert!(took >= wait * 2, "{mode}: {took:?}");
        assert!(took < Duration::from_secs(3), "{mode}: {took:?}");
        assert_eq!(frames_read(&frames), read, "{mode}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_rust_host_answers_a_node_hosts_call_before_it_is_ready()
{
    let home = Scratch::new("link-early");
    made(&home, &[&at("realm", "r")]).await;
    let answer = home.join("answer");
    let (host, node) = open(
        &home,
        HostOptions::default(),
        &["--ask-early", arg(&answer)],
    );
    let reply = eventually("the answer", || {
        let text = std::fs::read_to_string(&answer).ok()?;
        serde_json::from_str::<Result<Option<Caller>, WireError>>(
            &text,
        )
        .ok()
    })
    .await;
    let state = *node.state().borrow();
    close(host, node).await;
    assert_eq!(reply, Ok(Some(Caller::Actor(at("agent", "a")))));
    assert_eq!(state, LinkState::Welcomed);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_actor_whose_start_calls_a_rust_actor_is_answered_on_a_fresh_node_host()
 {
    let home = Scratch::new("link-fresh");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let (realm, agent) = (at("realm", "r"), at("agent", "a"));
    host.create(&realm, Vec::new()).await.unwrap();
    host.create(&agent, agent_input(Some(&realm)))
        .await
        .unwrap();
    let first = call(&host, &agent, &Started).await.unwrap();
    let mut state = node.state();
    let exit =
        host.call(&agent, Exit::NAME, args(&Exit), DEADLINE).await;
    assert_eq!(code(&exit.unwrap_err()), "link.lost");
    wait_for(&mut state, LinkState::Ready).await;
    let second = call(&host, &agent, &Started).await;
    close(host, node).await;

    assert_eq!(first, Some(Seen::output(1)));
    assert_eq!(second.unwrap(), Some(Seen::output(2)));
}

fn agents_kept_from_agents() -> Routes {
    Routes::only(&[
        (CallerKind::Actor("realm".to_owned()), &["workspace"]),
        (
            CallerKind::Actor("agent".to_owned()),
            &["realm", "workspace"],
        ),
        (CallerKind::Client, &["realm", "agent"]),
    ])
}

#[tokio::test(flavor = "multi_thread")]
async fn a_create_the_route_table_refuses_fails_with_actor_refused() {
    let home = Scratch::new("link-refused-create");
    let (host, node) =
        ready(&home, agents_kept_from_agents(), &[]).await;
    let (realm, agent) = (at("realm", "r"), at("agent", "b"));
    host.create(&realm, Vec::new()).await.unwrap();
    let made = call(
        &host,
        &realm,
        &Make {
            at: agent.clone(),
            input: None,
        },
    )
    .await;
    close(host, node).await;
    assert_eq!(
        made.unwrap().code().as_deref(),
        Some("actor.refused")
    );
    assert!(!any_file_of(&home, "agent", "b"));

    let home = Scratch::new("link-refused-create-node");
    let mut node = OwnStandIn::start(&home, &[]);
    node.welcome(&KINDS, agents_kept_from_agents()).await;
    node.write(&create(
        1,
        Some(Caller::Actor(realm.clone())),
        &agent,
    ));
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
        from: Caller::Actor(realm),
        to: agent,
        operation: Operation::Create,
    });
    assert!(frames.contains(&event), "{frames:?}");
    assert!(!any_file_of(&home, "agent", "b"));
}

/// The call registers before the exit's, so it is out when the Node
/// host dies.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_in_flight_fails_with_link_lost_when_the_node_host_dies()
 {
    let home = Scratch::new("link-lost");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let mut state = node.state();
    let calls = async {
        tokio::join!(
            biased;
            host.call(&agent, Slow::NAME, args(&Slow { ms: 5_000 }), DEADLINE),
            host.call(&agent, Exit::NAME, args(&Exit), DEADLINE),
        )
    };
    let (slow, exit) = tokio::time::timeout(GONE_WAIT, calls)
        .await
        .expect("the call in flight outlived the Node host");
    assert_eq!(code(&slow.unwrap_err()), "link.lost");
    assert_eq!(code(&exit.unwrap_err()), "link.lost");

    let next =
        host.call(&agent, Who::NAME, args(&Who), DEADLINE).await;
    assert_eq!(code(&next.unwrap_err()), "link.down");
    wait_for(&mut state, LinkState::Ready).await;
    let after = call(&host, &agent, &Who).await;
    close(host, node).await;
    assert_eq!(after.unwrap(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agents_call_to_an_agent_is_refused_and_the_rust_host_hears_it()
 {
    let home = Scratch::new("link-refused-call");
    let (host, node) =
        ready(&home, agents_kept_from_agents(), &[]).await;
    let (a, b) = (at("agent", "a"), at("agent", "b"));
    host.create(&a, agent_input(None)).await.unwrap();
    let mut events = host.events();
    let seen = call(&host, &a, &relay(&b, &Who)).await;
    let event = tokio::time::timeout(WAIT, events.recv()).await;
    close(host, node).await;

    assert_eq!(
        seen.unwrap().code().as_deref(),
        Some("actor.refused")
    );
    assert_eq!(
        event.expect("the Rust host heard nothing").unwrap(),
        HostEvent::Refused {
            from: Caller::Actor(a),
            to: b,
            operation: Operation::Call {
                action: Who::NAME.to_owned(),
            },
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_clients_call_crosses_as_client_and_is_refused_where_the_table_says()
 {
    let home = Scratch::new("link-client");
    let frames = home.join("frames");
    let table = Routes::only(&[
        (CallerKind::Client, &["realm"]),
        (
            CallerKind::Actor("realm".to_owned()),
            &["workspace", "agent"],
        ),
    ]);
    let (host, node) =
        ready(&home, table, &["--silent", "--frames", arg(&frames)])
            .await;
    let workspace = at("workspace", "w");
    host.create(&workspace, Vec::new()).await.unwrap();
    let mut refused = Vec::new();
    for to in [&workspace, &at("agent", "a")] {
        let called = host
            .call_for_client(to, Who::NAME, args(&Who), DEADLINE)
            .await;
        refused.push(code(&called.unwrap_err()));
    }
    close(host, node).await;
    assert_eq!(refused, ["actor.refused", "actor.refused"]);
    assert_eq!(frames_read(&frames), ["welcome", "goodbye"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_a_key_with_no_file_fails_with_actor_not_found_each_way()
 {
    let home = Scratch::new("link-not-found");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let out = host
        .call(
            &at("agent", "missing"),
            Who::NAME,
            args(&Who),
            DEADLINE,
        )
        .await;
    let back =
        call(&host, &agent, &relay(&at("realm", "missing"), &Who))
            .await;
    close(host, node).await;

    assert_eq!(code(&out.unwrap_err()), "actor.not_found");
    assert_eq!(
        back.unwrap().code().as_deref(),
        Some("actor.not_found")
    );
    assert!(!any_file_of(&home, "agent", "missing"));
    assert!(!any_file_of(&home, "realm", "missing"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_create_across_the_link_makes_the_actor_once() {
    let home = Scratch::new("link-create");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let (realm, agent) = (at("realm", "r"), at("agent", "7"));
    host.create(&realm, Vec::new()).await.unwrap();
    host.create(&agent, agent_input(Some(&realm)))
        .await
        .unwrap();
    let made =
        home.join("actors").join("agent").join("7.sqlite").exists();
    let input = call(&host, &agent, &Input).await;
    let again = host.create(&agent, agent_input(None)).await;
    close(host, node).await;

    assert!(made);
    assert_eq!(input.unwrap(), Some(realm));
    assert_eq!(code(&again.unwrap_err()), "actor.duplicate_key");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_create_the_node_host_never_answers_fails_with_not_ready_at_the_start_deadline()
 {
    let home = Scratch::new("link-silent");
    let options = HostOptions {
        start_deadline: Duration::from_secs(1),
        ..HostOptions::default()
    };
    let (host, node) = open(&home, options, &["--silent"]);
    let mut state = node.state();
    wait_for(&mut state, LinkState::Ready).await;
    let asked = Instant::now();
    let created =
        host.create(&at("agent", "a"), agent_input(None)).await;
    let took = asked.elapsed();
    let stayed = !state.has_changed().unwrap();
    close(host, node).await;

    assert_eq!(code(&created.unwrap_err()), "actor.not_ready");
    assert!(took < Duration::from_secs(2), "{took:?}");
    assert!(stayed, "the link went down");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_coded_error_crosses_the_link_each_way_with_its_group_code_message_and_metadata()
 {
    let home = Scratch::new("link-coded");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let (realm, agent) = (at("realm", "r"), at("agent", "a"));
    host.create(&realm, Vec::new()).await.unwrap();
    host.create(&agent, agent_input(None)).await.unwrap();
    let out = host
        .call(&agent, Fail::NAME, args(&coded()), DEADLINE)
        .await;
    let back = call(&host, &agent, &relay(&realm, &coded())).await;
    close(host, node).await;

    let out = ActorError::extract(&out.unwrap_err());
    let message = "client may not send 'end'.";
    let metadata = json!({ "action": "end", "caller": "client" });
    assert_eq!(
        (out.group(), out.code(), out.message()),
        ("message", "wrong_caller", message)
    );
    assert_eq!(out.metadata(), Some(metadata.clone()));
    assert_eq!(
        back.unwrap(),
        Seen::Error {
            group: "message".to_owned(),
            code: "wrong_caller".to_owned(),
            message: message.to_owned(),
            metadata: Some(metadata),
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_uncoded_handler_error_crosses_as_an_internal_error_with_its_text()
 {
    let home = Scratch::new("link-uncoded");
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let (realm, agent) = (at("realm", "r"), at("agent", "a"));
    host.create(&realm, Vec::new()).await.unwrap();
    host.create(&agent, agent_input(None)).await.unwrap();
    let text = "the disk is full";
    let uncoded = Fail::Uncoded {
        text: text.to_owned(),
    };
    let out = host
        .call(&agent, Fail::NAME, args(&uncoded), DEADLINE)
        .await;
    let back = call(&host, &agent, &relay(&realm, &uncoded)).await;
    close(host, node).await;

    let out = ActorError::extract(&out.unwrap_err());
    let out = Seen::Error {
        group: out.group().to_owned(),
        code: out.code().to_owned(),
        message: out.message().to_owned(),
        metadata: out.metadata(),
    };
    for seen in [out, back.unwrap()] {
        let Seen::Error {
            group,
            code,
            message,
            metadata,
        } = &seen
        else {
            panic!("{seen:?}");
        };
        assert_eq!(
            (group.as_str(), code.as_str()),
            ("core", "internal_error")
        );
        assert!(message.contains(text), "{seen:?}");
        let error =
            metadata.as_ref().and_then(|meta| meta["error"].as_str());
        assert!(
            error.is_some_and(|error| error.contains(text)),
            "{seen:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stamp_the_other_host_does_not_speak_for_breaks_the_link() {
    let realm = at("realm", "r");
    for (stamp, logged) in [
        ("none", "with no caller"),
        ("client", "as client"),
        ("realm/x", "as realm/x"),
    ] {
        let home = Scratch::new("link-forge");
        made(&home, &[&realm]).await;
        let record = home.join("starts");
        let logs = Logs::default();
        let _recording = logs.record(LevelFilter::WARN);
        let (host, node) = open(
            &home,
            HostOptions::default(),
            &["--forge", stamp, "--starts", arg(&record)],
        );
        eventually("the next child's start", || {
            (starts(&record) == 2).then_some(())
        })
        .await;
        let called = host.is_running(&realm);
        node.shutdown().await;
        host.shutdown().await.unwrap();

        assert!(
            has_line(&logs, &["broke the link", logged]),
            "{stamp}: {}",
            logs.text()
        );
        assert!(!called, "{stamp}");
    }

    let home = Scratch::new("link-forge-node");
    let mut node = OwnStandIn::start(&home, &[]);
    node.welcome(&KINDS, Routes::Any).await;
    let forged = Some(Caller::Actor(at("agent", "x")));
    node.write(&request(1, forged, &at("agent", "a"), &Who));
    let (status, errors) = node.ended(WAIT).await;
    assert!(!status.success(), "{status}");
    assert!(errors.contains("as agent/x"), "{errors}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kind_neither_host_runs_fails_in_the_calling_host() {
    let home = Scratch::new("link-no-kind");
    let mut node = OwnStandIn::start(&home, &[]);
    node.welcome(&KINDS, Routes::Any).await;
    let agent = at("agent", "a");
    node.write(&create(1, None, &agent));
    let (_, made) = node.until_reply(1).await;
    assert!(made.is_ok(), "{made:?}");
    let relayed = relay(&at("realmm", "r"), &Who);
    node.write(&request(2, None, &agent, &relayed));
    let (before, reply) = node.until_reply(2).await;

    assert!(
        !before
            .iter()
            .any(|frame| matches!(frame, Frame::Request { .. })),
        "{before:?}"
    );
    let seen = decoded::<Seen>(&reply.unwrap());
    assert_eq!(seen.code().as_deref(), Some("actor.not_registered"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peers_request_for_a_kind_of_its_own_is_not_sent_back() {
    let home = Scratch::new("link-own-kind");
    let mut node = OwnStandIn::start(&home, &[]);
    node.welcome(&KINDS, Routes::Any).await;
    node.write(&request(1, None, &at("realm", "x"), &Who));
    let (before, reply) = node.until_reply(1).await;

    assert!(
        !before
            .iter()
            .any(|frame| matches!(frame, Frame::Request { .. })),
        "{before:?}"
    );
    let error = reply.unwrap_err();
    assert_eq!(
        (error.group.as_str(), error.code.as_str()),
        ("actor", "not_registered")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_call_and_a_create_cross_the_link() {
    let home = Scratch::new("link-only");
    let frames = home.join("frames");
    let (host, node) = ready(
        &home,
        Routes::Any,
        &["--silent", "--frames", arg(&frames)],
    )
    .await;
    let agent = at("agent", "a");
    let refused = [
        host.get_or_start(&agent).await,
        host.stop(&agent, ShutdownKind::Sleep).await,
        host.set_aside_timers(&agent).await.map(drop),
        host.rearm_timer(&agent, "e1", 0).await.map(drop),
    ];
    close(host, node).await;
    for outcome in refused {
        assert_eq!(
            code(&outcome.unwrap_err()),
            "actor.invalid_operation"
        );
    }
    assert_eq!(frames_read(&frames), ["welcome", "goodbye"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_past_its_deadline_times_out_and_its_late_reply_is_dropped()
 {
    let home = Scratch::new("link-late");
    let record = home.join("starts");
    let (host, node) =
        ready(&home, Routes::Any, &["--starts", arg(&record)]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let mut state = node.state();
    state.borrow_and_update();
    let late = host
        .call(
            &agent,
            Slow::NAME,
            args(&Slow { ms: 500 }),
            Duration::from_millis(100),
        )
        .await;
    let after = call(&host, &agent, &Slow { ms: 300 }).await;
    let next = call(&host, &agent, &Who).await;
    let stayed = !state.has_changed().unwrap();
    close(host, node).await;

    assert_eq!(code(&late.unwrap_err()), "actor.action_timed_out");
    after.unwrap();
    assert_eq!(next.unwrap(), None);
    assert!(stayed, "the link went down");
    assert_eq!(starts(&record), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_frame_over_the_limit_fails_at_once_and_the_link_stays_up()
{
    let home = Scratch::new("link-limit");
    let record = home.join("starts");
    let (host, node) =
        ready(&home, Routes::Any, &["--starts", arg(&record)]).await;
    let (realm, agent) = (at("realm", "r"), at("agent", "a"));
    host.create(&realm, Vec::new()).await.unwrap();
    host.create(&agent, agent_input(None)).await.unwrap();
    let mut state = node.state();
    state.borrow_and_update();
    let over = host
        .call(&agent, Who::NAME, vec![0; FRAME_LIMIT], DEADLINE)
        .await
        .unwrap_err();
    let answered = call(
        &host,
        &agent,
        &relay(&realm, &Large { bytes: FRAME_LIMIT }),
    )
    .await;
    let next = call(&host, &agent, &Who).await;
    let stayed = !state.has_changed().unwrap();
    close(host, node).await;

    assert!(
        over.chain().all(|cause| cause
            .downcast_ref::<ActorError>()
            .is_none()),
        "{over:?}"
    );
    let over = format!("{over:#}");
    let length = over
        .split("a frame of ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|length| length.parse::<usize>().ok());
    assert!(
        length.is_some_and(|length| length > FRAME_LIMIT),
        "{over}"
    );
    assert!(
        over.contains(&format!("over the limit of {FRAME_LIMIT}")),
        "{over}"
    );
    let answered = answered.unwrap();
    assert_eq!(
        answered.code().as_deref(),
        Some("core.internal_error")
    );
    let Seen::Error { message, .. } = &answered else {
        unreachable!();
    };
    assert!(
        message.contains(&format!("over the limit of {FRAME_LIMIT}")),
        "{message}"
    );
    assert_eq!(next.unwrap(), None);
    assert!(stayed, "the link went down");
    assert_eq!(starts(&record), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_hosts_call_runs_off_the_link_thread() {
    let home = Scratch::new("link-thread");
    let realm = at("realm", "r");
    made(&home, &[&realm]).await;
    let (host, node) = ready(&home, Routes::Any, &[]).await;
    let agent = at("agent", "a");
    host.create(&agent, agent_input(None)).await.unwrap();
    let worker = tokio::spawn(async {
        std::thread::current().name().map(str::to_owned)
    })
    .await
    .unwrap();
    let running = host.is_running(&realm);
    let seen = call(&host, &agent, &relay(&realm, &Thread)).await;
    close(host, node).await;

    assert!(!running);
    assert_ne!(worker.as_deref(), Some("node-link"));
    assert_eq!(seen.unwrap(), Seen::output(worker));
}

/// The reply is larger than a pipe holds, so a child of another test,
/// which holds a copy of the output's end from its fork to its exec,
/// cannot take it whole.
#[tokio::test(flavor = "multi_thread")]
async fn a_node_host_whose_output_breaks_ends_itself() {
    let home = Scratch::new("link-output");
    let mut node = OwnStandIn::start(&home, &[]);
    node.welcome(&KINDS, Routes::Any).await;
    let agent = at("agent", "a");
    node.write(&create(1, None, &agent));
    let (_, made) = node.until_reply(1).await;
    assert!(made.is_ok(), "{made:?}");
    node.close_output();
    let large = Fail::Uncoded {
        text: "x".repeat(1 << 20),
    };
    node.write(&request(2, None, &agent, &large));
    let (status, _) = node.ended(GONE_WAIT).await;
    #[cfg(unix)]
    assert_eq!(status.signal(), Some(9), "{status}");
    #[cfg(windows)]
    assert!(!status.success(), "{status}");
}

#[tokio::test]
async fn a_kind_both_hosts_hold_is_refused_at_open() {
    let home = Scratch::new("link-both");
    let record = home.join("starts");
    let opened = child::open(
        &home,
        factories(&["realm", "agent"]),
        HostOptions::default(),
        linked::child(&["--starts", arg(&record)]),
    );
    let Err(error) = opened else {
        panic!("a host and its child both hold `agent`");
    };
    assert!(format!("{error:#}").contains("`agent`"), "{error:#}");
    assert!(!record.exists());
}
