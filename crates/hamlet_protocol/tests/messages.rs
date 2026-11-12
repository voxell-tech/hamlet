//! Wire messages through the public types.

use hamlet_protocol::client::*;
use serde_json::{Value, json};

const ID: &str = "00000000-0000-4000-8000-000000000001";
const IMAGE: &str =
    "0000000000000000000000000000000000000000000000000000000000000001";

fn id() -> RequestId {
    ID.parse().unwrap()
}

fn client(message: ClientMessage, wire: Value) {
    assert_eq!(serde_json::to_value(&message).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<ClientMessage>(wire).unwrap(),
        message
    );
}

#[test]
fn catalogue_edits_preserve_missing_null_and_assigned_prices() {
    for (price, set) in [
        (None, json!({})),
        (Some(None), json!({"price": null})),
        (
            Some(Some(Price {
                input: 1.0,
                output: 2.0,
            })),
            json!({"price": {"input": 1.0, "output": 2.0}}),
        ),
    ] {
        client(
            ClientMessage::Edit(Edit::Catalogue {
                id: id(),
                model: "faux/test".into(),
                set: EditCatalogueSet {
                    images: None,
                    price,
                    note: None,
                },
            }),
            json!({"type": "edit", "id": ID, "doc": "catalogue", "model": "faux/test", "set": set}),
        );
    }
}

fn agent() -> AgentId {
    AgentId(2)
}

fn chat() -> ChatRef {
    ChatRef {
        kind: None,
        from: None,
        with: agent(),
        past: Some(2),
    }
}

#[test]
fn hello_carries_the_version_and_token() {
    client(
        ClientMessage::Hello(Hello {
            v: VERSION,
            token: "test-token".into(),
        }),
        json!({"type": "hello", "v": VERSION.to_string(), "token": "test-token"}),
    );
}

#[test]
fn chief_requests_carry_addresses_ids_and_choices() {
    client(
        ClientMessage::Say(Say {
            files: Vec::new(),
            id: id(),
            images: Vec::new(),
            to: agent(),
            text: "Hello".into(),
        }),
        json!({"type": "say", "id": ID, "to": 2, "text": "Hello"}),
    );
    client(
        ClientMessage::Say(Say {
            files: Vec::new(),
            id: id(),
            images: vec![
                AttachedImage::last(IMAGE.parse().unwrap()),
                AttachedImage {
                    at: Some(2),
                    id: IMAGE.parse().unwrap(),
                },
            ],
            to: agent(),
            text: "Look".into(),
        }),
        json!({"type": "say", "id": ID, "images": [{"id": IMAGE}, {"at": 2, "id": IMAGE}], "to": 2, "text": "Look"}),
    );
    client(
        ClientMessage::Say(Say {
            files: vec![AttachedFile {
                at: Some(4),
                id: IMAGE.parse().unwrap(),
                name: "notes/a.md".into(),
                size: 3,
            }],
            id: id(),
            images: Vec::new(),
            to: agent(),
            text: "Read".into(),
        }),
        json!({"type": "say", "id": ID, "files": [{"at": 4, "id": IMAGE, "name": "notes/a.md", "size": 3}], "to": 2, "text": "Read"}),
    );
    client(
        ClientMessage::PutFile(PutFile { id: id(), size: 3 }),
        json!({"type": "put-file", "id": ID, "size": 3}),
    );
    client(
        ClientMessage::PutFilePart(PutFilePart {
            data: "YWJj".into(),
            id: id(),
            offset: 0,
            upload: "u".into(),
        }),
        json!({"type": "put-file-part", "id": ID, "data": "YWJj", "offset": 0, "upload": "u"}),
    );
    client(
        ClientMessage::Answer(Answer {
            id: id(),
            wait: "wait-1".into(),
            choice: Choice::Allow,
            text: None,
            picks: None,
        }),
        json!({"type": "answer", "id": ID, "wait": "wait-1", "choice": "allow"}),
    );
    client(
        ClientMessage::Answer(Answer {
            id: id(),
            wait: "wait-1".into(),
            choice: Choice::Reply,
            text: Some("Yes".into()),
            picks: None,
        }),
        json!({"type": "answer", "id": ID, "wait": "wait-1", "choice": "reply", "text": "Yes"}),
    );
    client(
        ClientMessage::Stop(Stop {
            id: id(),
            agent: None,
        }),
        json!({"type": "stop", "id": ID}),
    );
    client(
        ClientMessage::Stop(Stop {
            id: id(),
            agent: Some(agent()),
        }),
        json!({"type": "stop", "id": ID, "agent": 2}),
    );
    client(
        ClientMessage::Rewind(Rewind {
            id: id(),
            chat: chat(),
            item: "entry:0".into(),
        }),
        json!({"type": "rewind", "id": ID, "chat": {"with": 2, "past": 2}, "item": "entry:0"}),
    );
    client(
        ClientMessage::New(New {
            id: id(),
            to: agent(),
        }),
        json!({"type": "new", "id": ID, "to": 2}),
    );
    client(
        ClientMessage::Resume(Resume {
            id: id(),
            to: agent(),
            number: 2,
        }),
        json!({"type": "resume", "id": ID, "to": 2, "number": 2}),
    );
    client(
        ClientMessage::Retry(Retry {
            id: id(),
            to: agent(),
        }),
        json!({"type": "retry", "id": ID, "to": 2}),
    );
}

#[test]
fn edits_choose_the_document_and_keep_limits_set_and_whole_distinct()
{
    client(
        ClientMessage::Edit(Edit::Charter {
            id: id(),
            text: None,
        }),
        json!({"type": "edit", "id": ID, "doc": "charter", "text": null}),
    );
    client(
        ClientMessage::Edit(Edit::Charter {
            id: id(),
            text: Some("Be kind".into()),
        }),
        json!({"type": "edit", "id": ID, "doc": "charter", "text": "Be kind"}),
    );
    client(
        ClientMessage::Edit(Edit::Character {
            id: id(),
            scholar: ScholarId(1),
            text: "Careful".into(),
        }),
        json!({"type": "edit", "id": ID, "doc": "character", "scholar": 1, "text": "Careful"}),
    );
    for (of, wire) in [
        (Keeper::Scholar(ScholarId(1)), json!({"scholar": 1})),
        (Keeper::Agent(agent()), json!({"agent": 2})),
    ] {
        client(
            ClientMessage::Edit(Edit::Memory {
                id: id(),
                of,
                path: "notes.md".into(),
                text: "Remember".into(),
            }),
            json!({"type": "edit", "id": ID, "doc": "memory", "of": wire, "path": "notes.md", "text": "Remember"}),
        );
    }
    for value in [json!(2.0), Value::Null] {
        client(
            ClientMessage::Edit(Edit::Limits(EditLimits::Set {
                id: id(),
                hamlet: Some("home".into()),
                reason: None,
                set: LimitsEdit {
                    section: "budget".into(),
                    key: "cost".into(),
                    value: value.clone(),
                },
            })),
            json!({"type": "edit", "id": ID, "doc": "limits", "hamlet": "home", "set": {"section": "budget", "key": "cost", "value": value}}),
        );
    }
    client(
        ClientMessage::Edit(Edit::Limits(EditLimits::Whole {
            id: id(),
            hamlet: Some("home".into()),
            reason: Some("Room to grow".into()),
            whole: std::collections::BTreeMap::from([(
                "toldDays".into(),
                json!(4),
            )]),
        })),
        json!({"type": "edit", "id": ID, "doc": "limits", "hamlet": "home", "reason": "Room to grow", "whole": {"toldDays": 4}}),
    );
    client(
        ClientMessage::Edit(Edit::Limits(EditLimits::Whole {
            id: id(),
            hamlet: None,
            reason: None,
            whole: std::collections::BTreeMap::new(),
        })),
        json!({"type": "edit", "id": ID, "doc": "limits", "whole": {}}),
    );
    client(
        ClientMessage::Edit(Edit::Settings {
            id: id(),
            set: EditSettingsSet {
                model: Some("faux/test".into()),
                character: None,
                effort: None,
                newcomers: None,
                reflect_every: Some(10),
                allow_net: Some(false),
                endpoints: Some(vec![Endpoint {
                    name: "local".into(),
                    base_url: "http://localhost:8080".into(),
                }]),
                workspace: None,
            },
        }),
        json!({"type": "edit", "id": ID, "doc": "settings", "set": {"model": "faux/test", "reflectEvery": 10, "allowNet": false, "endpoints": [{"name": "local", "baseUrl": "http://localhost:8080"}]}}),
    );
    client(
        ClientMessage::Edit(Edit::Settings {
            id: id(),
            set: EditSettingsSet {
                model: None,
                character: None,
                effort: None,
                newcomers: None,
                reflect_every: None,
                allow_net: None,
                endpoints: None,
                workspace: None,
            },
        }),
        json!({"type": "edit", "id": ID, "doc": "settings", "set": {}}),
    );
    client(
        ClientMessage::Edit(Edit::Catalogue {
            id: id(),
            model: "faux/test".into(),
            set: EditCatalogueSet {
                images: Some(Some(ImageInput {
                    per_line: Some(4),
                    ..Default::default()
                })),
                price: None,
                note: Some("Local".into()),
            },
        }),
        json!({"type": "edit", "id": ID, "doc": "catalogue", "model": "faux/test", "set": {"images": {"perLine": 4}, "note": "Local"}}),
    );
}

#[test]
fn watches_open_pages_and_close_without_a_request_id() {
    client(
        ClientMessage::Open(Open {
            id: id(),
            chat: chat(),
            before: Some("entry:0".into()),
        }),
        json!({"type": "open", "id": ID, "chat": {"with": 2, "past": 2}, "before": "entry:0"}),
    );
    client(
        ClientMessage::Open(Open {
            id: id(),
            chat: ChatRef {
                kind: None,
                from: None,
                with: agent(),
                past: None,
            },
            before: None,
        }),
        json!({"type": "open", "id": ID, "chat": {"with": 2}}),
    );
    client(
        ClientMessage::Close(Close { chat: chat() }),
        json!({"type": "close", "chat": {"with": 2, "past": 2}}),
    );
}

#[test]
fn logins_start_answer_and_cancel_without_repeating_the_type_tag() {
    client(
        ClientMessage::Login(Login::Start {
            id: id(),
            start: LoginStart {
                provider: "openai".into(),
                method: LoginMethod::Key,
            },
        }),
        json!({"type": "login", "id": ID, "start": {"provider": "openai", "method": "key"}}),
    );
    client(
        ClientMessage::Login(Login::Start {
            id: id(),
            start: LoginStart {
                provider: "chatgpt".into(),
                method: LoginMethod::Subscription,
            },
        }),
        json!({"type": "login", "id": ID, "start": {"provider": "chatgpt", "method": "subscription"}}),
    );
    client(
        ClientMessage::Login(Login::Reply {
            id: id(),
            reply: LoginReply {
                login: "login-1".into(),
                value: "fake-key".into(),
            },
        }),
        json!({"type": "login", "id": ID, "reply": {"login": "login-1", "value": "fake-key"}}),
    );
    client(
        ClientMessage::Login(Login::Cancel {
            id: id(),
            cancel: "login-1".into(),
        }),
        json!({"type": "login", "id": ID, "cancel": "login-1"}),
    );
}

#[test]
fn checks_and_reads_select_what_the_chief_requested() {
    client(
        ClientMessage::Check(Check::Model {
            id: id(),
            model: Some("faux/test".into()),
        }),
        json!({"type": "check", "id": ID, "what": "model", "model": "faux/test"}),
    );
    client(
        ClientMessage::Check(Check::Model {
            id: id(),
            model: None,
        }),
        json!({"type": "check", "id": ID, "what": "model"}),
    );
    client(
        ClientMessage::Check(Check::Connection { id: id() }),
        json!({"type": "check", "id": ID, "what": "connection"}),
    );
    client(
        ClientMessage::Read(Read::Prompt {
            id: id(),
            agent: agent(),
        }),
        json!({"type": "read", "id": ID, "what": "prompt", "agent": 2}),
    );
    client(
        ClientMessage::Read(Read::History {
            id: id(),
            agent: agent(),
        }),
        json!({"type": "read", "id": ID, "what": "history", "agent": 2}),
    );
    client(
        ClientMessage::Read(Read::Models {
            id: id(),
            refresh: Some(true),
        }),
        json!({"type": "read", "id": ID, "what": "models", "refresh": true}),
    );
    client(
        ClientMessage::Read(Read::Models {
            id: id(),
            refresh: None,
        }),
        json!({"type": "read", "id": ID, "what": "models"}),
    );
    client(
        ClientMessage::Read(Read::Papers {
            id: id(),
            of: Keeper::Agent(agent()),
            full: Some(true),
        }),
        json!({"type": "read", "id": ID, "what": "papers", "of": {"agent": 2}, "full": true}),
    );
    client(
        ClientMessage::Read(Read::Papers {
            id: id(),
            of: Keeper::Scholar(ScholarId(1)),
            full: None,
        }),
        json!({"type": "read", "id": ID, "what": "papers", "of": {"scholar": 1}}),
    );
    client(
        ClientMessage::Read(Read::Charter { id: id() }),
        json!({"type": "read", "id": ID, "what": "charter"}),
    );
}

fn server(wire: Value) -> ServerMessage {
    let message =
        serde_json::from_value::<ServerMessage>(wire.clone())
            .unwrap();
    assert_eq!(serde_json::to_value(&message).unwrap(), wire);
    message
}

fn limits_wire() -> Value {
    json!({
        "budget": {"period": "day", "cost": 5.0, "minutes": null},
        "road": {"price": 0.01, "most": 24, "growsOverDays": 7},
        "reserved": {"hire": false, "roads": true, "end": false, "merge": true},
        "model": null, "toldDays": 7, "protectedMinutes": 0,
        "asking": {"review": "remote", "questions": 3, "refusals": 3}
    })
}

fn hamlet_wire() -> Value {
    json!({
        "id": "home", "name": "Home", "folder": null, "before": ["/tmp/old"],
        "mode": "manual", "modeWords": "Commands wait for a chief.", "limits": limits_wire(),
        "budget": {
            "period": "day", "periodWords": "today", "gone": 0.25,
            "spent": {"tokens": 100, "micros": 1250000, "ms": 200},
            "caps": {"micros": 5000000, "ms": null}, "summary": "$1.25 today", "lines": ["100 tokens"],
            "left": 0.75, "remains": "$3.75 left"
        },
        "roads": [], "at": null, "mayor": 2, "tier": 50, "chiefs": ["chief"]
    })
}

fn hand_wire() -> Value {
    json!({
        "id": 1, "kind": "hand", "name": "hand", "nickname": null, "hamlet": null, "scholar": null,
        "parent": null, "pin": null, "purpose": "", "since": 0, "mayor": false, "model": "faux/test",
        "ownModel": null, "effort": null, "fast": false,
        "busy": false, "faulted": false,
        "activity": {"doing": "rest", "doingWords": "At rest", "step": 0, "failures": 0},
        "saying": {"text": "", "thought": false},
        "spent": {"tokens": 0, "micros": 0, "ms": 0, "cost": "$0.00"},
        "consulting": false, "conversations": [], "skills": []
    })
}

fn agent_wire() -> Value {
    json!({
        "id": 2, "kind": "hire", "name": "hire#2", "nickname": "Fen", "hamlet": "home", "scholar": 1,
        "parent": 1, "pin": "v1", "purpose": "Help the chief",
        "since": 1700000000000_i64, "mayor": true, "model": "faux/test",
        "ownModel": "faux/test", "effort": "xhigh", "fast": true,
        "busy": true, "faulted": false,
        "activity": {"doing": "read-file", "doingWords": "Reading a file", "step": 2, "failures": 0},
        "saying": {"text": "Looking", "thought": true},
        "spent": {"tokens": 100, "micros": 1250000, "ms": 200, "cost": "$1.25"},
        "consulting": false, "conversations": [], "skills": ["release"]
    })
}

fn scholar_wire() -> Value {
    json!({
        "id": 1, "name": "founder", "chief": "chief", "since": 0,
        "arrival": "waiting", "arrivalBy": "chief", "house": null,
        "skills": ["release"], "versions": [{"at": 5, "name": "v1", "note": "First"}], "hires": [2]
    })
}

fn change_wire() -> Value {
    json!({
        "path": "NOTES.md", "created": false,
        "lines": [{"mark": " ", "text": "Same"}, {"mark": "+", "text": "New"}, {"mark": "-", "text": "Old"}],
        "total": 3, "added": 1, "removed": 1, "count": "1 added, 1 removed"
    })
}

fn wait_wire() -> Value {
    json!({
        "id": "wait-1", "by": "hire#2", "agent": 2,
        "about": "NOTES.md", "subject": "NOTES.md", "change": change_wire(),
        "answers": ["allow", "always", "deny", "go", "withdraw", "reply", "dismiss"],
        "since": 1700000000000_u64, "asked": 1, "chat": {"with": 2},
        "detail": {"kind": "approval", "danger": "edit", "says": "wants to edit", "always": "session"}
    })
}

fn grants_wire() -> Value {
    json!([
        {"agent": 2, "name": "hire#2", "permission": "network", "by": "chief", "source": "grant"},
        {"agent": null, "name": "all", "permission": "unconfined", "by": null, "source": "limits"}
    ])
}

fn settings_wire() -> Value {
    json!({
        "model": "faux/test", "reflectEvery": 10, "allowNet": false, "effort": "low",
        "newcomers": "hand", "character": "Curious.",
        "endpoints": [{"name": "local", "baseUrl": "http://localhost:8080", "key": false}],
        "unset": false, "link": {"state": "connected", "models": 1, "words": "Connected"},
        "workspace": {"url": null, "running": null}
    })
}

fn providers_wire() -> Value {
    json!([{"id": "chatgpt", "name": "ChatGPT", "configured": false, "methods": ["key", "subscription"], "label": "Continue with ChatGPT"}])
}

fn state_wire() -> Value {
    json!({
        "type": "state", "epoch": 1, "revision": 2, "chief": "chief", "confinement": {"kind": "sandbox", "name": "Seatbelt"},
        "hamlets": [hamlet_wire()], "agents": [hand_wire(), agent_wire()], "waits": [wait_wire()],
        "grants": grants_wire(), "settings": settings_wire(), "providers": providers_wire(),
        "realm": "/home/chief/.hamlet", "hand": 1, "scholars": [scholar_wire()], "limits": {"roadsBetween": true, "budget": {"period": "day", "cost": 5.0, "minutes": null}}, "library": [], "closed": [],
        "deeds": [{"name": "mode", "usage": "<manual|auto>", "description": "Whether agents ask before they change the folder"}]
    })
}

fn usage_wire() -> Value {
    json!({
        "calls": 2, "unreported": 0, "input": 100, "cached": 20, "output": 40, "reasoning": 10,
        "ms": 200, "micros": 500, "lastInput": 100, "summary": "140 tokens"
    })
}

#[test]
fn state_and_every_update_keep_the_servers_ready_made_values() {
    let ServerMessage::State(state) = server(state_wire()) else {
        panic!("Expected state")
    };
    assert_eq!(state.hand, AgentId(1));
    assert_eq!(state.agents[0].kind, AgentKind::Hand);
    assert_eq!(state.agents[1].name, "hire#2");
    assert_eq!(state.agents[1].activity.doing, Doing::ReadFile);
    assert_eq!(state.hamlets[0].mayor, AgentId(2));
    assert_eq!(state.scholars[0].hires, [AgentId(2)]);
    assert_eq!(
        state.hamlets[0].budget.remains.as_deref(),
        Some("$3.75 left")
    );
    assert_eq!(
        state.waits[0].change.as_ref().unwrap().lines[0].mark,
        ChangeLinesMark::Space
    );
    let updates = [
        json!({"type": "hamlet", "hamlet": hamlet_wire()}),
        json!({"type": "agent", "agent": agent_wire()}),
        json!({"type": "agent-gone", "id": 2}),
        json!({"type": "scholar", "scholar": scholar_wire()}),
        json!({"type": "scholar-gone", "id": 1}),
        json!({"type": "wait", "wait": wait_wire()}),
        json!({"type": "wait-closed", "id": "wait-1", "how": "allowed"}),
        json!({"type": "grants", "grants": grants_wire()}),
        json!({"type": "settings", "settings": settings_wire()}),
        json!({"type": "providers", "providers": providers_wire()}),
    ];
    for update in updates {
        let ServerMessage::Change(changes) = server(
            json!({"type": "change", "epoch": 1, "revision": 3, "changes": [update]}),
        ) else {
            panic!("Expected change")
        };
        assert_eq!(changes.changes.len(), 1);
    }
}

#[test]
fn turns_keep_every_operation_and_optional_nullable_tool_changes() {
    let mut operations = vec![
        json!({"op": "add", "item": {"id": "entry:0", "at": 10, "kind": "assistant", "text": "Hi"}}),
        json!({"op": "add", "item": {"id": "entry:1", "at": 10, "kind": "assistant", "text": "Hello"}, "replaces": "entry:0"}),
        json!({"op": "append", "item": "entry:1", "text": " chief"}),
        json!({"op": "close", "item": "tool:0", "exit": 7, "ms": 20, "failure": "Failed"}),
        json!({"op": "status", "working": true, "pending": 2, "last": {"what": "Reading", "at": 10}}),
        json!({"op": "status", "working": false, "pending": 0, "last": null}),
        json!({"op": "usage", "usage": usage_wire()}),
        json!({"op": "reset"}),
    ];
    for change in [Value::Null, change_wire()] {
        operations.push(json!({"op": "close", "item": "tool:0", "exit": 0, "ms": null, "failure": null, "change": change}));
    }
    for operation in operations {
        server(
            json!({"type": "turn", "chat": {"with": 2}, "number": 2, "ops": [operation]}),
        );
    }
    server(json!({"type": "turn", "chat": {"with": 2}, "ops": []}));
}

#[test]
fn transcripts_keep_every_item_kind_and_page_status() {
    let mut items = ["user", "assistant", "reasoning", "notice", "error"].into_iter()
        .map(|kind| json!({"id": kind, "at": 10, "kind": kind, "text": "Words"})).collect::<Vec<_>>();
    items[0]["chief"] = json!("chief");
    items[0]["rewindable"] = json!(true);
    for (exit, change) in
        [(Value::Null, Value::Null), (json!(7), change_wire())]
    {
        items.push(json!({"id": "tool:0", "at": 10, "kind": "tool", "text": "", "tool": {
            "name": "bash", "summary": "echo test", "exit": exit, "ms": null, "failure": null, "change": change
        }}));
    }
    server(json!({
        "type": "transcript", "to": ID, "chat": {"with": 2, "past": 2},
        "number": 2, "items": items, "earlier": "entry:0", "last": {"what": "Reading", "at": 10},
        "working": true, "pending": 1, "usage": usage_wire()
    }));
    server(json!({
        "type": "transcript", "chat": {"with": 2},
        "items": [], "earlier": null, "last": null, "working": false, "pending": 0, "usage": usage_wire()
    }));
}

#[test]
fn login_steps_keep_their_request_and_payload() {
    for step in [
        json!({"url": "https://example.invalid/login", "instructions": "Open this"}),
        json!({"url": "https://example.invalid/login"}),
        json!({"code": "ABCD"}),
        json!({"prompt": "Key", "secret": true}),
        json!({"progress": "Waiting"}),
        json!({"done": "Connected"}),
        json!({"failed": "Cancelled"}),
    ] {
        server(
            json!({"type": "login", "login": "login-1", "to": ID, "step": step}),
        );
    }
    server(
        json!({"type": "login", "login": "login-1", "step": {"done": "Connected"}}),
    );
}

#[test]
fn faults_can_address_a_request_or_the_whole_connection() {
    server(json!({"type": "fault", "text": "Refused", "to": ID}));
    server(json!({"type": "fault", "text": "Refused"}));
}

#[test]
fn unknown_and_missing_nullable_fields_are_refused_at_every_level() {
    let mut state = state_wire();
    state["later"] = json!(true);
    let mut agent = state_wire();
    agent["agents"][1]["later"] = json!(true);
    let mut detail = state_wire();
    detail["waits"][0]["detail"]["later"] = json!(true);
    let mut at = state_wire();
    at["hamlets"][0].as_object_mut().unwrap().remove("at");
    let mut words = state_wire();
    words["settings"]["link"]
        .as_object_mut()
        .unwrap()
        .remove("words");
    for wire in [
        state,
        agent,
        detail,
        at,
        words,
        json!({"type": "read", "to": ID, "charter": {"text": "Be kind", "own": true}, "later": true}),
        json!({"type": "read", "to": ID, "charter": {"text": "Be kind", "own": true, "later": 1}}),
        json!({"type": "login", "login": "login-1", "step": {"done": "Connected", "later": 1}}),
    ] {
        assert!(
            serde_json::from_value::<ServerMessage>(wire.clone())
                .is_err(),
            "{wire}"
        );
    }
    for wire in [
        json!({"type": "hello", "v": VERSION.to_string(), "token": "t", "later": 1}),
        json!({"type": "edit", "id": ID, "doc": "limits", "hamlet": "home", "whole": {}, "later": 1}),
    ] {
        assert!(
            serde_json::from_value::<ClientMessage>(wire.clone())
                .is_err(),
            "{wire}"
        );
    }
}

#[test]
fn society_and_realm_fields_round_trip_without_losing_departures_or_scholars()
 {
    let mut wire = state_wire();
    let (a, b) = (json!(2), json!(3));
    wire["agents"][1]["left"] = json!({"how": "merged", "into": a, "at": 12, "by": "the mayor", "reason": "Shared work", "words": "scout merged into founder"});
    wire["agents"][1]["consulting"] = json!(true);
    wire["agents"][1]["conversations"] = json!([{"with": b, "kind": "letters", "count": 2, "last": null}]);
    wire["hamlets"][0]["roads"] = json!([{"a": a, "b": b, "kind": "agreed", "messages": 10, "level": "rail", "lastFrom": null}]);
    wire["scholars"][0]["left"] =
        json!({"at": 14, "by": "the chief", "reason": "Retired"});
    wire["scholars"][0]["arrival"] = json!("housed");
    wire["scholars"][0]["house"] =
        json!({"x": 1.0, "z": 2.0, "turn": 0.5});
    wire["hamlets"][0]["at"] = json!({"x": 2.5, "z": -3.5});
    wire["library"] = json!([{"name": "maps", "description": "Draw maps", "by": "scout"}]);
    wire["closed"] = json!([{"id": "mill", "name": "The Mill", "at": 13, "by": "the chief", "reason": "Finished"}]);
    server(wire.clone());
    wire["hamlets"][0]["at"] = Value::Null;
    wire["agents"][1]["left"]["into"] = Value::Null;
    server(wire);
    let wire = json!({"type": "open", "id": ID, "chat": {"with": 2, "from": 3}});
    client(serde_json::from_value(wire.clone()).unwrap(), wire);
}

#[test]
fn a_peer_watch_selects_questions() {
    client(
        ClientMessage::Open(Open {
            id: id(),
            chat: ChatRef {
                from: Some(AgentId(3)),
                kind: Some(ChatRefKind::Questions),
                past: None,
                with: agent(),
            },
            before: None,
        }),
        json!({"type": "open", "id": ID, "chat": {"with": 2, "from": 3, "kind": "questions"}}),
    );
}

#[test]
fn every_message_type_is_documented() {
    let page = include_str!("../../../docs/protocol.md");
    let mut generator =
        schemars::generate::SchemaSettings::draft2020_12()
            .for_serialize()
            .into_generator();
    let mut missing = Vec::new();
    for union in [
        generator.root_schema_for::<ClientMessage>(),
        generator.root_schema_for::<ServerMessage>(),
    ] {
        let union = union.to_value();
        let forms = union["oneOf"].as_array().unwrap();
        assert!(!forms.is_empty());
        for form in forms {
            let kind =
                form["properties"]["type"]["const"].as_str().unwrap();
            if !page.contains(&format!("`{kind}`")) {
                missing.push(kind.to_owned());
            }
        }
    }
    assert_eq!(
        missing,
        Vec::<String>::new(),
        "missing from docs/protocol.md"
    );
}
