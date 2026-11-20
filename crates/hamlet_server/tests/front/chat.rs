//! A chief's chat through the door: lines, their replies, the frames
//! of the chat a client follows, and who may answer a sign-in.

use std::collections::HashSet;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use hamlet_acceptance::{Client, chat, fold};
use hamlet_protocol::client::{
    ChatRef, Check, Choice, ClientMessage, Edit, ItemKind, Login,
    LoginMethod, LoginReply, LoginStart, LoginStep, New, Open, Read,
    ReadResult, ReplyKind, RequestId, Resume, Retry, Say,
    ServerMessage, Stop, TranscriptOperation,
};
use hamlet_testing::{Scratch, Server};
use serde_json::json;

use super::{PATIENCE, serve, serve_with};

/// What the faux model answers the first line it is given.
const REPLY: &str = "The door carried this reply.";

/// A key typed in a sign-in, which nothing the server sends or
/// prints may hold.
const KEY: &str = "made-up-door-key-6b1e0c2f";

/// How long a stop may wait for the requests its connection sent
/// before it, when they wait for an agent that cannot be reached.
const STOP_BOUND: Duration = Duration::from_millis(500);

#[tokio::test(flavor = "multi_thread")]
async fn a_client_following_an_agents_chat_gets_its_reply() {
    let (mut server, _home, _side) = serve(json!([REPLY])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    chief
        .send(ClientMessage::Say(Say {
            id: RequestId::new(),
            to: hand,
            text: "Say something.".into(),
            files: Vec::new(),
            images: Vec::new(),
        }))
        .await;
    chief.shows(&chat, REPLY).await;
    let frames = frames(&chief);
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    for frame in &frames {
        assert!(
            !frame.contains(&server.token),
            "a frame carried the token: {frame}"
        );
    }
    assert!(
        !server.log().contains(&server.token),
        "the server printed the token"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_that_breaks_the_schema_is_refused_alone() {
    let (mut server, _home, _side) = serve(json!([REPLY])).await;
    let mut chief = Client::join(&server).await;
    let broken = RequestId::new();
    chief
        .send_raw(
            &json!({ "type": "open", "id": broken }).to_string(),
        )
        .await;
    assert!(
        matches!(
            chief.answer(&broken).await,
            ServerMessage::Fault(_)
        ),
        "a message outside the schema is refused with a fault"
    );
    let hand = chief.hand();
    let opened = chief
        .ask(|id| {
            ClientMessage::Open(Open {
                id,
                chat: chat(hand),
                before: None,
            })
        })
        .await;
    assert!(
        matches!(opened, ServerMessage::Transcript(_)),
        "the connection serves the next message: {opened:?}"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// Two lines sent back to back, as a piped `hamlet chat` sends them:
/// each is answered with the empty notice, and only once the
/// agent's answer to it is in the chat.
#[tokio::test(flavor = "multi_thread")]
async fn a_line_is_answered_when_its_turn_ends() {
    let (mut server, _home, _side) =
        serve(json!(["The first answer.", "The second answer."]))
            .await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    let first = chief.tell(hand, "First line.").await;
    let second = chief.tell(hand, "Second line.").await;
    for (line, answer) in [
        (&first, "The first answer."),
        (&second, "The second answer."),
    ] {
        let reply = chief.reply(line).await;
        assert_eq!(
            (reply.kind, reply.text.as_str()),
            (ReplyKind::Notice, ""),
            "a line is answered with the empty notice"
        );
        let at = chief
            .messages()
            .iter()
            .position(|message| message.to() == Some(line))
            .unwrap();
        let before = fold::items(
            &chief.messages()[..at],
            &chat,
            &HashSet::new(),
        );
        assert!(
            before.iter().any(|item| item.text == answer),
            "the reply to a line comes after the answer {answer:?}: \
             {before:?}"
        );
    }
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// The first answer takes 60 s unless stopped, three times the 20 s
/// a client waits for its reply, so only the stop can end it in
/// time; a stop takes well under a second here.
#[tokio::test(flavor = "multi_thread")]
async fn a_line_dropped_by_stop_is_answered() {
    let (mut server, _home, _side) =
        serve(json!([{ "text": "Too slow.", "waitMs": 60_000 }]))
            .await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    let slow = chief.tell(hand, "Take your time.").await;
    chief
        .until("the Hand to work", |chief| chief.is_working(&chat))
        .await;
    let stopped = chief
        .request(|id| {
            ClientMessage::Stop(Stop {
                agent: Some(hand),
                id,
            })
        })
        .await;
    assert_eq!(
        (stopped.kind, stopped.text.as_str()),
        (ReplyKind::Notice, "Stopped 1 running turns.")
    );
    let reply = chief.reply(&slow).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, ""),
        "a stopped line is answered with the empty notice"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// A line and a `/stop` sent back to back, as a piped `hamlet chat`
/// sends them, twenty times: the stop finds the line's turn and ends
/// it before its answer. Each answer takes 2 s unless stopped.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_sent_after_a_line_ends_that_lines_turn() {
    let rounds = 20;
    let answers = (0..rounds)
        .map(|n| json!({ "text": format!("Answer {n}."), "waitMs": 2_000 }))
        .collect::<Vec<_>>();
    let (mut server, _home, _side) = serve(json!(answers)).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    for n in 0..rounds {
        let line = chief.tell(hand, &format!("Line {n}.")).await;
        let stop = chief.tell(hand, "/stop").await;
        let stopped = chief.reply(&stop).await;
        let reply = chief.reply(&line).await;
        assert_eq!(
            (stopped.kind, stopped.text.as_str()),
            (ReplyKind::Notice, "Stopped 1 running turns."),
            "round {n}"
        );
        assert_eq!(
            (reply.kind, reply.text.as_str()),
            (ReplyKind::Notice, ""),
            "round {n}: a stopped line is answered with the empty \
             notice"
        );
        let answers = chief
            .items(&chat)
            .into_iter()
            .filter(|item| item.kind == ItemKind::Assistant)
            .map(|item| item.text)
            .collect::<Vec<_>>();
        assert!(
            answers.is_empty(),
            "round {n}: a stopped line's turn answered {answers:?}"
        );
    }
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// A line, then `/stop`, while the Node host is down: the stop drops
/// the line, which never runs once the host is back.
#[tokio::test(flavor = "multi_thread")]
async fn a_line_stopped_while_its_agent_is_down_never_runs() {
    let held = HeldNode::new();
    let (mut server, home, _side) =
        held.serve(json!(["Up.", "Back."])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Are you up?").await;
    kill_node(&home).await;
    let line = chief.tell(hand, "Are you there?").await;
    let sent = Instant::now();
    let stop = chief.tell(hand, "/stop").await;
    let stopped = chief.reply(&stop).await;
    let took = sent.elapsed();
    assert_eq!(
        (stopped.kind, stopped.text.as_str()),
        (ReplyKind::Notice, "Stopped 1 running turns.")
    );
    assert!(took < STOP_BOUND, "the stop took {took:?}");
    let reply = chief.reply(&line).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, ""),
        "a dropped line is answered with the empty notice"
    );
    held.release();
    let back = chief.say(hand, "Hello again.").await;
    assert_eq!(back.kind, ReplyKind::Notice, "{}", back.text);
    let page = chief.open(&chat(hand)).await;
    let lines = page
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::User)
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        ["Are you up?", "Hello again."],
        "the dropped line never reached the agent"
    );
    let replies = chief
        .messages()
        .iter()
        .filter(|message| message.to() == Some(&line))
        .count();
    assert_eq!(replies, 1, "the line is answered once");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// A read of an agent whose Node host is down waits for the host; a
/// `/stop` sent after it ends that wait and is answered at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_ends_the_wait_of_a_call_sent_before_it() {
    let held = HeldNode::new();
    let (mut server, home, _side) = held.serve(json!(["Up."])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Are you up?").await;
    kill_node(&home).await;
    let read = RequestId::new();
    chief
        .send(ClientMessage::Read(Read::Prompt {
            agent: hand,
            id: read.clone(),
        }))
        .await;
    let sent = Instant::now();
    let stop = chief.tell(hand, "/stop").await;
    let stopped = chief.reply(&stop).await;
    let took = sent.elapsed();
    assert_eq!(
        (stopped.kind, stopped.text.as_str()),
        (ReplyKind::Notice, "Stopped 0 running turns.")
    );
    assert!(took < STOP_BOUND, "the stop took {took:?}");
    assert!(
        matches!(chief.answer(&read).await, ServerMessage::Fault(_)),
        "the read is answered with the failure that kept it waiting"
    );
    held.release();
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_session_is_shown_to_its_watcher_once() {
    let (mut server, _home, _side) = serve(json!(["Hello."])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    chief.say(hand, "Hello.").await;
    for (number, slash) in [(2, true), (3, false)] {
        let asked = chief.messages().len();
        let began = if slash {
            chief.command(hand, "/new").await
        } else {
            chief
                .request(|id| {
                    ClientMessage::New(New { id, to: hand })
                })
                .await
                .text
        };
        assert_eq!(began, format!("Session {number} began."));
        chief.settle(Duration::from_secs(1)).await;
        let after = &chief.messages()[asked..];
        let shown = after
            .iter()
            .filter_map(|message| match message {
                ServerMessage::Reply(reply)
                    if reply.text == began =>
                {
                    Some("reply".to_owned())
                }
                ServerMessage::Transcript(page)
                    if page.to.is_none() && page.chat == chat =>
                {
                    let items = page
                        .items
                        .iter()
                        .map(|item| item.text.as_str())
                        .collect::<Vec<_>>();
                    Some(format!("page {items:?}"))
                }
                ServerMessage::Turn(turn)
                    if turn.operations.iter().any(|op| {
                        matches!(op, TranscriptOperation::Reset {})
                    }) =>
                {
                    Some("reset".to_owned())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            shown,
            ["reply", r#"page ["Started a new session."]"#],
            "session {number}: {after:#?}"
        );
    }
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_sent_again_after_a_drop_opens_one_task() {
    let (mut server, _home, _side) =
        serve(json!(["Once.", "Twice."])).await;
    let mut dropped = Client::join(&server).await;
    let said = Say {
        id: RequestId::new(),
        to: dropped.hand(),
        text: "Only once, please.".into(),
        files: Vec::new(),
        images: Vec::new(),
    };
    dropped.send(ClientMessage::Say(said.clone())).await;
    dropped.close().await;
    let mut chief = Client::join(&server).await;
    chief.send(ClientMessage::Say(said.clone())).await;
    let reply = chief.reply(&said.id).await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let page = chief.open(&chat(said.to)).await;
    let lines = page
        .items
        .iter()
        .filter(|item| {
            item.kind == ItemKind::User && item.text == said.text
        })
        .count();
    let answers = page
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Assistant)
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(lines, 1, "the line is in the chat once: {page:?}");
    assert_eq!(answers, ["Once."], "one task answered it");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_open_after_its_follow_ended_follows_again() {
    let (mut server, _home, _side) =
        serve(json!([REPLY, "The second reply."])).await;
    let mut other = Client::join(&server).await;
    let hand = other.hand();
    let chat = chat(hand);
    other.open(&chat).await;
    other.tell(hand, "Say something.").await;
    other.shows(&chat, REPLY).await;
    let mut chief = Client::join(&server).await;
    let missed = chief.page(&chat, "no-such-item").await;
    assert!(
        matches!(&missed, ServerMessage::Fault(_)),
        "a page before an item that is not there is a fault: \
         {missed:?}"
    );
    let past = chief
        .ask(|id| {
            ClientMessage::Open(Open {
                id,
                chat: ChatRef {
                    past: Some(77),
                    ..chat.clone()
                },
                before: None,
            })
        })
        .await;
    let ServerMessage::Reply(past) = past else {
        panic!("a session that is not there is refused: {past:?}");
    };
    assert_eq!(
        (past.kind, past.text.as_str()),
        (ReplyKind::Error, "there is no past session 77 here")
    );
    chief.open(&chat).await;
    chief.tell(hand, "Say more.").await;
    chief.shows(&chat, "The second reply.").await;
    other.close().await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// Each request the door answers from the actors, with what changed
/// shown to every client before the reply.
#[tokio::test(flavor = "multi_thread")]
async fn each_request_is_answered_in_todays_words() {
    let (mut server, _home, _side) =
        serve(json!(["Hello back."])).await;
    let mut chief = Client::join(&server).await;
    let mut other = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    other.open(&chat).await;
    let reply = chief.say(hand, "Hello.").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);

    let checked = chief
        .request(|id| {
            ClientMessage::Check(Check::Model {
                id,
                model: Some("faux::faux-1".into()),
            })
        })
        .await;
    assert_eq!(checked.kind, ReplyKind::Notice, "{}", checked.text);
    let unnamed = chief
        .request(|id| {
            ClientMessage::Check(Check::Model { id, model: None })
        })
        .await;
    assert_eq!(
        (unnamed.kind, unnamed.text.as_str()),
        (ReplyKind::Notice, "Every agent runs the faux model here."),
        "the window's check of the realm's model"
    );
    assert!(!chief.prompt(hand).await.is_empty(), "a prompt is read");
    let history = chief.history(hand).await;
    assert!(
        history.iter().any(|past| past.current && past.number == 1),
        "the history holds the current session: {history:?}"
    );
    let read = chief.read(|id| Read::Charter { id }).await;
    let ServerMessage::Read(ReadResult::Charter { charter, .. }) =
        read
    else {
        panic!("the charter is read: {read:?}");
    };
    assert!(!charter.own && !charter.text.is_empty(), "{charter:?}");

    let retried = chief
        .request(|id| ClientMessage::Retry(Retry { id, to: hand }))
        .await;
    assert_eq!(
        (retried.kind, retried.text.as_str()),
        (
            ReplyKind::Error,
            "the last message was answered, so there is nothing to \
             carry on"
        )
    );
    let line = chief
        .items(&chat)
        .into_iter()
        .find(|item| item.kind == ItemKind::User)
        .expect("the chief's line is in the chat");
    let rewound = chief.rewind(&chat, &line.id).await;
    assert_eq!(
        (rewound.kind, rewound.text.as_str()),
        (ReplyKind::Rewound, "Hello.")
    );

    let seen = other.messages().len();
    let asked = chief.messages().len();
    let began = chief
        .request(|id| ClientMessage::New(New { id, to: hand }))
        .await;
    assert_eq!(
        (began.kind, began.text.as_str()),
        (ReplyKind::Notice, "Session 2 began.")
    );
    let answered = chief
        .messages()
        .iter()
        .rposition(|message| {
            matches!(message, ServerMessage::Reply(_))
        })
        .unwrap();
    assert!(
        chief.messages()[asked..answered].iter().any(
            |message| matches!(message, ServerMessage::State(_))
        ),
        "the asker is shown the realm before the reply"
    );
    let reread = |message: &ServerMessage| {
        matches!(
            message,
            ServerMessage::Transcript(page)
                if page.to.is_none() && page.chat == chat
        )
    };
    other
        .until("the watcher's session read again", |other| {
            other.messages()[seen..].iter().any(reread)
        })
        .await;
    other
        .until("the watcher shown the realm", |other| {
            other.messages()[seen..].iter().any(|message| {
                matches!(message, ServerMessage::State(_))
            })
        })
        .await;
    let back = chief
        .request(|id| {
            ClientMessage::Resume(Resume {
                id,
                number: 1,
                to: hand,
            })
        })
        .await;
    assert_eq!(
        (back.kind, back.text.as_str()),
        (ReplyKind::Notice, "Back in session 1.")
    );
    let stopped = chief
        .request(|id| ClientMessage::Stop(Stop { agent: None, id }))
        .await;
    assert_eq!(
        (stopped.kind, stopped.text.as_str()),
        (ReplyKind::Notice, "Nothing was running.")
    );
    let answered = chief.choose("#99", Choice::Allow, None).await;
    assert_eq!(answered.kind, ReplyKind::Error, "{}", answered.text);
    assert!(
        answered.text.starts_with("nothing waits here"),
        "{}",
        answered.text
    );
    chief.close().await;
    other.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_answer_from_another_connection_is_refused() {
    let (mut server, _home, _side) = serve_signing_in().await;
    let mut chief = Client::join(&server).await;
    let mut other = Client::join(&server).await;
    let start = start_sign_in(&mut chief).await;
    let refused = other
        .request(|id| {
            ClientMessage::Login(Login::Reply {
                id,
                reply: LoginReply {
                    login: start.to_string(),
                    value: KEY.into(),
                },
            })
        })
        .await;
    assert_eq!(
        (refused.kind, refused.text.as_str()),
        (ReplyKind::Error, "No sign-in of yours is open.")
    );
    other
        .send(ClientMessage::Login(Login::Start {
            id: start.clone(),
            start: LoginStart {
                method: LoginMethod::Key,
                provider: "loopback".into(),
            },
        }))
        .await;
    let ServerMessage::Reply(restarted) = other.answer(&start).await
    else {
        panic!("a start another connection holds is answered");
    };
    assert_eq!(
        (restarted.kind, restarted.text.as_str()),
        (ReplyKind::Error, "No sign-in of yours is open.")
    );
    finish_sign_in(&mut chief, &start).await;
    let frames = [frames(&chief), frames(&other)].concat();
    chief.close().await;
    other.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    keeps_the_key_to_itself(&server, &frames);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_given_in_a_sign_in_reaches_no_client() {
    let (mut server, _home, _side) = serve_signing_in().await;
    let mut chief = Client::join(&server).await;
    let start = start_sign_in(&mut chief).await;
    finish_sign_in(&mut chief, &start).await;
    let hand = chief.hand();
    chief.open(&chat(hand)).await;
    let said = chief.say(hand, &format!("my key is {KEY}")).await;
    assert_eq!(said.kind, ReplyKind::Notice, "{}", said.text);
    let saved = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Charter {
                id,
                text: Some(format!("Our key is {KEY}.")),
            })
        })
        .await;
    assert_eq!(saved.kind, ReplyKind::Notice, "{}", saved.text);
    let read = chief.read(|id| Read::Charter { id }).await;
    assert!(
        matches!(
            &read,
            ServerMessage::Read(ReadResult::Charter { .. })
        ),
        "{read:?}"
    );
    let frames = frames(&chief);
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    assert!(
        frames.iter().any(|frame| frame.contains("my key is")),
        "the line reached its watcher"
    );
    keeps_the_key_to_itself(&server, &frames);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_to_a_subscription_shows_each_step_once() {
    let (mut server, _home, _side) = serve_signing_in().await;
    let mut chief = Client::join(&server).await;
    let start = RequestId::new();
    chief
        .send(ClientMessage::Login(Login::Start {
            id: start.clone(),
            start: LoginStart {
                method: LoginMethod::Subscription,
                provider: "chatgpt".into(),
            },
        }))
        .await;
    login_step(
        &mut chief,
        &start,
        "the prompt for the code",
        |step| matches!(step, LoginStep::Prompt { .. }),
    )
    .await;
    let answered = chief
        .request(|id| {
            ClientMessage::Login(Login::Reply {
                id,
                reply: LoginReply {
                    login: start.to_string(),
                    value: KEY.into(),
                },
            })
        })
        .await;
    assert_eq!(answered.kind, ReplyKind::Notice, "{}", answered.text);
    login_step(&mut chief, &start, "the sign-in's end", |step| {
        matches!(
            step,
            LoginStep::Done { .. } | LoginStep::Failed { .. }
        )
    })
    .await;
    let steps = chief
        .messages()
        .iter()
        .filter_map(|message| match message {
            ServerMessage::Login(progress)
                if progress.to.as_ref() == Some(&start) =>
            {
                Some(progress.step.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        steps,
        [
            LoginStep::Url {
                instructions: Some("Offline sign-in".into()),
                url: "https://example.invalid/oauth".into(),
            },
            LoginStep::Url {
                instructions: None,
                url: "https://example.invalid/device".into(),
            },
            LoginStep::Code {
                code: "SAFE-CODE".into(),
            },
            LoginStep::Progress {
                progress: "Waiting for the offline code".into(),
            },
            LoginStep::Prompt {
                prompt: "Paste the code.".into(),
                secret: true,
            },
            LoginStep::Done {
                done: "chatgpt".into(),
            },
        ],
        "each step of the sign-in reaches its client once, in order"
    );
    let frames = frames(&chief);
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    keeps_the_key_to_itself(&server, &frames);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_cancelled_from_another_connection_goes_on() {
    let (mut server, _home, _side) = serve_signing_in().await;
    let mut chief = Client::join(&server).await;
    let mut other = Client::join(&server).await;
    let start = start_sign_in(&mut chief).await;
    let refused = other
        .request(|id| {
            ClientMessage::Login(Login::Cancel {
                cancel: start.to_string(),
                id,
            })
        })
        .await;
    assert_eq!(
        (refused.kind, refused.text.as_str()),
        (ReplyKind::Error, "No sign-in of yours is open.")
    );
    finish_sign_in(&mut chief, &start).await;
    let frames = [frames(&chief), frames(&other)].concat();
    chief.close().await;
    other.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    keeps_the_key_to_itself(&server, &frames);
}

/// A server as [`serve`] starts it, whose Node host also lists two
/// loopback providers to sign in to.
async fn serve_signing_in() -> (Server, Scratch, Scratch) {
    serve_with(json!([REPLY]), &[("HAMLET_DEMO_SIGN_IN", "1")]).await
}

/// Starts a sign-in with a key on the loopback provider, and waits
/// for its prompt for the key.
async fn start_sign_in(chief: &mut Client) -> RequestId {
    let start = RequestId::new();
    chief
        .send(ClientMessage::Login(Login::Start {
            id: start.clone(),
            start: LoginStart {
                method: LoginMethod::Key,
                provider: "loopback".into(),
            },
        }))
        .await;
    let step =
        login_step(chief, &start, "the prompt for the key", |step| {
            matches!(step, LoginStep::Prompt { .. })
        })
        .await;
    assert_eq!(
        step,
        LoginStep::Prompt {
            prompt: "Paste your key.".into(),
            secret: true,
        }
    );
    start
}

/// Answers the sign-in `start` began with the key, and waits for it
/// to end signed in.
async fn finish_sign_in(chief: &mut Client, start: &RequestId) {
    let answered = chief
        .request(|id| {
            ClientMessage::Login(Login::Reply {
                id,
                reply: LoginReply {
                    login: start.to_string(),
                    value: KEY.into(),
                },
            })
        })
        .await;
    assert_eq!(
        (answered.kind, answered.text.as_str()),
        (ReplyKind::Notice, "Sign-in answer received.")
    );
    let ended =
        login_step(chief, start, "the sign-in's end", |step| {
            matches!(
                step,
                LoginStep::Done { .. } | LoginStep::Failed { .. }
            )
        })
        .await;
    assert_eq!(
        ended,
        LoginStep::Done {
            done: "loopback".into()
        }
    );
}

/// The first step of the sign-in `start` began that `matches`.
async fn login_step(
    chief: &mut Client,
    start: &RequestId,
    what: &str,
    matches: impl Fn(&LoginStep) -> bool,
) -> LoginStep {
    let found = |chief: &Client| {
        chief.messages().iter().find_map(|message| match message {
            ServerMessage::Login(progress)
                if progress.to.as_ref() == Some(start)
                    && matches(&progress.step) =>
            {
                Some(progress.step.clone())
            }
            _ => None,
        })
    };
    chief.until(what, |chief| found(chief).is_some()).await;
    found(chief).unwrap()
}

fn frames(chief: &Client) -> Vec<String> {
    chief
        .messages()
        .iter()
        .map(|message| serde_json::to_string(message).unwrap())
        .collect()
}

fn keeps_the_key_to_itself(server: &Server, frames: &[String]) {
    for frame in frames {
        assert!(
            !frame.contains(KEY),
            "a frame carried the key: {frame}"
        );
    }
    assert!(
        !server.log().contains(KEY),
        "the server printed the key"
    );
}

/// A `node` for the server that, once its first start ended, starts
/// again only when released.
struct HeldNode {
    side: Scratch,
}

impl HeldNode {
    fn new() -> Self {
        let side = Scratch::new("front-held-node");
        let script = format!(
            "#!/bin/sh\nif [ -e {started} ]; then\nwhile [ ! -e \
             {go} ]; do sleep 0.05; done\nfi\ntouch {started}\nexec \
             node \"$@\"\n",
            started = side.join("started").display(),
            go = side.join("go").display(),
        );
        let node = side.join("node");
        std::fs::write(&node, script).unwrap();
        std::fs::set_permissions(
            &node,
            Permissions::from_mode(0o755),
        )
        .unwrap();
        Self { side }
    }

    async fn serve(
        &self,
        responses: serde_json::Value,
    ) -> (Server, Scratch, Scratch) {
        let node = self.side.join("node");
        serve_with(
            responses,
            &[("HAMLET_NODE", node.to_str().unwrap())],
        )
        .await
    }

    /// Lets the Node host start again.
    fn release(&self) {
        std::fs::write(self.side.join("go"), "").unwrap();
    }
}

/// Kills the server's Node host and waits until it has ended.
async fn kill_node(home: &Scratch) {
    let pid = std::fs::read_to_string(home.join("demo-node.pid"))
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    hamlet_testing::signals::kill(pid).unwrap();
    let began = Instant::now();
    while !hamlet_os::process::has_ended(pid).unwrap() {
        assert!(
            began.elapsed() < PATIENCE,
            "the Node host never ended"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
