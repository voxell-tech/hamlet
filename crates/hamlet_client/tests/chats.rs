use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_client::chats::{Chat, Entry, Item, Transcript};
use hamlet_client::messages::Reconnected;
use hamlet_client::outbox::Outbox;
use hamlet_client::{ClientPlugin, receive};
use hamlet_protocol::client as protocol;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None });
    app
}

fn reference() -> protocol::ChatRef {
    protocol::ChatRef {
        kind: None,
        from: None,
        with: protocol::Address {
            hamlet: "home".into(),
            agent: "mayor".into(),
        },
        past: None,
    }
}

fn requests(app: &mut App) -> Vec<protocol::ClientMessage> {
    app.world_mut().resource_mut::<Outbox>().drain().collect()
}

#[test]
fn a_chat_is_watched_until_removed_and_reopens_without_losing_its_items()
 {
    let mut app = app();
    let chat = app.world_mut().spawn(Chat(reference())).id();
    app.update();
    let sent = requests(&mut app);
    assert!(
        matches!(&sent[..], [protocol::ClientMessage::Open(open)] if open.chat == reference() && open.before.is_none())
    );
    app.world_mut()
        .entity_mut(chat)
        .insert(Transcript::from_items(vec![Item {
            id: "kept:0".into(),
            at: 1000,
            entry: Entry::Assistant("kept while reconnecting".into()),
        }]));
    app.world_mut().trigger(Reconnected);
    app.update();
    let sent = requests(&mut app);
    assert!(
        matches!(&sent[..], [protocol::ClientMessage::Open(open)] if open.chat == reference() && open.before.is_none())
    );
    assert_eq!(
        app.world().get::<Transcript>(chat).unwrap().items().len(),
        1
    );
    app.world_mut().entity_mut(chat).remove::<Chat>();
    app.update();
    assert!(
        matches!(&requests(&mut app)[..], [protocol::ClientMessage::Close(close)] if close.chat == reference())
    );
    app.world_mut().entity_mut(chat).insert(Chat(reference()));
    app.update();
    requests(&mut app);
    app.world_mut().despawn(chat);
    app.update();
    assert!(
        matches!(&requests(&mut app)[..], [protocol::ClientMessage::Close(close)] if close.chat == reference())
    );
}

fn frames(text: &str) -> Vec<protocol::ServerMessage> {
    text.lines()
        .filter_map(|line| {
            let frame =
                serde_json::from_str::<serde_json::Value>(line)
                    .unwrap();
            (frame["from"] == "server").then(|| {
                serde_json::from_value(frame["message"].clone())
                    .unwrap()
            })
        })
        .collect()
}

#[derive(Resource, Default)]
struct Observed {
    output: Vec<(Entity, protocol::TranscriptOperation)>,
    finished: Vec<Entity>,
}

fn observe(app: &mut App) {
    use hamlet_client::messages::{JobFinished, SessionOutput};
    app.init_resource::<Observed>()
        .add_observer(
            |event: On<SessionOutput>, mut seen: ResMut<Observed>| {
                seen.output
                    .push((event.chat, event.operation.clone()));
            },
        )
        .add_observer(
            |event: On<JobFinished>, mut seen: ResMut<Observed>| {
                seen.finished.push(event.chat);
            },
        );
}

#[test]
fn live_turns_equal_the_transcript_read_back_and_emit_each_operation_once()
 {
    use hamlet_client::chats::{Status, Usage};
    let mut app = app();
    observe(&mut app);
    let chat = app.world_mut().spawn(Chat(reference())).id();
    app.update();
    let mut recorded = frames(include_str!(
        "../../hamlet_protocol/frames/turn.jsonl"
    ));
    let read_back = recorded.pop().unwrap();
    let mut expected_ops = Vec::new();
    let mut saw_working = false;
    for frame in recorded {
        let before = app
            .world()
            .get::<Transcript>(chat)
            .map_or(0, Transcript::shape);
        let appended = matches!(&frame, protocol::ServerMessage::Turn(turn) if turn.operations.iter().all(|operation| matches!(operation, protocol::TranscriptOperation::Append {..})));
        if let protocol::ServerMessage::Turn(turn) = &frame {
            expected_ops.extend(
                turn.operations
                    .iter()
                    .cloned()
                    .map(|operation| (chat, operation)),
            );
        }
        receive(app.world_mut(), frame);
        if appended {
            assert_eq!(
                app.world().get::<Transcript>(chat).unwrap().shape(),
                before
            );
        }
        if let Some(status) = app.world().get::<Status>(chat) {
            saw_working |= status.is_working();
        }
    }
    assert!(saw_working);
    let world = app.world();
    let said = world.get::<Transcript>(chat).unwrap();
    assert_eq!(said.items().len(), 4);
    assert!(
        matches!(&said.items()[0].entry, Entry::Chief { name, text } if name == "chief" && text == "Hello, mayor.")
    );
    assert!(
        matches!(&said.items()[1].entry, Entry::Reasoning(text) if text == "I will greet the chief.")
    );
    assert!(
        matches!(&said.items()[2].entry, Entry::Tool { name, exit: Some(0), ms: Some(_), .. } if name == "bash")
    );
    assert!(
        matches!(&said.items()[3].entry, Entry::Assistant(text) if text == "Hello chief. The mayor is here and ready to work with you. ".repeat(6).trim_end())
    );
    assert!(!said.is_answering());
    assert!(!world.get::<Status>(chat).unwrap().is_working());
    let protocol::ServerMessage::Transcript(page) = &read_back else {
        panic!("the recording ends with a read-back transcript");
    };
    assert_eq!(
        world.get::<Usage>(chat).unwrap().summary(),
        page.usage.summary
    );
    assert_eq!(world.resource::<Observed>().output, expected_ops);
    assert_eq!(world.resource::<Observed>().finished, vec![chat]);
    let live = said.items().to_vec();
    receive(app.world_mut(), read_back);
    assert_eq!(
        app.world().get::<Transcript>(chat).unwrap().items(),
        live
    );
    assert_eq!(
        app.world().resource::<Observed>().finished,
        vec![chat]
    );
}

#[test]
fn earlier_requests_one_page_and_prepends_it_to_the_received_transcript()
 {
    use hamlet_client::chats::earlier;
    let mut app = app();
    let chat = app.world_mut().spawn(Chat(reference())).id();
    app.update();
    requests(&mut app);
    earlier(app.world_mut(), chat);
    assert!(requests(&mut app).is_empty());
    let pages = frames(include_str!(
        "../../hamlet_protocol/frames/paging.jsonl"
    ))
    .into_iter()
    .filter_map(|message| {
        if let protocol::ServerMessage::Transcript(page) = message {
            Some(page)
        } else {
            None
        }
    })
    .collect::<Vec<_>>();
    receive(
        app.world_mut(),
        protocol::ServerMessage::Transcript(pages[0].clone()),
    );
    let later = app
        .world()
        .get::<Transcript>(chat)
        .unwrap()
        .items()
        .to_vec();
    assert_eq!(later.len(), 40);
    assert!(pages[0].earlier.is_some());
    assert_eq!(
        app.world().get::<Transcript>(chat).unwrap().earlier(),
        pages[0].earlier.as_deref()
    );
    earlier(app.world_mut(), chat);
    earlier(app.world_mut(), chat);
    let sent = requests(&mut app);
    let [protocol::ClientMessage::Open(open)] = &sent[..] else {
        panic!("one page request: {sent:?}");
    };
    assert_eq!(open.chat, reference());
    assert_eq!(open.before.as_deref(), Some(later[0].id.as_str()));
    let mut page = pages[1].clone();
    page.to = Some(open.id.clone());
    receive(
        app.world_mut(),
        protocol::ServerMessage::Transcript(page),
    );
    let said = app.world().get::<Transcript>(chat).unwrap();
    assert_eq!(said.items().len(), 46);
    assert_eq!(&said.items()[6..], &later);
    assert!(
        matches!(&said.items()[0].entry, Entry::Chief { text, .. } if text == "Line 1.")
    );
    assert_eq!(said.earlier(), None);
    earlier(app.world_mut(), chat);
    assert!(requests(&mut app).is_empty());
}

#[test]
fn chats_follow_the_addressed_agent_and_the_current_mayor_as_the_realm_changes()
 {
    use hamlet_client::chats::{Conversations, TalksTo};
    use hamlet_client::realm::mayor;
    use hamlet_testing::realm::{agent, state};
    let mut app = app();
    let office = app.world_mut().spawn(Chat(reference())).id();
    let mut named_ref = reference();
    named_ref.with.agent = "founder".into();
    let named = app.world_mut().spawn(Chat(named_ref)).id();
    app.update();
    assert!(app.world().get::<TalksTo>(office).is_none());
    let mut realm = state();
    receive(
        app.world_mut(),
        protocol::ServerMessage::State(realm.clone()),
    );
    app.update();
    let founder = mayor(app.world_mut()).unwrap();
    assert_eq!(
        app.world().get::<TalksTo>(office).unwrap().0,
        founder
    );
    assert_eq!(app.world().get::<TalksTo>(named).unwrap().0, founder);
    assert_eq!(
        app.world()
            .get::<Conversations>(founder)
            .unwrap()
            .iter()
            .count(),
        2
    );
    realm.agents[0].mayor = false;
    let mut other = agent("home", 1, "writer");
    other.mayor = true;
    realm.agents.push(other);
    realm.revision += 1;
    receive(
        app.world_mut(),
        protocol::ServerMessage::State(realm.clone()),
    );
    app.update();
    let writer = mayor(app.world_mut()).unwrap();
    assert_ne!(founder, writer);
    assert_eq!(app.world().get::<TalksTo>(office).unwrap().0, writer);
    assert_eq!(app.world().get::<TalksTo>(named).unwrap().0, founder);
    realm.agents.clear();
    realm.revision += 1;
    receive(app.world_mut(), protocol::ServerMessage::State(realm));
    app.update();
    assert!(app.world().get::<TalksTo>(office).is_none());
    assert!(app.world().get::<TalksTo>(named).is_none());
    assert!(app.world().get::<Chat>(office).is_some());
}

#[derive(Resource, Default)]
struct RemovedWords(Vec<(Entity, String)>);

#[test]
fn a_recorded_rewind_returns_the_chief_words_and_replaces_the_reset_items()
 {
    use hamlet_client::messages::{Rewind, Rewound};
    let mut app = app();
    app.init_resource::<RemovedWords>().add_observer(
        |event: On<Rewound>, mut seen: ResMut<RemovedWords>| {
            seen.0.push((event.chat, event.text.clone()));
        },
    );
    let chat = app.world_mut().spawn(Chat(reference())).id();
    app.update();
    requests(&mut app);
    let mut before = Vec::new();
    let mut pending = None::<protocol::Rewind>;
    for line in
        include_str!("../../hamlet_protocol/frames/rewind.jsonl")
            .lines()
    {
        let frame =
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        if frame["from"] == "client" {
            if let protocol::ClientMessage::Rewind(recorded) =
                serde_json::from_value::<protocol::ClientMessage>(
                    frame["message"].clone(),
                )
                .unwrap()
            {
                before = app
                    .world()
                    .get::<Transcript>(chat)
                    .unwrap()
                    .items()
                    .to_vec();
                assert_eq!(before.len(), 4);
                assert!(
                    before
                        .iter()
                        .any(|item| item.id == recorded.item)
                );
                app.world_mut().trigger(Rewind {
                    chat,
                    item: recorded.item.clone(),
                });
                let sent = requests(&mut app);
                let [protocol::ClientMessage::Rewind(rewind)] =
                    &sent[..]
                else {
                    panic!("one rewind request: {sent:?}");
                };
                assert_eq!(rewind.item, recorded.item);
                pending = Some(rewind.clone());
            }
            continue;
        }
        let mut message = serde_json::from_value::<
            protocol::ServerMessage,
        >(frame["message"].clone())
        .unwrap();
        if let protocol::ServerMessage::Reply(reply) = &mut message
            && reply.kind == protocol::ReplyKind::Rewound
        {
            reply.to = pending
                .as_ref()
                .expect("the rewind request precedes its reply")
                .id
                .clone();
        }
        let reset = matches!(&message, protocol::ServerMessage::Turn(turn) if turn.operations.iter().any(|operation| matches!(operation, protocol::TranscriptOperation::Reset {})));
        receive(app.world_mut(), message);
        if reset {
            assert!(
                app.world()
                    .get::<Transcript>(chat)
                    .unwrap()
                    .items()
                    .is_empty()
            );
        }
    }
    let said = app.world().get::<Transcript>(chat).unwrap();
    assert_eq!(said.items().len(), 2);
    for item in said.items() {
        assert_eq!(
            Some(item),
            before.iter().find(|old| old.id == item.id)
        );
    }
    assert!(said.item(&pending.unwrap().item).is_none());
    assert_eq!(
        app.world().resource::<RemovedWords>().0,
        vec![(chat, "Take back this line.".into())]
    );
}

#[test]
fn tool_results_keep_the_exit_time_failure_and_the_approved_change() {
    for (recording, summary, code, failure) in [
        (
            include_str!("../../hamlet_protocol/frames/tool.jsonl"),
            "exit 7",
            7,
            Some("broken"),
        ),
        (
            include_str!(
                "../../hamlet_protocol/frames/approval.jsonl"
            ),
            "reviewed",
            0,
            None,
        ),
    ] {
        let recorded = frames(recording);
        let id = recorded
            .iter()
            .filter_map(|message| match message {
                protocol::ServerMessage::Turn(turn) => {
                    Some(&turn.operations)
                }
                _ => None,
            })
            .flatten()
            .find_map(|operation| match operation {
                protocol::TranscriptOperation::Add {
                    item, ..
                } if item.tool.as_ref().is_some_and(|tool| {
                    tool.summary.contains(summary)
                }) =>
                {
                    Some(item.id.clone())
                }
                _ => None,
            })
            .expect("the recorded tool to inspect");
        let ms = recorded
            .iter()
            .filter_map(|message| match message {
                protocol::ServerMessage::Turn(turn) => {
                    Some(&turn.operations)
                }
                _ => None,
            })
            .flatten()
            .find_map(|operation| match operation {
                protocol::TranscriptOperation::Close {
                    item,
                    ms,
                    ..
                } if item == &id => *ms,
                _ => None,
            })
            .expect("the recorded tool close carries its duration");
        let mut app = app();
        let chat = app.world_mut().spawn(Chat(reference())).id();
        for message in recorded {
            receive(app.world_mut(), message);
        }
        let item = app
            .world()
            .get::<Transcript>(chat)
            .unwrap()
            .item(&id)
            .unwrap();
        let Entry::Tool {
            exit,
            ms: time,
            failure: why,
            change,
            ..
        } = &item.entry
        else {
            panic!("tool entry");
        };
        assert_eq!(*exit, Some(code));
        assert_eq!(*time, Some(ms));
        assert_eq!(why.as_deref(), failure);
        if code == 0 {
            let change = change.as_ref().unwrap();
            assert_eq!(change.path, "result.txt");
            assert_eq!(change.count, "1 line added, 1 removed");
            assert_eq!(
                change
                    .lines
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<Vec<_>>(),
                ["heading", "approved", "reviewed"]
            );
        }
    }
}

fn turn(
    operations: Vec<protocol::TranscriptOperation>,
) -> protocol::ServerMessage {
    protocol::ServerMessage::Turn(protocol::Turn {
        chat: reference(),
        number: Some(1),
        operations,
    })
}

#[test]
fn working_and_last_activity_follow_status_and_finishing_is_not_repeated()
 {
    use hamlet_client::chats::Status;
    let mut app = app();
    observe(&mut app);
    let chat = app.world_mut().spawn(Chat(reference())).id();
    receive(
        app.world_mut(),
        turn(vec![protocol::TranscriptOperation::Status {
            working: true,
            pending: 2,
            last: None,
        }]),
    );
    assert_eq!(app.world().get::<Status>(chat).unwrap().pending, 3);
    assert!(
        app.world().get::<Transcript>(chat).unwrap().is_answering()
    );
    let idle = turn(vec![protocol::TranscriptOperation::Status {
        working: false,
        pending: 0,
        last: Some(protocol::LastActivity {
            at: 1234,
            what: "Finished the chief's task".into(),
        }),
    }]);
    receive(app.world_mut(), idle.clone());
    receive(app.world_mut(), idle);
    let status = app.world().get::<Status>(chat).unwrap();
    assert!(!status.is_working());
    assert_eq!(
        status.last,
        Some((1234, "Finished the chief's task".into()))
    );
    assert!(
        !app.world().get::<Transcript>(chat).unwrap().is_answering()
    );
    assert_eq!(
        app.world().resource::<Observed>().finished,
        vec![chat]
    );
}

#[test]
fn new_and_resumed_conversations_keep_their_received_number() {
    use hamlet_client::chats::ConversationNumber;
    let mut app = app();
    let chat = app.world_mut().spawn(Chat(reference())).id();
    let mut numbers = Vec::new();
    for message in frames(include_str!(
        "../../hamlet_protocol/frames/new-and-resume.jsonl"
    )) {
        let transcript = matches!(
            &message,
            protocol::ServerMessage::Transcript(_)
        );
        receive(app.world_mut(), message);
        if transcript {
            numbers.push(
                app.world()
                    .get::<ConversationNumber>(chat)
                    .unwrap()
                    .0,
            );
        }
    }
    assert_eq!(numbers, [1, 2, 1]);
    let said = app.world().get::<Transcript>(chat).unwrap();
    assert_eq!(said.items().len(), 3);
    assert!(
        matches!(&said.items()[0].entry, Entry::Chief { text, .. } if text == "Remember our first conversation.")
    );
}

#[test]
fn a_refused_page_can_be_requested_again() {
    use hamlet_client::chats::earlier;
    let mut app = app();
    let chat = app
        .world_mut()
        .spawn((
            Chat(reference()),
            Transcript::from_items(vec![Item {
                id: "first:0".into(),
                at: 0,
                entry: Entry::Notice("A page".into()),
            }])
            .with_earlier(Some("first:0".into())),
        ))
        .id();
    app.update();
    requests(&mut app);
    for fault in [false, true] {
        earlier(app.world_mut(), chat);
        let sent = requests(&mut app);
        let [protocol::ClientMessage::Open(open)] = &sent[..] else {
            panic!("page request: {sent:?}");
        };
        let response = if fault {
            protocol::ServerMessage::Fault(protocol::Fault {
                to: Some(open.id.clone()),
                text: "No page".into(),
            })
        } else {
            protocol::ServerMessage::Reply(protocol::Reply {
                to: open.id.clone(),
                kind: protocol::ReplyKind::Error,
                text: "No page".into(),
                upload: None,
            })
        };
        receive(app.world_mut(), response);
    }
    earlier(app.world_mut(), chat);
    assert!(matches!(
        &requests(&mut app)[..],
        [protocol::ClientMessage::Open(_)]
    ));
}

#[test]
fn item_kinds_chief_fallback_and_structural_changes_survive_text_streaming()
 {
    use hamlet_testing::realm::state;
    let mut app = app();
    let chat = app.world_mut().spawn(Chat(reference())).id();
    receive(app.world_mut(), protocol::ServerMessage::State(state()));
    let item = |id: &str, kind: protocol::ItemKind, text: &str| {
        protocol::Item {
            id: id.into(),
            at: 42,
            kind,
            text: text.into(),
            chief: None,
            author: None,
            rewindable: None,
            tool: None,
        }
    };
    for (id, kind, text) in [
        ("chief", protocol::ItemKind::User, "Chief words"),
        ("reason", protocol::ItemKind::Reasoning, "A reason"),
        ("notice", protocol::ItemKind::Notice, "A notice"),
        ("error", protocol::ItemKind::Error, "An error"),
        ("answer", protocol::ItemKind::Answered, "> Which?\nRight"),
    ] {
        let before =
            app.world().get::<Transcript>(chat).unwrap().shape();
        receive(
            app.world_mut(),
            turn(vec![protocol::TranscriptOperation::Add {
                item: item(id, kind, text),
                replaces: None,
            }]),
        );
        assert!(
            app.world().get::<Transcript>(chat).unwrap().shape()
                > before
        );
    }
    let said = app.world().get::<Transcript>(chat).unwrap();
    assert!(
        matches!(&said.item("chief").unwrap().entry, Entry::Chief { name, text } if name == "chief" && text == "Chief words")
    );
    assert!(
        matches!(&said.item("notice").unwrap().entry, Entry::Notice(text) if text == "A notice")
    );
    assert!(
        matches!(&said.item("error").unwrap().entry, Entry::Error(text) if text == "An error")
    );
    assert!(
        matches!(&said.item("answer").unwrap().entry, Entry::Answered(text) if text == "> Which?\nRight")
    );
    let shape = said.shape();
    receive(
        app.world_mut(),
        turn(vec![protocol::TranscriptOperation::Append {
            item: "reason".into(),
            text: " grows".into(),
        }]),
    );
    let said = app.world().get::<Transcript>(chat).unwrap();
    assert_eq!(said.shape(), shape);
    assert!(
        matches!(&said.item("reason").unwrap().entry, Entry::Reasoning(text) if text == "A reason grows")
    );
    receive(
        app.world_mut(),
        turn(vec![protocol::TranscriptOperation::Add {
            item: item(
                "replacement",
                protocol::ItemKind::Reasoning,
                "Kept reason",
            ),
            replaces: Some("reason".into()),
        }]),
    );
    let said = app.world().get::<Transcript>(chat).unwrap();
    assert!(said.shape() > shape);
    assert!(said.item("reason").is_none());
    assert_eq!(said.items().last().unwrap().id, "replacement");
    assert_eq!(said.items().len(), 5);
    let mut tool = item("tool", protocol::ItemKind::Tool, "");
    tool.tool = Some(protocol::ItemTool {
        name: "write".into(), summary: "file".into(), exit: None, ms: None, failure: None,
        change: Some(serde_json::from_value(serde_json::json!({
            "path":"file", "created":true, "lines":[], "total":0, "added":0, "removed":0, "count":"empty"
        })).unwrap()),
    });
    receive(
        app.world_mut(),
        turn(vec![protocol::TranscriptOperation::Add {
            item: tool,
            replaces: None,
        }]),
    );
    for change in [None, Some(None)] {
        let shape =
            app.world().get::<Transcript>(chat).unwrap().shape();
        receive(
            app.world_mut(),
            turn(vec![protocol::TranscriptOperation::Close {
                item: "tool".into(),
                exit: 0,
                ms: Some(1),
                failure: None,
                change: change.clone(),
            }]),
        );
        let said = app.world().get::<Transcript>(chat).unwrap();
        assert!(said.shape() > shape);
        assert!(
            matches!(&said.item("tool").unwrap().entry, Entry::Tool { change: shown, .. } if shown.is_some() == change.is_none())
        );
    }
}

#[test]
fn past_and_current_chats_receive_only_their_own_frames() {
    let mut app = app();
    let current = app.world_mut().spawn(Chat(reference())).id();
    let mut past_ref = reference();
    past_ref.past = Some(1);
    let past = app.world_mut().spawn(Chat(past_ref.clone())).id();
    app.update();
    let sent = requests(&mut app);
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().any(|request| matches!(request, protocol::ClientMessage::Open(open) if open.chat == past_ref)));
    for message in frames(include_str!(
        "../../hamlet_protocol/frames/turn.jsonl"
    )) {
        receive(app.world_mut(), message);
    }
    assert_eq!(
        app.world()
            .get::<Transcript>(current)
            .unwrap()
            .items()
            .len(),
        4
    );
    assert!(
        app.world()
            .get::<Transcript>(past)
            .unwrap()
            .items()
            .is_empty()
    );
    let mut page = frames(include_str!(
        "../../hamlet_protocol/frames/turn.jsonl"
    ))
    .pop()
    .unwrap();
    let protocol::ServerMessage::Transcript(transcript) = &mut page
    else {
        panic!("transcript");
    };
    transcript.chat = past_ref;
    receive(app.world_mut(), page);
    assert_eq!(
        app.world().get::<Transcript>(past).unwrap().items(),
        app.world().get::<Transcript>(current).unwrap().items()
    );
    assert!(requests(&mut app).is_empty());
}

#[test]
fn another_transcript_keeps_the_page_outstanding_but_stale() {
    use hamlet_client::chats::earlier;

    for reset in [false, true] {
        let mut app = app();
        let chat = app.world_mut().spawn(Chat(reference())).id();
        app.update();
        requests(&mut app);
        let pages = frames(include_str!(
            "../../hamlet_protocol/frames/paging.jsonl"
        ))
        .into_iter()
        .filter_map(|message| match message {
            protocol::ServerMessage::Transcript(page) => Some(page),
            _ => None,
        })
        .collect::<Vec<_>>();
        receive(
            app.world_mut(),
            protocol::ServerMessage::Transcript(pages[0].clone()),
        );
        earlier(app.world_mut(), chat);
        let sent = requests(&mut app);
        let [protocol::ClientMessage::Open(open)] = &sent[..] else {
            panic!("one page request: {sent:?}");
        };
        if reset {
            receive(
                app.world_mut(),
                turn(vec![protocol::TranscriptOperation::Reset {}]),
            );
        }
        let mut replacement = pages[0].clone();
        replacement.to = (!reset).then(protocol::RequestId::new);
        replacement.items.truncate(2);
        replacement.items[1].id = "latest:0".into();
        replacement.earlier = Some("replacement-cursor".into());
        replacement.number = Some(2);
        replacement.working = true;
        replacement.pending = 3;
        replacement.usage.summary = "Current usage".into();
        receive(
            app.world_mut(),
            protocol::ServerMessage::Transcript(replacement.clone()),
        );
        earlier(app.world_mut(), chat);
        assert!(
            requests(&mut app).is_empty(),
            "the original page still waits"
        );
        let kept =
            app.world().get::<Transcript>(chat).unwrap().clone();
        let mut stale = pages[1].clone();
        stale.to = Some(open.id.clone());
        receive(
            app.world_mut(),
            protocol::ServerMessage::Transcript(stale),
        );
        let said = app.world().get::<Transcript>(chat).unwrap();
        assert_eq!(said.items(), kept.items());
        assert_eq!(said.shape(), kept.shape());
        assert_eq!(said.earlier(), Some("replacement-cursor"));
        assert!(said.is_answering());
        use hamlet_client::chats::{
            ConversationNumber, Status, Usage,
        };
        assert_eq!(
            app.world().get::<Status>(chat).unwrap().pending,
            4
        );
        assert_eq!(
            app.world().get::<Usage>(chat).unwrap().summary(),
            "Current usage"
        );
        assert_eq!(
            app.world().get::<ConversationNumber>(chat).unwrap().0,
            2
        );
        earlier(app.world_mut(), chat);
        let sent = requests(&mut app);
        let [protocol::ClientMessage::Open(next)] = &sent[..] else {
            panic!("a fresh page can be requested: {sent:?}");
        };
        assert_ne!(open.id, next.id);
        let mut fresh = pages[1].clone();
        fresh.to = Some(next.id.clone());
        receive(
            app.world_mut(),
            protocol::ServerMessage::Transcript(fresh),
        );
        let said = app.world().get::<Transcript>(chat).unwrap();
        assert_eq!(said.items().len(), 8);
        assert_eq!(said.items().last().unwrap().id, "latest:0");
    }
}

#[test]
fn peer_inputs_keep_their_author_and_never_become_chief_inputs() {
    let mut app = app();
    let chat = app.world_mut().spawn(Chat(reference())).id();
    app.update();
    let page = serde_json::json!({
        "type": "transcript", "chat": reference(), "items": [
            {"id":"1:0", "at":1, "kind":"user", "text":"Check this", "author":"scout@mill"},
            {"id":"2:0", "at":2, "kind":"user", "text":"My instruction", "chief":"chief"}
        ], "earlier": null, "last": null, "working":false, "pending":0,
        "usage":{"input":0,"output":0,"cached":0,"calls":0,"lastInput":0,"reasoning":0,"unreported":0,"summary":"", "micros":0,"ms":0}
    });
    receive(app.world_mut(), serde_json::from_value(page).unwrap());
    let items = app.world().get::<Transcript>(chat).unwrap().items();
    assert_eq!(
        items[0].entry,
        Entry::Agent {
            name: "scout@mill".into(),
            text: "Check this".into()
        }
    );
    assert!(
        matches!(&items[1].entry, Entry::Chief { name, .. } if name == "chief")
    );
}
