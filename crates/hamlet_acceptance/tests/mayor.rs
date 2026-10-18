//! The mayor alone: chat, turns, tools, approvals, faults, stops,
//! rewinds, sessions and paging.

use std::time::Duration;

use hamlet_acceptance::realm::{wait_id, wait_says};
use hamlet_acceptance::*;
use hamlet_protocol::v0::*;
use serde_json::json;

fn notice(reply: &Reply) -> (&ReplyKind, &str) {
    (&reply.kind, reply.text.as_str())
}

/// A folder for the hamlet to work on, outside the home.
fn folder(s: &Session) -> String {
    let dir = s.home.side().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    dir.display().to_string()
}

/// Has the first hamlet work on a folder of its own in manual mode.
async fn manual(s: &mut Session) -> String {
    let dir = folder(s);
    let reply = s.chief.say(mayor(), &format!("/folder {dir}")).await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    let reply = s.chief.say(mayor(), "/mode manual").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    dir
}

#[tokio::test]
async fn answers_a_line_said_to_the_mayor() {
    let mut s = Session::start(
        Script::new().responses([words("Hello, chief.")]),
    )
    .await;
    let mayor_chat = chat(mayor());
    let opened = s.chief.open(&mayor_chat).await;
    assert!(opened.items.is_empty());
    assert_eq!(opened.earlier, None);
    let reply = s.chief.say(mayor(), "Hello, mayor.").await;
    assert_eq!(notice(&reply), (&ReplyKind::Notice, ""));
    s.chief.shows(&mayor_chat, "Hello, chief.").await;
    s.chief.idle("home", "founder").await;
    let items = s.chief.items(&mayor_chat);
    assert_eq!(
        items
            .iter()
            .map(|i| (&i.kind, i.text.as_str(), i.chief.as_deref()))
            .collect::<Vec<_>>(),
        [
            (&ItemKind::User, "Hello, mayor.", Some("chief")),
            (&ItemKind::Assistant, "Hello, chief.", None)
        ]
    );
    let founder = s.chief.agent("home", "founder").unwrap();
    assert!(founder.mayor);
    assert_eq!(founder.model, "faux::faux-1");
    assert_eq!(
        founder.saying,
        AgentSaying {
            text: "Hello, chief.".into(),
            thought: false
        }
    );
}

#[tokio::test]
async fn streams_a_turn_whose_ops_equal_the_transcript_read_again() {
    let greeting = "Hello chief. The mayor is here and ready to \
                    work with you. "
        .repeat(6);
    let greeting = greeting.trim();
    let mut s = Session::start(
        Script::new().tokens_per_second(100.0).responses([
            call("bash", json!({"command": "echo hello"}))
                .thinking("I will greet the chief."),
            words(greeting),
        ]),
    )
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.say(mayor(), "Hello, mayor.").await;
    s.chief.shows(&mayor_chat, greeting).await;
    s.chief
        .until("the turn to end", |c| !c.working(&mayor_chat))
        .await;
    let ops = s.chief.ops(&mayor_chat);
    assert!(ops.iter().any(|op| matches!(op, Op::Append { .. })));
    let live = s.chief.items(&mayor_chat);
    let again = s.chief.open(&mayor_chat).await;
    assert_eq!(again.items, live);
    assert_eq!(
        again.items.iter().map(|i| &i.kind).collect::<Vec<_>>(),
        [
            &ItemKind::User,
            &ItemKind::Reasoning,
            &ItemKind::Tool,
            &ItemKind::Assistant
        ]
    );
    assert_eq!(again.items[1].text, "I will greet the chief.");
    let tool = again.items[2].tool.as_ref().unwrap();
    assert_eq!(
        (tool.name.as_str(), tool.exit, tool.failure.as_deref()),
        ("bash", Some(0), None)
    );
    let last = again.items.last().unwrap();
    assert_eq!(
        again.last,
        Some(Some(TranscriptLast {
            what: "replied".into(),
            at: last.at
        }))
    );
    assert!(!again.working);
    assert_eq!(again.pending, 0);
}

#[tokio::test]
async fn closes_each_tool_with_its_exit_and_failure() {
    let mut s = Session::start(Script::new().responses([
        calls([
            ("bash", json!({"command": "echo success"})),
            ("bash", json!({"command": "echo broken; exit 7"})),
        ]),
        words("One command succeeded and one failed."),
    ]))
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.say(mayor(), "Run both commands.").await;
    s.chief
        .shows(&mayor_chat, "One command succeeded and one failed.")
        .await;
    let closes = s
        .chief
        .ops(&mayor_chat)
        .into_iter()
        .filter_map(|op| match op {
            Op::Close { exit, failure, .. } => Some((exit, failure)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(closes, [(0, None), (7, Some("broken".into()))]);
    let tools = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .filter_map(|i| i.tool)
        .map(|t| t.summary)
        .collect::<Vec<_>>();
    assert_eq!(
        tools,
        [
            r#"{"command":"echo success"}"#,
            r#"{"command":"echo broken; exit 7"}"#
        ]
    );
}

#[tokio::test]
async fn holds_a_command_and_an_edit_for_approval_in_manual_mode() {
    let mut s = Session::start(Script::new().responses([
        calls([
            (
                "bash",
                json!({"command": "printf 'heading\\napproved\\n' > result.txt"}),
            ),
            (
                "write",
                json!({"path": "result.txt", "content": "heading\nreviewed\n"}),
            ),
        ]),
        words("The approved command finished."),
    ]))
    .await;
    let dir = manual(&mut s).await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief
        .tell(mayor(), "Run the command and write in manual mode.")
        .await;
    let command = s
        .chief
        .wait_for("a command approval", |w| {
            matches!(
                w,
                Wait::Approval {
                    danger: WaitDanger::Command,
                    ..
                }
            )
        })
        .await;
    let Wait::Approval {
        answers,
        asked,
        about,
        agent,
        ..
    } = &command
    else {
        unreachable!()
    };
    assert_eq!(
        answers,
        &[Choice::Allow, Choice::Always, Choice::Deny]
    );
    assert_eq!(*asked, 1);
    assert_eq!(
        agent,
        &Some(AgentRef {
            hamlet: "home".into(),
            number: 0
        })
    );
    assert_eq!(about, "printf 'heading\\napproved\\n' > result.txt");
    assert_eq!(wait_says(&command), "wants to run");
    let reply =
        s.chief.choose(wait_id(&command), Choice::Allow, None).await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    let edit = s
        .chief
        .wait_for("an edit approval", |w| {
            matches!(
                w,
                Wait::Approval {
                    danger: WaitDanger::Edit,
                    ..
                }
            )
        })
        .await;
    let Wait::Approval {
        change: Some(change),
        ..
    } = &edit
    else {
        panic!("an edit approval carries its change: {edit:?}")
    };
    assert_eq!(wait_says(&edit), "wants to edit");
    assert_eq!(
        change,
        &Change {
            path: "result.txt".into(),
            created: false,
            lines: vec![
                ChangeLines {
                    mark: ChangeLinesMark::Space,
                    text: "heading".into()
                },
                ChangeLines {
                    mark: ChangeLinesMark::Minus,
                    text: "approved".into()
                },
                ChangeLines {
                    mark: ChangeLinesMark::Plus,
                    text: "reviewed".into()
                },
            ],
            total: 3,
            added: 1,
            removed: 1,
            count: "1 line added, 1 removed".into(),
        }
    );
    s.chief.choose(wait_id(&edit), Choice::Allow, None).await;
    s.chief
        .shows(&mayor_chat, "The approved command finished.")
        .await;
    let closed = s
        .chief
        .changes()
        .into_iter()
        .filter_map(|u| match u {
            Update::WaitClosed { id, how } => Some((id, how)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        closed,
        [
            (wait_id(&command).to_string(), "allow".to_string()),
            (wait_id(&edit).to_string(), "allow".to_string())
        ]
    );
    let write = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .find_map(|i| i.tool.filter(|t| t.name == "write"))
        .unwrap();
    assert_eq!(write.change.as_ref(), Some(change));
    assert_eq!(
        std::fs::read_to_string(format!("{dir}/result.txt")).unwrap(),
        "heading\nreviewed\n"
    );
    let reply = s.chief.say(mayor(), "/mode auto").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief
        .until("the hamlet in auto mode", |c| {
            c.realm().hamlets[0].mode == HamletMode::Auto
        })
        .await;
}

#[tokio::test]
async fn refuses_a_denied_command_and_tells_the_model_why() {
    let mut s = Session::start(Script::new().responses([
        call("bash", json!({"command": "echo never"})),
        words("Understood."),
    ]))
    .await;
    let dir = manual(&mut s).await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Run it.").await;
    s.chief
        .wait_for("a command approval", |w| {
            matches!(
                w,
                Wait::Approval {
                    danger: WaitDanger::Command,
                    ..
                }
            )
        })
        .await;
    let reply =
        s.chief.say(mayor(), "/deny not on this machine").await;
    assert_eq!(
        notice(&reply),
        (
            &ReplyKind::Notice,
            "Answered /deny: mayor wants to run `echo never`"
        )
    );
    s.chief.shows(&mayor_chat, "Understood.").await;
    let tool = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .find_map(|i| i.tool)
        .unwrap();
    assert!(
        tool.failure
            .as_deref()
            .is_some_and(|f| f.contains("not on this machine")),
        "{tool:?}"
    );
    assert!(!std::path::Path::new(&dir).join("never").exists());
}

#[tokio::test]
async fn keeps_words_from_a_waiting_mayor_and_answers_by_command() {
    let mut s = Session::start(Script::new().responses([
        call("bash", json!({"command": "echo allowed"})),
        words("Done."),
    ]))
    .await;
    manual(&mut s).await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Run it.").await;
    s.chief
        .wait_for("a command approval", |w| {
            matches!(w, Wait::Approval { .. })
        })
        .await;
    let asked = "[approval] mayor wants to run `echo allowed`\n    \
                 echo allowed\n    answer with: /allow /always /deny";
    let reply = s.chief.say(mayor(), "Something else.").await;
    assert_eq!(
        notice(&reply),
        (
            &ReplyKind::Notice,
            format!(
                "Not sent. mayor is waiting for your \
                 answer:\n{asked}"
            )
            .as_str()
        )
    );
    let reply = s.chief.say(mayor(), "/go").await;
    assert_eq!(
        reply.text,
        format!("That cannot be answered that way:\n{asked}")
    );
    let reply = s.chief.say(to("home", "nowhere"), "/queue").await;
    assert!(reply.text.contains(asked), "{reply:?}");
    let reply = s.chief.say(to("home", "nowhere"), "/allow").await;
    assert_eq!(
        reply.text,
        "Answered /allow: mayor wants to run `echo allowed`"
    );
    s.chief.shows(&mayor_chat, "Done.").await;
    let reply = s.chief.say(mayor(), "/queue").await;
    assert_eq!(reply.text, "Nothing waits for you.");
    let users = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .filter(|i| i.kind == ItemKind::User)
        .count();
    assert_eq!(users, 1);
}

#[tokio::test]
async fn gives_and_takes_back_a_standing_network_permission() {
    let mut s = Session::start(Script::new().responses([
        call("bash", json!({"command": "echo net", "network": true})),
        words("Fetched."),
    ]))
    .await;
    let reply = s.chief.say(mayor(), "/grants").await;
    assert_eq!(reply.text, "No agent holds a permission.");
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Fetch it.").await;
    let wait = s
        .chief
        .wait_for("a network approval", |w| {
            matches!(
                w,
                Wait::Approval {
                    danger: WaitDanger::Network,
                    ..
                }
            )
        })
        .await;
    assert_eq!(wait_says(&wait), "wants the network for");
    let reply = s.chief.say(mayor(), "/always").await;
    assert_eq!(
        reply.text,
        "Answered /always: mayor wants the network for `echo net`"
    );
    s.chief.shows(&mayor_chat, "Fetched.").await;
    s.chief
        .until("a network grant", |c| {
            c.realm().grants.iter().any(|g| {
                g.permission == Permission::Network
                    && g.source == GrantSource::Grant
            })
        })
        .await;
    let reply = s.chief.say(mayor(), "/grants").await;
    assert_eq!(
        reply.text,
        "mayor may use: network (given by chief; /revoke mayor \
         network)"
    );
    let reply = s.chief.say(mayor(), "/revoke mayor").await;
    assert_eq!(
        reply.text,
        "usage: /revoke <agent> <network|unconfined>"
    );
    let reply = s.chief.say(mayor(), "/revoke founder network").await;
    assert_eq!(reply.text, "`founder` no longer has network.");
    s.chief
        .until("the grant taken back", |c| {
            c.realm().grants.is_empty()
        })
        .await;
    let reply = s.chief.say(mayor(), "/grants").await;
    assert_eq!(reply.text, "No agent holds a permission.");
}

#[tokio::test]
async fn names_a_permission_the_limits_give() {
    let mut s = Session::start(Script::new()).await;
    let reply = s
        .chief
        .limit("home", "permissions", "unconfined", json!(["mayor"]))
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    let reply = s.chief.say(mayor(), "/grants").await;
    assert_eq!(reply.text, "mayor may use: unconfined (limits.toml)");
    let reply =
        s.chief.say(mayor(), "/revoke mayor unconfined").await;
    assert_eq!(
        reply.text,
        "Revoked what was granted. `mayor` still has unconfined \
         under [permissions] in limits.toml."
    );
}

#[tokio::test]
async fn faults_when_the_model_fails_until_dismissed() {
    let mut s = Session::start(
        Script::new().responses([failure("Faux model unavailable")]),
    )
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Try the model.").await;
    let wait = s
        .chief
        .wait_for("a fault", |w| matches!(w, Wait::Fault { .. }))
        .await;
    let Wait::Fault { about, answers, .. } = &wait else {
        unreachable!()
    };
    assert!(about.contains("Faux model unavailable"), "{about}");
    assert_eq!(answers, &[Choice::Dismiss]);
    s.chief
        .until("the mayor faulted", |c| {
            c.agent("home", "founder")
                .is_some_and(|a| a.faulted && !a.busy)
        })
        .await;
    let error = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .find(|i| i.kind == ItemKind::Error)
        .unwrap();
    assert!(
        error.text.contains("Faux model unavailable"),
        "{error:?}"
    );
    let reply =
        s.chief.choose(wait_id(&wait), Choice::Dismiss, None).await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief
        .until("the fault closed", |c| c.waits().is_empty())
        .await;
    assert!(s.chief.changes().contains(&Update::WaitClosed {
        id: wait_id(&wait).into(),
        how: "dismiss".into()
    }));
}

#[tokio::test]
async fn faults_on_an_empty_answer() {
    let mut s =
        Session::start(Script::new().responses([words("")])).await;
    s.chief.tell(mayor(), "Say something.").await;
    let wait = s
        .chief
        .wait_for("a fault", |w| matches!(w, Wait::Fault { .. }))
        .await;
    let Wait::Fault { about, .. } = &wait else {
        unreachable!()
    };
    assert_eq!(
        about,
        "the model answered with nothing. Send the message again"
    );
}

#[tokio::test]
async fn carries_on_after_a_failure_with_retry() {
    let mut s = Session::start(Script::new().responses([
        failure("Faux model unavailable"),
        words("Carried on."),
    ]))
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Try the model.").await;
    s.chief
        .wait_for("a fault", |w| matches!(w, Wait::Fault { .. }))
        .await;
    let reply = s
        .chief
        .request(|id| ClientMessage::Retry(Retry { id, to: mayor() }))
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    s.chief.shows(&mayor_chat, "Carried on.").await;
    s.chief.idle("home", "founder").await;
    let reply = s.chief.say(mayor(), "/retry").await;
    assert_eq!(
        notice(&reply),
        (
            &ReplyKind::Notice,
            "the last message was answered, so there is nothing to \
             carry on"
        )
    );
}

#[tokio::test]
async fn refuses_to_retry_while_a_turn_runs() {
    let mut s = Session::start(
        Script::new()
            .responses([words("Slow.").held_until("release")]),
    )
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Take your time.").await;
    s.chief
        .until("the turn to run", |c| c.working(&mayor_chat))
        .await;
    let typed = s.chief.say(mayor(), "/retry").await;
    assert_eq!(
        notice(&typed),
        (
            &ReplyKind::Notice,
            "A turn is running here. Stop it, then retry."
        )
    );
    let sent = s
        .chief
        .request(|id| ClientMessage::Retry(Retry { id, to: mayor() }))
        .await;
    assert_eq!(
        notice(&sent),
        (
            &ReplyKind::Error,
            "A turn is running here. Stop it, then retry."
        )
    );
    std::fs::write(s.home.side().join("release"), "").unwrap();
    s.chief.shows(&mayor_chat, "Slow.").await;
}

#[tokio::test]
async fn stops_a_running_command_and_its_turn() {
    let mut s = Session::start(Script::new().responses([
        call(
            "bash",
            json!({"command": "echo started > NOTES.md; sleep 30"}),
        ),
        words("This answer must never arrive."),
    ]))
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Run until I stop you.").await;
    let notes = s.home.file("houses/home/0/NOTES.md");
    s.chief
        .until("the command to start", |_| {
            std::fs::read_to_string(&notes)
                .is_ok_and(|t| t.trim() == "started")
        })
        .await;
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Stop(Stop {
                id,
                agent: Some(mayor()),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    s.chief
        .until("the stop notice", |c| {
            c.items(&mayor_chat).last().is_some_and(|i| {
                i.kind == ItemKind::Notice
                    && i.text.starts_with("Stopped:")
            })
        })
        .await;
    s.chief.idle("home", "founder").await;
    s.chief.settle(Duration::from_millis(500)).await;
    assert!(
        !s.chief
            .texts(&mayor_chat)
            .contains(&"This answer must never arrive.".to_string())
    );
    let tool = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .find_map(|i| i.tool)
        .unwrap();
    assert_ne!(tool.exit, Some(0));
}

#[tokio::test]
async fn answers_stop_commands_in_words() {
    let mut s =
        Session::start(Script::new().responses([call(
            "bash",
            json!({"command": "echo never"}),
        )]))
        .await;
    manual(&mut s).await;
    s.chief.tell(mayor(), "Run it.").await;
    s.chief
        .wait_for("an approval", |w| {
            matches!(w, Wait::Approval { .. })
        })
        .await;
    let reply = s.chief.say(mayor(), "/stop nobody").await;
    assert_eq!(
        notice(&reply),
        (&ReplyKind::Error, "no agent named `nobody`")
    );
    assert_eq!(s.chief.waits().len(), 1);
    let reply = s.chief.say(mayor(), "/stop founder").await;
    assert_eq!(reply.text, "Stopping founder.");
    s.chief
        .until("the approval to close", |c| c.waits().is_empty())
        .await;
    let reply = s.chief.say(mayor(), "/stop mayor").await;
    assert_eq!(reply.text, "Stopping mayor.");
    let reply = s.chief.say(mayor(), "/stop all").await;
    assert_eq!(reply.text, "Stopping everything.");
    let reply = s
        .chief
        .request(|id| ClientMessage::Stop(Stop { id, agent: None }))
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
}

#[tokio::test]
async fn rewinds_a_chief_line_and_what_followed() {
    let mut s = Session::start(Script::new().responses([
        words("First answer."),
        words("Second answer."),
    ]))
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.say(mayor(), "Keep this line.").await;
    s.chief.shows(&mayor_chat, "First answer.").await;
    s.chief.say(mayor(), "Take back this line.").await;
    s.chief.shows(&mayor_chat, "Second answer.").await;
    s.chief.idle("home", "founder").await;
    let items = s.chief.items(&mayor_chat);
    assert_eq!(items[0].rewindable, Some(true));
    let target = items
        .iter()
        .find(|i| i.text == "Take back this line.")
        .unwrap()
        .id
        .clone();
    let chat_ref = mayor_chat.clone();
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Rewind(Rewind {
                id,
                chat: chat_ref,
                item: target,
            })
        })
        .await;
    assert_eq!(
        notice(&reply),
        (&ReplyKind::Rewound, "Take back this line.")
    );
    s.chief
        .until("the rewound transcript", |c| {
            c.texts(&mayor_chat)
                == ["Keep this line.", "First answer."]
        })
        .await;
    let reset = s
        .chief
        .messages()
        .iter()
        .position(|m| {
            matches!(m, ServerMessage::Turn(t)
                if t.ops.iter().any(|op| matches!(op, Op::Reset {})))
        })
        .expect("a rewind resets its watchers");
    let ServerMessage::Transcript(after) =
        &s.chief.messages()[reset + 1]
    else {
        panic!("a reset is followed at once by its transcript")
    };
    assert_eq!(after.chat, mayor_chat);
}

#[tokio::test]
async fn starts_a_new_session_and_resumes_the_first() {
    let mut s = Session::start(
        Script::new().responses([words("The first conversation.")]),
    )
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief
        .say(mayor(), "Remember our first conversation.")
        .await;
    s.chief.shows(&mayor_chat, "The first conversation.").await;
    s.chief.idle("home", "founder").await;
    let reply = s.chief.say(mayor(), "/new").await;
    assert_eq!(
        notice(&reply),
        (&ReplyKind::Notice, "Started a new session.")
    );
    let history = s.chief.history(mayor()).await;
    let first = history.iter().find(|p| p.number == 1).unwrap();
    assert!(!first.current);
    assert_eq!(first.messages, 2);
    assert_eq!(first.by, "chief");
    // The new session is listed once it is used.
    assert_eq!(history.len(), 1);
    let reply = s.chief.say(mayor(), "/resume wrong").await;
    assert_eq!(
        reply.text,
        "Say which: `/resume <number>`, with the number History \
         shows for a past session."
    );
    let reply = s.chief.say(mayor(), "/resume 99").await;
    assert_eq!(
        notice(&reply),
        (&ReplyKind::Notice, "There is no past session `99` here.")
    );
    let reply = s.chief.say(mayor(), "/resume 1").await;
    assert_eq!(reply.text, "Went back to an earlier session.");
    s.chief
        .until("the first session back", |c| {
            c.texts(&mayor_chat)
                == [
                    "Remember our first conversation.",
                    "The first conversation.",
                    "Went back to an earlier session.",
                ]
        })
        .await;
    let past = ChatRef {
        past: Some(2),
        ..chat(mayor())
    };
    let page = s.chief.open(&past).await;
    assert_eq!(page.number, Some(2));
    assert_eq!(
        page.items
            .iter()
            .map(|i| (&i.kind, i.text.as_str()))
            .collect::<Vec<_>>(),
        [(&ItemKind::Notice, "Started a new session.")]
    );
}

#[tokio::test]
async fn starts_and_resumes_sessions_by_message() {
    let mut s = Session::start(Script::new()).await;
    let reply = s
        .chief
        .request(|id| ClientMessage::New(New { id, to: mayor() }))
        .await;
    assert_eq!(
        notice(&reply),
        (&ReplyKind::Notice, "Started a new session.")
    );
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Resume(Resume {
                id,
                to: mayor(),
                number: 1,
            })
        })
        .await;
    assert_eq!(reply.text, "Went back to an earlier session.");
}

#[tokio::test]
async fn pages_a_long_transcript_forty_items_at_a_time() {
    let mut s =
        Session::start(Script::new().responses(
            (1..=23).map(|i| words(&format!("Answer {i}."))),
        ))
        .await;
    for i in 1..=23 {
        let reply = s.chief.say(mayor(), &format!("Line {i}.")).await;
        assert_eq!(notice(&reply), (&ReplyKind::Notice, ""));
    }
    let mayor_chat = chat(mayor());
    let latest = s.chief.open(&mayor_chat).await;
    assert_eq!(latest.items.len(), 40);
    assert_eq!(latest.items[0].text, "Line 4.");
    assert_eq!(latest.earlier.as_ref(), Some(&latest.items[0].id));
    let ServerMessage::Transcript(older) = s
        .chief
        .page(&mayor_chat, latest.earlier.as_ref().unwrap())
        .await
    else {
        panic!("an older page")
    };
    assert_eq!(older.earlier, None);
    let all = older
        .items
        .iter()
        .chain(&latest.items)
        .map(|i| i.text.clone())
        .collect::<Vec<_>>();
    let expected = (1..=23)
        .flat_map(|i| [format!("Line {i}."), format!("Answer {i}.")])
        .collect::<Vec<_>>();
    assert_eq!(all, expected);
    let ids = older
        .items
        .iter()
        .chain(&latest.items)
        .map(|i| &i.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 46);
    assert!(matches!(
        s.chief.page(&mayor_chat, "no-such-item").await,
        ServerMessage::Fault(_)
    ));
}

#[tokio::test]
async fn admits_a_line_sent_twice_with_one_id_once() {
    let mut s =
        Session::start(Script::new().responses([words("Once.")]))
            .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    let line = ClientMessage::Say(Say {
        id: id(),
        to: mayor(),
        text: "Only once.".into(),
    });
    s.chief.send(line.clone()).await;
    s.chief.send(line).await;
    s.chief.shows(&mayor_chat, "Once.").await;
    s.chief.idle("home", "founder").await;
    s.chief.settle(Duration::from_millis(500)).await;
    assert_eq!(s.chief.texts(&mayor_chat), ["Only once.", "Once."]);
}

#[tokio::test]
async fn stops_a_turn_when_the_budget_runs_out() {
    let mut s =
        Session::start(Script::new().price(1_000_000.0).responses([
            call("read", json!({"path": "NOTES.md"})),
            words("This answer must never arrive."),
        ]))
        .await;
    let reply = s.chief.say(mayor(), "/budget").await;
    assert_eq!(
        reply.text,
        "Hamlet today: $0.00 of $5.00\n- mayor: faux::faux-1, $0.00"
    );
    let reply =
        s.chief.limit("home", "budget", "cost", json!(0.01)).await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief
        .say(mayor(), "Read your notes until the budget stops you.")
        .await;
    s.chief
        .until("the budget notice", |c| {
            c.items(&mayor_chat).last().is_some_and(|i| {
                i.kind == ItemKind::Notice
                    && i.text.contains("budget")
                    && i.text.contains("$0.01")
                    && i.text.contains("chief")
            })
        })
        .await;
    s.chief.idle("home", "founder").await;
    assert!(
        !s.chief
            .texts(&mayor_chat)
            .contains(&"This answer must never arrive.".to_string())
    );
    let founder = s.chief.agent("home", "founder").unwrap();
    assert!(founder.spent.micros > 0);
    let reply = s.chief.say(mayor(), "/reset-budget").await;
    assert_eq!(
        reply.text,
        "The budget starts again from here. What was spent before \
         is kept in the ledger."
    );
}

#[tokio::test]
async fn answers_a_question_the_mayor_asks() {
    let mut s = Session::start(Script::new().responses([
        call("ask_chief", json!({"question": "Which grain?"})),
        words("Wheat it is."),
    ]))
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Plant something.").await;
    let wait = s
        .chief
        .wait_for("a question", |w| {
            matches!(w, Wait::Question { .. })
        })
        .await;
    assert_eq!(
        wait,
        s.chief.waits()[0],
        "one question waits for the chief"
    );
    let Wait::Question { about, answers, .. } = &wait else {
        unreachable!()
    };
    assert_eq!(about, "Which grain?");
    assert!(answers.contains(&Choice::Reply), "{answers:?}");
    let reply = s.chief.say(mayor(), "/reply").await;
    assert_eq!(reply.text, "say what to reply: /reply <text>");
    let reply = s.chief.say(mayor(), "Wheat").await;
    assert!(reply.text.contains("Answered /reply"), "{reply:?}");
    s.chief.shows(&mayor_chat, "Wheat it is.").await;
}

#[tokio::test]
async fn answers_typed_commands_that_change_nothing() {
    let mut s = Session::start(Script::new().responses([
        words("One."),
        words("Two."),
        words("Three."),
    ]))
    .await;
    let reply = s.chief.say(mayor(), "/undo").await;
    assert_eq!(
        notice(&reply),
        (
            &ReplyKind::Notice,
            "Nothing to undo. Only a removal or a merge can be \
             undone (the archive keeps the house); a permission is \
             taken back with /revoke. A command that ran, a file \
             that was changed and what was spent cannot be undone."
        )
    );
    let reply = s.chief.say(mayor(), "/queue").await;
    assert_eq!(reply.text, "Nothing waits for you.");
    let reply = s.chief.say(mayor(), "/mode fast").await;
    assert_eq!(
        notice(&reply),
        (&ReplyKind::Error, "usage: /mode <manual|auto>")
    );
    // Unknown commands and commands with words go to the mayor as
    // words.
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    for line in ["/nothing", "/new words", "/NEW"] {
        let reply = s.chief.say(mayor(), line).await;
        assert_eq!(
            notice(&reply),
            (&ReplyKind::Notice, ""),
            "{line}"
        );
    }
    s.chief.shows(&mayor_chat, "Three.").await;
    let users = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .filter(|i| i.kind == ItemKind::User)
        .map(|i| i.text)
        .collect::<Vec<_>>();
    assert_eq!(users, ["/nothing", "/new words", "/NEW"]);
}

#[tokio::test]
async fn keeps_a_wait_and_the_chat_across_a_crash() {
    let mut home = Home::new();
    let dir = home.side().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    let server = home
        .serve(
            &Script::new().responses([
                words("Before the crash."),
                call("write", json!({"path": "b.txt", "content": "after restart"})),
            ]),
            QUIET,
        )
        .await;
    let mut chief = Client::join(&server).await;
    let mayor_chat = chat(mayor());
    chief.open(&mayor_chat).await;
    chief.say(mayor(), "Hello.").await;
    chief.shows(&mayor_chat, "Before the crash.").await;
    chief
        .say(mayor(), &format!("/folder {}", dir.display()))
        .await;
    chief.say(mayor(), "/mode manual").await;
    chief.tell(mayor(), "Make another file.").await;
    let wait = chief
        .wait_for("an edit approval", |w| {
            matches!(
                w,
                Wait::Approval {
                    danger: WaitDanger::Edit,
                    ..
                }
            )
        })
        .await;
    server.kill().await;
    let server = home
        .serve(
            &Script::new().responses([words("After restart.")]),
            QUIET,
        )
        .await;
    let mut chief = Client::join(&server).await;
    let again = chief.waits();
    assert_eq!(
        again,
        std::slice::from_ref(&wait),
        "the same wait after a crash"
    );
    let page = chief.open(&mayor_chat).await;
    assert_eq!(page.items[0].text, "Hello.");
    assert_eq!(page.items[1].text, "Before the crash.");
    chief.choose(wait_id(&wait), Choice::Allow, None).await;
    chief.shows(&mayor_chat, "After restart.").await;
    assert_eq!(
        std::fs::read_to_string(dir.join("b.txt")).unwrap(),
        "after restart"
    );
}

#[tokio::test]
async fn stops_in_order_and_carries_on_at_the_next_start() {
    stops_in_order(false).await;
}

#[tokio::test]
async fn stops_in_order_at_ctrl_c_too() {
    stops_in_order(true).await;
}

#[tokio::test]
async fn ends_at_once_at_a_second_ctrl_c() {
    let mut s = Session::start(
        Script::new().responses([words("Too late.").stalled(30_000)]),
    )
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Take your time.").await;
    s.chief
        .until("the line in the chat", |c| {
            c.items(&mayor_chat).len() == 1
        })
        .await;
    s.chief.settle(Duration::from_millis(300)).await;
    s.server.interrupt();
    s.chief.settle(Duration::from_millis(500)).await;
    assert!(
        s.server.running(),
        "the first Ctrl-C waits for the turn"
    );
    s.server.interrupt();
    let began = std::time::Instant::now();
    assert_eq!(s.server.ended().await, Some(130));
    assert!(began.elapsed() < Duration::from_secs(2));
}

/// SIGTERM, or SIGINT when `interrupt`, ends the server in order.
async fn stops_in_order(interrupt: bool) {
    let mut home = Home::new();
    let server = home
        .serve(
            &Script::new().responses([call(
                "bash",
                json!({"command": "sleep 5"}),
            )]),
            QUIET,
        )
        .await;
    let mut chief = Client::join(&server).await;
    let mayor_chat = chat(mayor());
    chief.open(&mayor_chat).await;
    chief.tell(mayor(), "Sleep.").await;
    chief
        .until("the tool to start", |c| {
            c.items(&mayor_chat).iter().any(|i| i.tool.is_some())
        })
        .await;
    let code = if interrupt {
        let mut server = server;
        server.interrupt();
        server.ended().await
    } else {
        server.stop().await
    };
    assert_eq!(code, Some(0));
    assert!(!home.file("server.json").exists());
    let (_, ending) = chief_rest(chief).await;
    assert_eq!(ending, Ending::Closed(1001));
    let server = home
        .serve(
            &Script::new().responses([words("Carried on.")]),
            QUIET,
        )
        .await;
    let mut chief = Client::join(&server).await;
    chief.open(&mayor_chat).await;
    chief.shows(&mayor_chat, "Carried on.").await;
    let tool = chief
        .items(&mayor_chat)
        .into_iter()
        .find_map(|i| i.tool)
        .unwrap();
    assert_ne!(tool.exit, Some(0));
}

async fn chief_rest(chief: Client) -> (Vec<ServerMessage>, Ending) {
    chief.into_wire().rest().await
}

#[tokio::test]
async fn tells_what_the_mayor_is_doing() {
    let tail = "é".repeat(400) + "done";
    let mut s = Session::start(Script::new().responses([
        call("read", json!({"path": "NOTES.md"})),
        call(
            "write",
            json!({"path": "NOTES.md", "content": "remember"}),
        ),
        words(&tail),
    ]))
    .await;
    s.chief.say(mayor(), "Look and remember.").await;
    s.chief
        .until("the mayor's last words", |c| {
            c.agent("home", "founder").is_some_and(|a| {
                !a.busy && a.saying.text.ends_with("done")
            })
        })
        .await;
    let founder = s.chief.agent("home", "founder").unwrap();
    // The last 600 bytes of what it said.
    assert_eq!(founder.saying.text, "é".repeat(298) + "done");
    let doings = s
        .chief
        .changes()
        .into_iter()
        .filter_map(|u| match u {
            Update::Agent { agent } => Some(agent.activity),
            _ => None,
        })
        .map(|a| (a.doing, a.doing_words))
        .collect::<Vec<_>>();
    assert!(
        doings.contains(&(
            Doing::ReadNotes,
            "Reading its notes".into()
        )),
        "{doings:?}"
    );
    assert!(
        doings.contains(&(
            Doing::WriteNotes,
            "Writing its notes".into()
        )),
        "{doings:?}"
    );
    assert_eq!(
        std::fs::read_to_string(
            s.home.file("houses/home/0/NOTES.md")
        )
        .unwrap(),
        "remember"
    );
}

fn notes(s: &Session) -> String {
    std::fs::read_to_string(s.home.file("houses/home/0/NOTES.md"))
        .unwrap()
}

fn reflected() -> [Step; 2] {
    [
        call(
            "write",
            json!({"path": "NOTES.md", "content": "# Notes\nReflected.\n"}),
        ),
        words("Noted."),
    ]
}

#[tokio::test]
async fn reflects_every_n_good_turns_but_not_after_a_failure() {
    let script = Script::new().responses(
        [failure("Bad request"), words("One."), words("Two.")]
            .into_iter()
            .chain(reflected()),
    );
    let mut s =
        Session::start_with(script, &[("HAMLET_REFLECT_EVERY", "2")])
            .await;
    for line in ["Fail.", "First."] {
        s.chief.say(mayor(), line).await;
        s.chief.idle("home", "founder").await;
    }
    s.chief.settle(Duration::from_millis(500)).await;
    assert_eq!(notes(&s), "# Notes\n");
    s.chief.say(mayor(), "Second.").await;
    let file = s.home.file("houses/home/0/NOTES.md");
    s.chief
        .until("the reflection", |_| {
            std::fs::read_to_string(&file).unwrap()
                == "# Notes\nReflected.\n"
        })
        .await;
}

#[tokio::test]
async fn reflects_when_the_chief_asks() {
    let script = Script::new()
        .responses([words("Yes.")].into_iter().chain(reflected()));
    let mut s = Session::start(script).await;
    s.chief.say(mayor(), "Succeed.").await;
    s.chief.idle("home", "founder").await;
    s.chief.settle(Duration::from_millis(300)).await;
    assert_eq!(notes(&s), "# Notes\n");
    let reply = s.chief.say(mayor(), "/reflect").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    let file = s.home.file("houses/home/0/NOTES.md");
    s.chief
        .until("the reflection", |_| {
            std::fs::read_to_string(&file).unwrap()
                == "# Notes\nReflected.\n"
        })
        .await;
}

#[tokio::test]
async fn runs_one_rounds_tool_calls_in_order() {
    let mut s = Session::start(Script::new().responses([
        calls([
            (
                "bash",
                json!({"command": "sleep 0.3; echo a >> order"}),
            ),
            ("bash", json!({"command": "echo b >> order"})),
        ]),
        words("Both ran."),
    ]))
    .await;
    let dir = folder(&s);
    s.chief.say(mayor(), &format!("/folder {dir}")).await;
    s.chief.say(mayor(), "/mode auto").await;
    s.chief.say(mayor(), "Run both.").await;
    s.chief.idle("home", "founder").await;
    let order =
        std::fs::read_to_string(format!("{dir}/order")).unwrap();
    assert_eq!(order, "a\nb\n");
}
