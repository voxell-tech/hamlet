use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_client::chats::Chat;
use hamlet_client::messages::*;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{HOME, MAYOR, mayor};
use hamlet_client::waits::{Choice, Waiting};
use hamlet_client::{ClientPlugin, receive};
use hamlet_protocol::v0;
use hamlet_testing::realm::{state, wait};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None });
    app
}

fn address(name: &str) -> v0::Address {
    v0::Address {
        hamlet: HOME.into(),
        agent: name.into(),
    }
}

#[test]
fn chief_messages_become_requests_with_fresh_uuid_ids_and_wire_addresses()
 {
    let mut app = app();
    let world = app.world_mut();
    let mut initial = state();
    initial.waits.push(wait("held"));
    receive(world, v0::ServerMessage::State(initial));
    let founder = mayor(world).unwrap();
    let wait =
        world.query::<(Entity, &Waiting)>().single(world).unwrap().0;
    let reference = v0::ChatRef {
        kind: None,
        from: None,
        with: address(MAYOR),
        past: Some(3),
    };
    let chat = world.spawn(Chat(reference.clone())).id();
    world.trigger(UserMessage {
        to: address(MAYOR),
        text: "hello".into(),
    });
    world.trigger(Answer {
        wait,
        choice: Choice::Allow,
        text: Some("go".into()),
    });
    world.trigger(Veto {
        agent: Some(founder),
    });
    world.trigger(Veto { agent: None });
    world.trigger(Rewind {
        chat,
        item: "entry:0".into(),
    });
    world.flush();
    let requests =
        world.resource_mut::<Outbox>().drain().collect::<Vec<_>>();
    let mut ids = std::collections::HashSet::new();
    for request in &requests {
        let value = serde_json::to_value(request).unwrap();
        let id = value["id"].as_str().unwrap();
        assert_eq!(
            uuid::Uuid::parse_str(id).unwrap().get_version_num(),
            4
        );
        assert!(ids.insert(id.to_string()));
    }
    assert!(
        matches!(&requests[0], v0::ClientMessage::Say(say) if say.to == address(MAYOR) && say.text == "hello")
    );
    assert!(
        matches!(&requests[1], v0::ClientMessage::Answer(answer) if answer.wait == "held" && answer.choice == Choice::Allow && answer.text.as_deref() == Some("go"))
    );
    assert!(
        matches!(&requests[2], v0::ClientMessage::Stop(stop) if stop.agent == Some(address("founder")))
    );
    assert!(
        matches!(&requests[3], v0::ClientMessage::Stop(stop) if stop.agent.is_none())
    );
    assert!(
        matches!(&requests[4], v0::ClientMessage::Rewind(rewind) if rewind.chat == reference && rewind.item == "entry:0")
    );
    assert_eq!(world.resource_mut::<Outbox>().drain().count(), 0);
}

#[derive(Resource, Default)]
struct Observed {
    responses: Vec<Response>,
    rewound: Vec<Rewound>,
    faults: Vec<ServerFault>,
}

#[test]
fn structured_questions_reconcile_and_send_picks_without_forging_chief_text()
 {
    use hamlet_client::waits::{Asks, Refused};
    use serde_json::json;

    let mut app = app();
    let world = app.world_mut();
    let mut initial = state();
    let mut question = serde_json::to_value(wait("ask")).unwrap();
    question["answers"] = json!(["reply", "dismiss"]);
    question["detail"] = json!({"kind": "question", "questions": {"message": "Choose", "questions": [{
        "question": "Which?", "description": "", "options": [
            {"title": "Left", "description": ""}, {"title": "Right", "description": ""}
        ], "text_input": false
    }]}});
    initial.waits = vec![serde_json::from_value(question).unwrap()];
    receive(world, v0::ServerMessage::State(initial.clone()));
    let entity =
        world.query::<(Entity, &Asks)>().single(world).unwrap().0;
    // The server, not the window, judges an answer.
    world.trigger(Answered {
        entity,
        picks: vec![v0::Pick::Words {
            text: "Left".into(),
        }],
    });
    world.flush();
    let sent =
        world.resource_mut::<Outbox>().drain().collect::<Vec<_>>();
    let [v0::ClientMessage::Answer(refused)] = sent.as_slice() else {
        panic!("the words are sent")
    };
    receive(
        world,
        v0::ServerMessage::Reply(v0::Reply {
            upload: None,
            to: refused.id.clone(),
            kind: v0::ReplyKind::Error,
            text: "question 1 takes no words".into(),
        }),
    );
    assert_eq!(
        world.get::<Refused>(entity),
        Some(&Refused("question 1 takes no words".into()))
    );
    world.trigger(Answered {
        entity,
        picks: vec![v0::Pick::Offer { index: 1 }],
    });
    world.flush();
    let requests =
        world.resource_mut::<Outbox>().drain().collect::<Vec<_>>();
    let [v0::ClientMessage::Answer(answer)] = requests.as_slice()
    else {
        panic!("one typed answer")
    };
    assert_eq!(answer.wait, "ask");
    assert_eq!(answer.text, None);
    assert_eq!(
        answer.picks,
        Some(vec![v0::Pick::Offer { index: 1 }])
    );
    receive(
        world,
        v0::ServerMessage::Reply(v0::Reply {
            upload: None,
            to: answer.id.clone(),
            kind: v0::ReplyKind::Notice,
            text: "answered".into(),
        }),
    );
    assert!(world.get::<Refused>(entity).is_none());
    initial.waits.clear();
    receive(world, v0::ServerMessage::State(initial));
    assert!(world.get_entity(entity).is_err());
}

#[test]
fn addressed_answers_reach_observers_and_a_rewind_returns_its_removed_words()
 {
    let mut app = app();
    app.init_resource::<Observed>()
        .add_observer(
            |event: On<Response>, mut seen: ResMut<Observed>| {
                seen.responses.push(event.event().clone());
            },
        )
        .add_observer(
            |event: On<Rewound>, mut seen: ResMut<Observed>| {
                seen.rewound.push(event.event().clone());
            },
        )
        .add_observer(
            |event: On<ServerFault>, mut seen: ResMut<Observed>| {
                seen.faults.push(event.event().clone());
            },
        );
    let world = app.world_mut();
    let id = world
        .resource_mut::<Outbox>()
        .send(|id| v0::ClientMessage::Read(v0::Read::Charter { id }));
    let reply = v0::ServerMessage::Reply(v0::Reply {
        upload: None,
        to: id.clone(),
        kind: v0::ReplyKind::Notice,
        text: "read".into(),
    });
    receive(world, reply.clone());
    assert_eq!(world.resource::<Observed>().responses[0].to, id);
    assert_eq!(
        world.resource::<Observed>().responses[0].message,
        reply
    );
    let chat = world
        .spawn(Chat(v0::ChatRef {
            kind: None,
            from: None,
            with: address(MAYOR),
            past: None,
        }))
        .id();
    world.trigger(Rewind {
        chat,
        item: "entry:0".into(),
    });
    world.flush();
    let requests =
        world.resource_mut::<Outbox>().drain().collect::<Vec<_>>();
    let v0::ClientMessage::Rewind(rewind) = &requests[1] else {
        panic!("rewind request");
    };
    receive(
        world,
        v0::ServerMessage::Reply(v0::Reply {
            upload: None,
            to: rewind.id.clone(),
            kind: v0::ReplyKind::Rewound,
            text: "removed words".into(),
        }),
    );
    let seen = world.resource::<Observed>();
    assert_eq!(seen.rewound.len(), 1);
    assert_eq!(seen.rewound[0].chat, chat);
    assert_eq!(seen.rewound[0].text, "removed words");
    assert_eq!(seen.responses.len(), 2);
    for value in [
        serde_json::json!({"type":"read", "to":id, "prompt":"prompt"}),
        serde_json::json!({"type":"login", "to":id, "login":"flow", "step":{"done":"done"}}),
        serde_json::json!({"type":"fault", "to":id, "text":"addressed failure"}),
        serde_json::json!({"type":"transcript", "to":id, "chat":{"with":{"hamlet":"home","agent":"mayor"}}, "items":[], "earlier":null, "last":null, "pending":0,"working":false, "usage":{"calls":0,"unreported":0,"input":0,"cached":0,"output":0,"reasoning":0,"ms":0,"micros":0,"lastInput":0,"summary":"No model calls yet"}}),
    ] {
        receive(world, serde_json::from_value(value).unwrap());
    }
    receive(
        world,
        v0::ServerMessage::Fault(v0::Fault {
            to: None,
            text: "unaddressed failure".into(),
        }),
    );
    let seen = world.resource::<Observed>();
    assert_eq!(seen.responses.len(), 6);
    assert_eq!(seen.faults.len(), 1);
    assert_eq!(seen.faults[0].text, "unaddressed failure");
}

#[test]
fn chats_expose_string_item_ids_server_usage_and_agent_relations() {
    use hamlet_client::chats::{
        Conversations, Entry, Item, Status, TalksTo, Transcript,
        Usage,
    };
    let mut app = app();
    let world = app.world_mut();
    receive(world, v0::ServerMessage::State(state()));
    let founder = mayor(world).unwrap();
    let transcript = Transcript::from_items(vec![
        Item {
            id: "user:0".into(),
            at: 1000,
            entry: Entry::User {
                user: "chief".into(),
                text: "hello".into(),
            },
        },
        Item {
            id: "tool:0".into(),
            at: 1001,
            entry: Entry::Tool {
                name: "bash".into(),
                summary: "pwd".into(),
                exit: Some(7),
                ms: Some(45),
                failure: Some("failed".into()),
                change: None,
            },
        },
    ])
    .with_earlier(Some("before".into()))
    .with_answering(true);
    assert_eq!(transcript.items().len(), 2);
    assert_eq!(transcript.item("user:0").unwrap().at, 1000);
    assert!(transcript.item("missing").is_none());
    assert_eq!(transcript.shape(), 2);
    assert!(transcript.answering());
    assert_eq!(transcript.earlier(), Some("before"));
    let chat = world
        .spawn((
            Chat(v0::ChatRef {
                kind: None,
                from: None,
                with: address(MAYOR),
                past: None,
            }),
            transcript,
            TalksTo(founder),
        ))
        .id();
    assert!(
        world
            .get::<Conversations>(founder)
            .unwrap()
            .iter()
            .any(|entity| entity == chat)
    );
    let status = Status {
        pending: 1,
        last: Some((1000, "working".into())),
    };
    assert!(status.working());
    assert!(!Status::default().working());
    let usage = Usage(v0::UsageView {
        calls: 1,
        unreported: 0,
        input: 100,
        cached: 50,
        output: 25,
        reasoning: 5,
        ms: 200,
        micros: 10,
        last_input: 100,
        summary: "the server's usage words".into(),
    });
    assert_eq!(usage.tokens(), 125);
    assert_eq!(usage.summary(), "the server's usage words");
}

#[test]
fn an_offline_client_keeps_requests_without_a_runtime_or_a_connection()
 {
    use hamlet_client::realm::Settings;
    use hamlet_client::{Connection, Tokio};
    let mut app = app();
    assert!(app.world().get_resource::<Tokio>().is_none());
    assert!(app.world().get_resource::<Connection>().is_some());
    assert!(app.world().resource::<Settings>().0.unset);
    app.world_mut().trigger(UserMessage {
        to: address(MAYOR),
        text: "kept while offline".into(),
    });
    app.update();
    assert_eq!(app.world().resource::<Outbox>().iter().count(), 1);
}
