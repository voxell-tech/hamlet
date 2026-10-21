//! The mayor alone: chat, turns, tools, approvals, faults, stops,
//! rewinds, sessions and paging.

#[path = "mayor/asking.rs"]
mod asking;

use std::time::Duration;

use hamlet_acceptance::*;
use hamlet_protocol::*;
use serde_json::json;

/// An approval's words after its agent's name.
fn says(wait: &Wait) -> &str {
    match &wait.detail {
        WaitDetail::Approval { says, .. } => says,
        _ => "",
    }
}

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
        Some(LastActivity {
            what: "replied".into(),
            at: last.at
        })
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
                w.detail,
                WaitDetail::Approval {
                    danger: WaitDanger::Command,
                    ..
                }
            )
        })
        .await;
    let Wait {
        answers,
        asked,
        about,
        agent,
        ..
    } = &command;
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
    assert_eq!(says(&command), "wants to run");
    assert!(!std::path::Path::new(&dir).join("result.txt").exists());
    let reply =
        s.chief.choose(&command.id, Choice::Allow, None).await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    let edit = s
        .chief
        .wait_for("an edit approval", |w| {
            matches!(
                w.detail,
                WaitDetail::Approval {
                    danger: WaitDanger::Edit,
                    ..
                }
            )
        })
        .await;
    let Wait {
        change: Some(change),
        ..
    } = &edit
    else {
        panic!("an edit approval carries its change: {edit:?}")
    };
    assert_eq!(says(&edit), "wants to edit");
    assert_eq!(
        std::fs::read_to_string(format!("{dir}/result.txt")).unwrap(),
        "heading\napproved\n"
    );
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
    s.chief.choose(&edit.id, Choice::Allow, None).await;
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
            (command.id.clone(), "allow".to_string()),
            (edit.id.clone(), "allow".to_string())
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
        call("bash", json!({"command": "echo ran > never"})),
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
                w.detail,
                WaitDetail::Approval {
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
            "Answered /deny: mayor wants to run `echo ran > never`"
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
            matches!(w.detail, WaitDetail::Approval { .. })
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
                w.detail,
                WaitDetail::Approval {
                    danger: WaitDanger::Network,
                    ..
                }
            )
        })
        .await;
    assert_eq!(says(&wait), "wants the network for");
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
    let mut s = Session::start(Script::new().responses([
        failure("Faux model unavailable"),
        failure("Faux model unavailable"),
    ]))
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Try the model.").await;
    let wait = s
        .chief
        .wait_for("a fault", |w| {
            matches!(w.detail, WaitDetail::Fault { .. })
        })
        .await;
    let Wait { about, answers, .. } = &wait;
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
    let page = s.chief.open(&mayor_chat).await;
    assert_eq!(
        page.last.unwrap(),
        LastActivity {
            what: "error".into(),
            at: error.at
        }
    );
    s.chief.say(mayor(), "Try again.").await;
    let repeated = s
        .chief
        .wait_for("the repeated fault", |w| {
            w.asked == 2
                && matches!(w.detail, WaitDetail::Fault { .. })
        })
        .await;
    assert_eq!(&repeated.id, &wait.id);
    let reply = s.chief.choose(&wait.id, Choice::Dismiss, None).await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief
        .until("the fault closed", |c| c.waits().is_empty())
        .await;
    assert!(s.chief.changes().contains(&Update::WaitClosed {
        id: wait.id.clone(),
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
        .wait_for("a fault", |w| {
            matches!(w.detail, WaitDetail::Fault { .. })
        })
        .await;
    let Wait { about, .. } = &wait;
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
        .wait_for("a fault", |w| {
            matches!(w.detail, WaitDetail::Fault { .. })
        })
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
            matches!(w.detail, WaitDetail::Approval { .. })
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
    let mut s = Session::start(Script::new().responses([
        words("First answer."),
        words("Second answer."),
    ]))
    .await;
    let mayor_chat = chat(mayor());
    s.chief.say(mayor(), "First line.").await;
    let first = s.chief.open(&mayor_chat).await;
    assert_eq!(first.number, Some(1));
    let reply = s
        .chief
        .request(|id| ClientMessage::New(New { id, to: mayor() }))
        .await;
    assert_eq!(
        notice(&reply),
        (&ReplyKind::Notice, "Started a new session.")
    );
    s.chief.say(mayor(), "Second line.").await;
    let second = s.chief.open(&mayor_chat).await;
    assert_eq!(second.number, Some(2));
    assert_eq!(
        second
            .items
            .iter()
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>(),
        ["Started a new session.", "Second line.", "Second answer."]
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
    assert_eq!(
        notice(&reply),
        (&ReplyKind::Notice, "Went back to an earlier session.")
    );
    let resumed = s.chief.open(&mayor_chat).await;
    assert_eq!(resumed.number, first.number);
    assert_eq!(
        resumed
            .items
            .iter()
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>(),
        [
            "First line.",
            "First answer.",
            "Went back to an earlier session."
        ]
    );
    let history = s.chief.history(mayor()).await;
    assert_eq!(
        history
            .iter()
            .filter(|p| p.current)
            .map(|p| p.number)
            .collect::<Vec<_>>(),
        [1]
    );
    let past = s
        .chief
        .open(&ChatRef {
            past: Some(2),
            ..mayor_chat
        })
        .await;
    assert_eq!(past.items, second.items);
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
        call(
            "ask_chief",
            json!({"questions": [{"question": "Which grain?"}]}),
        ),
        words("Wheat it is."),
    ]))
    .await;
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.tell(mayor(), "Plant something.").await;
    let wait = s
        .chief
        .wait_for("a question", |w| {
            matches!(w.detail, WaitDetail::Question { .. })
        })
        .await;
    assert_eq!(
        wait,
        s.chief.waits()[0],
        "one question waits for the chief"
    );
    let Wait { about, answers, .. } = &wait;
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
                w.detail,
                WaitDetail::Approval {
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
    chief.choose(&wait.id, Choice::Allow, None).await;
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
    let running = home.file("houses/home/0/running");
    let server = home
        .serve(
            &Script::new().responses([call(
                "bash",
                json!({"command": "printf started > running; while :; do sleep 1; done"}),
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
            running.exists()
                && c.items(&mayor_chat)
                    .iter()
                    .any(|i| i.tool.is_some())
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
        call("search", json!({"pattern": "Notes"})),
        call("find", json!({"path": "."})),
        call(
            "write",
            json!({"path": "NOTES.md", "content": "remember"}),
        ),
        call("read", json!({"path": "skills/missing/SKILL.md"})),
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
    let activities = s
        .chief
        .changes()
        .into_iter()
        .filter_map(|u| match u {
            Update::Agent { agent } => Some(agent.activity),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        activities.iter().any(|a| a.failures == 1),
        "{activities:?}"
    );
    for (doing, step) in [
        (Doing::ReadNotes, 2),
        (Doing::ReadFile, 4),
        (Doing::ReadFile, 6),
        (Doing::WriteNotes, 8),
        (Doing::ReadSkill, 10),
    ] {
        assert!(
            activities.iter().any(|a| a.doing == doing
                && a.step == step
                && a.failures == 0),
            "{activities:?}"
        );
    }
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

#[tokio::test]
async fn faults_on_thinking_without_an_answer() {
    let mut s = Session::start(
        Script::new()
            .responses([words("").thinking("Still thinking.")]),
    )
    .await;
    s.chief.open(&chat(mayor())).await;
    s.chief.say(mayor(), "Answer.").await;
    let wait = s
        .chief
        .wait_for("an empty-answer fault", |w| {
            matches!(w.detail, WaitDetail::Fault { .. })
        })
        .await;
    let Wait { about, .. } = wait;
    assert_eq!(
        about,
        "the model answered with nothing. Send the message again"
    );
    assert!(
        s.chief
            .items(&chat(mayor()))
            .iter()
            .any(|i| i.kind == ItemKind::Reasoning
                && i.text == "Still thinking.")
    );
}

#[tokio::test]
async fn bounds_one_task_and_admits_the_next() {
    let mut s =
        Session::start(Script::new().price(1000.0).responses([
            call("read", json!({"path": "NOTES.md"})),
            words("A new task answers."),
        ]))
        .await;
    s.chief.limit("home", "budget", "cost", json!(null)).await;
    s.chief.limit("home", "task", "cost", json!(0.01)).await;
    s.chief.open(&chat(mayor())).await;
    s.chief.say(mayor(), "Read.").await;
    s.chief
        .until("a task cap notice", |c| {
            c.texts(&chat(mayor()))
                .iter()
                .any(|t| t.contains("This task has spent its cap"))
        })
        .await;
    s.chief.say(mayor(), "A new task.").await;
    s.chief.shows(&chat(mayor()), "A new task answers.").await;
    assert!(s.chief.waits().is_empty());
}

#[tokio::test]
async fn reads_a_lowered_cap_before_the_next_model_call() {
    let mut s = Session::start(
        Script::new().responses([
            call("read", json!({"path": "NOTES.md"}))
                .held_until("release"),
            words("After a raise."),
        ]),
    )
    .await;
    s.chief.open(&chat(mayor())).await;
    s.chief.tell(mayor(), "Read.").await;
    s.chief
        .until("a running turn", |c| c.working(&chat(mayor())))
        .await;
    assert_eq!(
        s.chief.limit("home", "budget", "cost", json!(0)).await.kind,
        ReplyKind::Notice
    );
    std::fs::write(s.home.side().join("release"), "").unwrap();
    s.chief
        .until("the lowered cap", |c| {
            c.texts(&chat(mayor()))
                .iter()
                .any(|t| t.contains("budget") && t.contains("$0.00"))
        })
        .await;
    s.chief.limit("home", "budget", "cost", json!(5)).await;
    s.chief.say(mayor(), "Now answer.").await;
    s.chief.shows(&chat(mayor()), "After a raise.").await;
}

#[tokio::test]
async fn reflects_owed_work_when_starting_a_new_chat() {
    let mut s = Session::start_with(
        Script::new().responses(
            [words("Remembered.")].into_iter().chain(reflected()),
        ),
        &[("HAMLET_REFLECT_EVERY", "10")],
    )
    .await;
    s.chief.say(mayor(), "Remember.").await;
    s.chief.say(mayor(), "/new").await;
    let file = s.home.file("houses/home/0/NOTES.md");
    s.chief
        .until("the retiring chat's notes", |_| {
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("Reflected.")
        })
        .await;
    let past = s
        .chief
        .open(&ChatRef {
            past: Some(1),
            ..chat(mayor())
        })
        .await;
    assert!(
        past.items.iter().any(|i| i
            .tool
            .as_ref()
            .is_some_and(|t| t.name == "write"))
    );
    assert_eq!(s.chief.open(&chat(mayor())).await.number, Some(2));
}

#[tokio::test]
async fn reflects_owed_work_at_an_orderly_stop() {
    let mut s = Session::start_with(
        Script::new().responses(
            [words("Remembered.")].into_iter().chain(reflected()),
        ),
        &[("HAMLET_REFLECT_EVERY", "10")],
    )
    .await;
    s.chief.say(mayor(), "Remember.").await;
    s.chief.idle("home", "founder").await;
    assert_eq!(s.server.stop().await, Some(0));
    assert_eq!(notes_from(&s.home), "# Notes\nReflected.\n");
    let server = s.home.serve(&Script::new(), QUIET).await;
    let mut chief = Client::join(&server).await;
    let page = chief.open(&chat(mayor())).await;
    assert_eq!(
        page.items.iter().filter(|i| i.text == "Noted.").count(),
        1
    );
}

fn notes_from(home: &Home) -> String {
    std::fs::read_to_string(home.file("houses/home/0/NOTES.md"))
        .unwrap()
}

#[tokio::test]
async fn refuses_reflection_approvals_during_shutdown() {
    let mut s = Session::start_with(
        Script::new().responses([
            words("Remembered."),
            call("bash", json!({"command": "touch never"})),
            words("Nothing ran."),
        ]),
        &[("HAMLET_REFLECT_EVERY", "10")],
    )
    .await;
    let dir = manual(&mut s).await;
    s.chief.say(mayor(), "Remember.").await;
    s.chief.idle("home", "founder").await;
    assert_eq!(s.server.stop().await, Some(0));
    assert!(!std::path::Path::new(&dir).join("never").exists());
    let server = s.home.serve(&Script::new(), QUIET).await;
    let mut chief = Client::join(&server).await;
    assert!(chief.waits().is_empty());
    let page = chief.open(&chat(mayor())).await;
    assert!(
        page.items
            .iter()
            .filter_map(|i| i.tool.as_ref()?.failure.as_ref())
            .any(|f| f.contains("nobody is here to approve it")),
        "{page:?}"
    );
}

#[tokio::test]
async fn rewinds_a_reflected_turn_without_undoing_files_or_spend() {
    let mut s = Session::start_with(
        Script::new().price(1.0).responses(
            [words("Remembered.")].into_iter().chain(reflected()),
        ),
        &[("HAMLET_REFLECT_EVERY", "1")],
    )
    .await;
    s.chief.open(&chat(mayor())).await;
    s.chief.say(mayor(), "Remember.").await;
    s.chief.shows(&chat(mayor()), "Noted.").await;
    s.chief.idle("home", "founder").await;
    let before =
        s.join().await.agent("home", "founder").unwrap().spent;
    let page = s.chief.open(&chat(mayor())).await;
    let target = page
        .items
        .iter()
        .find(|i| i.kind == ItemKind::User)
        .unwrap()
        .id
        .clone();
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Rewind(Rewind {
                id,
                chat: chat(mayor()),
                item: target,
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Rewound);
    assert_eq!(notes(&s), "# Notes\nReflected.\n");
    let page = s.chief.open(&chat(mayor())).await;
    assert!(page.items.is_empty(), "{page:?}");
    assert_eq!(
        s.join().await.agent("home", "founder").unwrap().spent,
        before
    );
    assert!(
        s.chief
            .say(mayor(), "/retry")
            .await
            .text
            .contains("nothing")
    );
    assert_eq!(s.server.stop().await, Some(0));
    let server = s.home.serve(&Script::new(), QUIET).await;
    let mut chief = Client::join(&server).await;
    assert!(chief.open(&chat(mayor())).await.items.is_empty());
    assert_eq!(chief.agent("home", "founder").unwrap().spent, before);
}

#[tokio::test]
async fn keeps_edit_trust_and_network_refusal_across_restart_but_not_resume()
 {
    let mut s = Session::start(Script::new().responses([
        call("write", json!({"path":"a.txt", "content":"one"})),
        call("bash", json!({"command":"echo curl > network"})),
        call("bash", json!({"command":"echo command > ran"})),
    ]))
    .await;
    let dir = manual(&mut s).await;
    s.chief.tell(mayor(), "Work.").await;
    let edit = s.chief.wait_for("edit", |_| true).await;
    s.chief.choose(&edit.id, Choice::Always, None).await;
    let net = s
        .chief
        .wait_for("network", |w| {
            matches!(
                w.detail,
                WaitDetail::Approval {
                    danger: WaitDanger::Network,
                    ..
                }
            )
        })
        .await;
    s.chief
        .choose(&net.id, Choice::Deny, Some("no network"))
        .await;
    let command = s
        .chief
        .wait_for("command", |w| {
            matches!(
                w.detail,
                WaitDetail::Approval {
                    danger: WaitDanger::Command,
                    ..
                }
            )
        })
        .await;
    s.server.kill().await;
    let server = s
        .home
        .serve(
            &Script::new().responses([
                call(
                    "write",
                    json!({"path":"b.txt", "content":"two"}),
                ),
                call(
                    "bash",
                    json!({"command":"echo wget > network"}),
                ),
                words("Finished."),
                call(
                    "write",
                    json!({"path":"c.txt", "content":"three"}),
                ),
                words("Refused."),
            ]),
            QUIET,
        )
        .await;
    let mut chief = Client::join(&server).await;
    assert_eq!(chief.waits(), std::slice::from_ref(&command));
    chief.open(&chat(mayor())).await;
    chief.choose(&command.id, Choice::Allow, None).await;
    chief.shows(&chat(mayor()), "Finished.").await;
    chief.idle("home", "founder").await;
    assert!(chief.waits().is_empty());
    let dir = std::path::Path::new(&dir);
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "one"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("b.txt")).unwrap(),
        "two"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("ran")).unwrap(),
        "command\n"
    );
    assert!(!dir.join("network").exists());
    chief.say(mayor(), "/new").await;
    chief.say(mayor(), "/resume 1").await;
    chief.tell(mayor(), "Write again.").await;
    let edit = chief
        .wait_for("edit after resume", |w| {
            matches!(
                w.detail,
                WaitDetail::Approval {
                    danger: WaitDanger::Edit,
                    ..
                }
            )
        })
        .await;
    chief.choose(&edit.id, Choice::Deny, None).await;
    chief.shows(&chat(mayor()), "Refused.").await;
    assert!(!dir.join("c.txt").exists());
}

#[tokio::test]
async fn stopping_drops_queued_lines_and_leaves_the_next_line_alone()
{
    let mut s = Session::start(
        Script::new().responses([
            call("bash", json!({"command":"echo never > forbidden"}))
                .held_until("release"),
            words("Next answer."),
        ]),
    )
    .await;
    let dir = manual(&mut s).await;
    s.chief.open(&chat(mayor())).await;
    s.chief.tell(mayor(), "Held.").await;
    s.chief
        .until("the running turn", |c| c.working(&chat(mayor())))
        .await;
    s.chief.tell(mayor(), "Queued.").await;
    s.chief.until("the queued line", |c| c.messages().iter().any(|m| matches!(m, ServerMessage::Turn(t) if t.ops.iter().any(|o| matches!(o, Op::Status { pending, .. } if *pending == 1))))).await;
    std::fs::write(s.home.side().join("release"), "").unwrap();
    let wait = s.chief.wait_for("command", |_| true).await;
    let reply = s.chief.say(mayor(), "/stop mayor").await;
    assert_eq!(reply.text, "Stopping mayor.");
    s.chief
        .until("the dropped line notice", |c| {
            c.items(&chat(mayor()))
                .iter()
                .any(|i| i.text.contains("1 queued dropped"))
        })
        .await;
    s.chief
        .until("the wait lapsed", |c| c.waits().is_empty())
        .await;
    assert!(s.chief.changes().contains(&Update::WaitClosed {
        id: wait.id.clone(),
        how: "stopped".into()
    }));
    s.chief.say(mayor(), "Next.").await;
    s.chief.shows(&chat(mayor()), "Next answer.").await;
    assert!(!std::path::Path::new(&dir).join("forbidden").exists());
    let page = s.chief.open(&chat(mayor())).await;
    assert!(!page.items.iter().any(|i| i.text == "Queued."));
}
