//! Wire messages through the generated public types.

use hamlet_protocol::v0::*;
use serde_json::{Value, json};

const ID: &str = "00000000-0000-4000-8000-000000000001";

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
                id: ID.into(),
                model: "faux/test".into(),
                set: EditCatalogueSet {
                    allowed: None,
                    price,
                    note: None,
                },
            }),
            json!({"type": "edit", "id": ID, "doc": "catalogue", "model": "faux/test", "set": set}),
        );
    }
}

fn address() -> Address {
    Address {
        hamlet: "home".into(),
        agent: "mayor".into(),
    }
}

fn chat() -> ChatRef {
    ChatRef {
        kind: None,
        from: None,
        with: address(),
        past: Some(2),
    }
}

#[test]
fn hello_carries_the_version_token_and_optional_seen_state() {
    client(
        ClientMessage::Hello(Hello {
            v: 0.0,
            token: "test-token".into(),
            seen: None,
        }),
        json!({"type": "hello", "v": 0.0, "token": "test-token"}),
    );
    client(
        ClientMessage::Hello(Hello {
            v: 0.0,
            token: "test-token".into(),
            seen: Some(Seen {
                epoch: 1,
                revision: 3,
            }),
        }),
        json!({"type": "hello", "v": 0.0, "token": "test-token", "seen": {"epoch": 1, "revision": 3}}),
    );
}

#[test]
fn chief_requests_carry_addresses_ids_and_choices() {
    client(
        ClientMessage::Say(Say {
            id: ID.into(),
            to: address(),
            text: "Hello".into(),
        }),
        json!({"type": "say", "id": ID, "to": {"hamlet": "home", "agent": "mayor"}, "text": "Hello"}),
    );
    client(
        ClientMessage::Answer(Answer {
            id: ID.into(),
            wait: "wait-1".into(),
            choice: Choice::Allow,
            text: None,
        }),
        json!({"type": "answer", "id": ID, "wait": "wait-1", "choice": "allow"}),
    );
    client(
        ClientMessage::Answer(Answer {
            id: ID.into(),
            wait: "wait-1".into(),
            choice: Choice::Reply,
            text: Some("Yes".into()),
        }),
        json!({"type": "answer", "id": ID, "wait": "wait-1", "choice": "reply", "text": "Yes"}),
    );
    client(
        ClientMessage::Stop(Stop {
            id: ID.into(),
            agent: None,
        }),
        json!({"type": "stop", "id": ID}),
    );
    client(
        ClientMessage::Stop(Stop {
            id: ID.into(),
            agent: Some(address()),
        }),
        json!({"type": "stop", "id": ID, "agent": {"hamlet": "home", "agent": "mayor"}}),
    );
    client(
        ClientMessage::Rewind(Rewind {
            id: ID.into(),
            chat: chat(),
            item: "entry:0".into(),
        }),
        json!({"type": "rewind", "id": ID, "chat": {"with": {"hamlet": "home", "agent": "mayor"}, "past": 2}, "item": "entry:0"}),
    );
    client(
        ClientMessage::New(New {
            id: ID.into(),
            to: address(),
        }),
        json!({"type": "new", "id": ID, "to": {"hamlet": "home", "agent": "mayor"}}),
    );
    client(
        ClientMessage::Resume(Resume {
            id: ID.into(),
            to: address(),
            number: 2,
        }),
        json!({"type": "resume", "id": ID, "to": {"hamlet": "home", "agent": "mayor"}, "number": 2}),
    );
    client(
        ClientMessage::Retry(Retry {
            id: ID.into(),
            to: address(),
        }),
        json!({"type": "retry", "id": ID, "to": {"hamlet": "home", "agent": "mayor"}}),
    );
}

#[test]
fn edits_choose_the_document_and_keep_limits_set_and_whole_distinct()
{
    client(
        ClientMessage::Edit(Edit::Charter {
            id: ID.into(),
            text: None,
        }),
        json!({"type": "edit", "id": ID, "doc": "charter", "text": null}),
    );
    client(
        ClientMessage::Edit(Edit::Charter {
            id: ID.into(),
            text: Some("Be kind".into()),
        }),
        json!({"type": "edit", "id": ID, "doc": "charter", "text": "Be kind"}),
    );
    client(
        ClientMessage::Edit(Edit::Character {
            id: ID.into(),
            agent: address(),
            text: "Careful".into(),
        }),
        json!({"type": "edit", "id": ID, "doc": "character", "agent": {"hamlet": "home", "agent": "mayor"}, "text": "Careful"}),
    );
    client(
        ClientMessage::Edit(Edit::Notes {
            id: ID.into(),
            agent: address(),
            text: "Remember".into(),
        }),
        json!({"type": "edit", "id": ID, "doc": "notes", "agent": {"hamlet": "home", "agent": "mayor"}, "text": "Remember"}),
    );
    for value in [json!(2.0), Value::Null] {
        client(
            ClientMessage::Edit(Edit::Limits(EditLimits::Set {
                id: ID.into(),
                hamlet: "home".into(),
                reason: None,
                set: EditLimitsSetSet {
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
            id: ID.into(),
            hamlet: "home".into(),
            reason: Some("Room to grow".into()),
            whole: std::collections::BTreeMap::from([(
                "mostAgents".into(),
                json!(4),
            )]),
        })),
        json!({"type": "edit", "id": ID, "doc": "limits", "hamlet": "home", "reason": "Room to grow", "whole": {"mostAgents": 4}}),
    );
    client(
        ClientMessage::Edit(Edit::Settings {
            id: ID.into(),
            set: EditSettingsSet {
                model: Some("faux/test".into()),
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
            id: ID.into(),
            set: EditSettingsSet {
                model: None,
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
            id: ID.into(),
            model: "faux/test".into(),
            set: EditCatalogueSet {
                allowed: Some(true),
                price: None,
                note: Some("Local".into()),
            },
        }),
        json!({"type": "edit", "id": ID, "doc": "catalogue", "model": "faux/test", "set": {"allowed": true, "note": "Local"}}),
    );
}

#[test]
fn watches_open_pages_and_close_without_a_request_id() {
    client(
        ClientMessage::Open(Open {
            id: ID.into(),
            chat: chat(),
            before: Some("entry:0".into()),
        }),
        json!({"type": "open", "id": ID, "chat": {"with": {"hamlet": "home", "agent": "mayor"}, "past": 2}, "before": "entry:0"}),
    );
    client(
        ClientMessage::Open(Open {
            id: ID.into(),
            chat: ChatRef {
                kind: None,
                from: None,
                with: address(),
                past: None,
            },
            before: None,
        }),
        json!({"type": "open", "id": ID, "chat": {"with": {"hamlet": "home", "agent": "mayor"}}}),
    );
    client(
        ClientMessage::Close(Close { chat: chat() }),
        json!({"type": "close", "chat": {"with": {"hamlet": "home", "agent": "mayor"}, "past": 2}}),
    );
}

#[test]
fn logins_start_answer_and_cancel_without_repeating_the_type_tag() {
    client(
        ClientMessage::Login(Login::Start {
            id: ID.into(),
            start: LoginStartStart {
                provider: "openai".into(),
                method: LoginMethod::Key,
            },
        }),
        json!({"type": "login", "id": ID, "start": {"provider": "openai", "method": "key"}}),
    );
    client(
        ClientMessage::Login(Login::Start {
            id: ID.into(),
            start: LoginStartStart {
                provider: "chatgpt".into(),
                method: LoginMethod::Subscription,
            },
        }),
        json!({"type": "login", "id": ID, "start": {"provider": "chatgpt", "method": "subscription"}}),
    );
    client(
        ClientMessage::Login(Login::Reply {
            id: ID.into(),
            reply: LoginReplyReply {
                login: "login-1".into(),
                value: "fake-key".into(),
            },
        }),
        json!({"type": "login", "id": ID, "reply": {"login": "login-1", "value": "fake-key"}}),
    );
    client(
        ClientMessage::Login(Login::Cancel {
            id: ID.into(),
            cancel: "login-1".into(),
        }),
        json!({"type": "login", "id": ID, "cancel": "login-1"}),
    );
}

#[test]
fn checks_and_reads_select_what_the_chief_requested() {
    client(
        ClientMessage::Check(Check::Model {
            id: ID.into(),
            model: Some("faux/test".into()),
        }),
        json!({"type": "check", "id": ID, "what": "model", "model": "faux/test"}),
    );
    client(
        ClientMessage::Check(Check::Model {
            id: ID.into(),
            model: None,
        }),
        json!({"type": "check", "id": ID, "what": "model"}),
    );
    client(
        ClientMessage::Check(Check::Connection { id: ID.into() }),
        json!({"type": "check", "id": ID, "what": "connection"}),
    );
    client(
        ClientMessage::Read(Read::Prompt {
            id: ID.into(),
            agent: address(),
        }),
        json!({"type": "read", "id": ID, "what": "prompt", "agent": {"hamlet": "home", "agent": "mayor"}}),
    );
    client(
        ClientMessage::Read(Read::History {
            id: ID.into(),
            agent: address(),
        }),
        json!({"type": "read", "id": ID, "what": "history", "agent": {"hamlet": "home", "agent": "mayor"}}),
    );
    client(
        ClientMessage::Read(Read::Models {
            id: ID.into(),
            refresh: Some(true),
        }),
        json!({"type": "read", "id": ID, "what": "models", "refresh": true}),
    );
    client(
        ClientMessage::Read(Read::Models {
            id: ID.into(),
            refresh: None,
        }),
        json!({"type": "read", "id": ID, "what": "models"}),
    );
    client(
        ClientMessage::Read(Read::Papers {
            id: ID.into(),
            agent: address(),
            full: Some(true),
        }),
        json!({"type": "read", "id": ID, "what": "papers", "agent": {"hamlet": "home", "agent": "mayor"}, "full": true}),
    );
    client(
        ClientMessage::Read(Read::Papers {
            id: ID.into(),
            agent: address(),
            full: None,
        }),
        json!({"type": "read", "id": ID, "what": "papers", "agent": {"hamlet": "home", "agent": "mayor"}}),
    );
    client(
        ClientMessage::Read(Read::Charter { id: ID.into() }),
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
        "budget": {"period": "day", "tokens": null, "cost": 5.0, "minutes": null},
        "task": {"messages": 12, "tokens": null, "cost": 1.0, "minutes": 20},
        "road": {"tokens": 0, "cost": 0.01, "minutes": 0, "growsOverDays": 7},
        "mostAgents": 12, "mostRoads": 24, "models": ["faux/test"], "model": null,
        "pinned": {"founder": "faux/test"},
        "mayor": {"createAgents": true, "buildRoads": false, "removeAgents": true, "mergeAgents": false, "toldDays": 7},
        "leaving": {"protectedMinutes": 0, "mergedNotes": 6000},
        "asking": {"mayorReview": "remote", "refusals": 3, "questions": 3},
        "permissions": {"network": ["founder"], "unconfined": []}
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
        }
    })
}

fn agent_wire() -> Value {
    json!({
        "ref": {"hamlet": "home", "number": 0}, "name": "founder", "purpose": "Help the chief",
        "since": 1700000000000_i64, "mayor": true, "model": "faux/test", "busy": true, "faulted": false,
        "activity": {"doing": "read-file", "doingWords": "Reading a file", "step": 2, "failures": 0},
        "saying": {"text": "Looking", "thought": true},
        "spent": {"tokens": 100, "micros": 1250000, "ms": 200, "cost": "$1.25"}
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
        "id": "wait-1", "kind": "approval", "danger": "edit", "by": "founder",
        "agent": {"hamlet": "home", "number": 0}, "says": "founder wants to edit", "about": "NOTES.md",
        "change": change_wire(), "answers": ["allow", "always", "deny", "go", "withdraw", "reply", "dismiss"],
        "since": 1700000000000_i64, "asked": 1, "chat": {"with": {"hamlet": "home", "agent": "mayor"}}
    })
}

fn grants_wire() -> Value {
    json!([
        {"agent": {"hamlet": "home", "number": 0}, "name": "founder", "permission": "network", "by": "chief", "source": "grant"},
        {"agent": null, "name": "all", "permission": "unconfined", "by": null, "source": "limits"}
    ])
}

fn settings_wire() -> Value {
    json!({
        "model": "faux/test", "reflectEvery": 10, "allowNet": false,
        "endpoints": [{"name": "local", "baseUrl": "http://localhost:8080", "key": false}],
        "unset": false, "link": {"state": "connected", "models": 1, "words": "Connected"}
    })
}

fn providers_wire() -> Value {
    json!([{"id": "chatgpt", "name": "ChatGPT", "configured": false, "methods": ["key", "subscription"], "label": "Continue with ChatGPT"}])
}

fn state_wire() -> Value {
    json!({
        "type": "state", "epoch": 1, "revision": 2, "chief": "chief", "sandbox": "Seatbelt",
        "hamlets": [hamlet_wire()], "agents": [agent_wire()], "waits": [wait_wire()],
        "grants": grants_wire(), "settings": settings_wire(), "providers": providers_wire()
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
    assert_eq!(state.agents[0].ref_.number, 0);
    assert_eq!(state.agents[0].activity.doing, Doing::ReadFile);
    assert_eq!(
        state.hamlets[0].limits.pinned["founder"],
        "faux/test"
    );
    assert_eq!(
        state.hamlets[0].budget.remains.as_deref(),
        Some("$3.75 left")
    );
    assert_eq!(
        match &state.waits[0] {
            Wait::Approval { change, .. } =>
                change.as_ref().unwrap().lines[0].mark.clone(),
            _ => panic!("Expected approval"),
        },
        ChangeLinesMark::Space
    );
    let updates = [
        json!({"type": "hamlet", "hamlet": hamlet_wire()}),
        json!({"type": "agent", "agent": agent_wire()}),
        json!({"type": "agent-gone", "ref": {"hamlet": "home", "number": 0}}),
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
    let mut ops = vec![
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
        ops.push(json!({"op": "close", "item": "tool:0", "exit": 0, "ms": null, "failure": null, "change": change}));
    }
    for op in ops {
        server(
            json!({"type": "turn", "chat": {"with": {"hamlet": "home", "agent": "mayor"}}, "number": 2, "ops": [op]}),
        );
    }
    server(
        json!({"type": "turn", "chat": {"with": {"hamlet": "home", "agent": "mayor"}}, "ops": []}),
    );
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
        "type": "transcript", "to": ID, "chat": {"with": {"hamlet": "home", "agent": "mayor"}, "past": 2},
        "number": 2, "items": items, "earlier": "entry:0", "working": true, "pending": 1, "usage": usage_wire()
    }));
    server(json!({
        "type": "transcript", "chat": {"with": {"hamlet": "home", "agent": "mayor"}},
        "items": [], "earlier": null, "working": false, "pending": 0, "usage": usage_wire()
    }));
}

#[test]
fn replies_and_login_steps_keep_their_request_and_payload() {
    for kind in ["notice", "error", "rewound"] {
        server(
            json!({"type": "reply", "to": ID, "kind": kind, "text": "Words"}),
        );
    }
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
fn reads_return_each_result_without_a_redundant_type_field() {
    let results = [
        json!({"type": "read", "to": ID, "prompt": "The prompt"}),
        json!({"type": "read", "to": ID, "history": [{
            "number": 1, "current": false, "title": "Earlier", "by": "chief", "began": 10,
            "modified": 20, "messages": 2, "tokens": 140
        }]}),
        json!({"type": "read", "to": ID, "models": [
            {"name": "faux/test", "price": {"input": 1.0, "output": 2.0}, "priced": "listed", "context": 100000, "allowed": true, "note": "Test"},
            {"name": "faux/unknown", "price": null, "priced": "unknown", "context": null, "allowed": false, "note": ""}
        ]}),
        json!({"type": "read", "to": ID, "papers": {"notes": "Remember", "character": "Careful", "skills": [{"name": "test", "description": "A skill"}]}}),
        json!({"type": "read", "to": ID, "charter": {"text": "Be kind", "own": true}}),
    ];
    for result in results {
        server(result);
    }
}

#[test]
fn faults_can_address_a_request_or_the_whole_connection() {
    server(json!({"type": "fault", "text": "Refused", "to": ID}));
    server(json!({"type": "fault", "text": "Refused"}));
}

#[test]
fn older_windows_accept_optional_additions_to_messages_and_nested_entities()
 {
    let expected = server(state_wire());
    let mut newer = state_wire();
    newer["later"] = json!(["new"]);
    newer["agents"][0]["later"] = json!({"value": true});
    assert_eq!(
        serde_json::from_value::<ServerMessage>(newer).unwrap(),
        expected
    );
    assert!(matches!(serde_json::from_value::<ServerMessage>(json!({
        "type": "read", "to": ID, "charter": {"text": "Be kind", "own": true, "later": 1}, "later": true
    })).unwrap(), ServerMessage::Read(ReadResult::Charter { .. })));
}

#[test]
fn file_gifts_round_trip_manifests_and_parts_with_the_upload_receipt()
{
    for wire in [
        json!({"type": "give", "id": ID, "to": {"hamlet": "home", "agent": "mayor"}, "files": [{"path": "folder/a", "size": 2}]}),
        json!({"type": "give-part", "id": ID, "upload": "upload-1", "path": "folder/a", "offset": 1, "data": "Qg=="}),
    ] {
        client(serde_json::from_value(wire.clone()).unwrap(), wire);
    }
    server(
        json!({"type": "reply", "to": ID, "kind": "notice", "text": "Ready for the gift's file parts.", "upload": "upload-1"}),
    );
}

#[test]
fn approval_waits_require_a_danger_and_other_kinds_require_null() {
    let mut wire = wait_wire();
    wire["danger"] = Value::Null;
    assert!(serde_json::from_value::<Wait>(wire.clone()).is_err());
    for kind in ["fault", "question", "objection"] {
        wire["kind"] = kind.into();
        assert!(serde_json::from_value::<Wait>(wire.clone()).is_ok());
        wire["danger"] = "command".into();
        assert!(
            serde_json::from_value::<Wait>(wire.clone()).is_err()
        );
        wire["danger"] = Value::Null;
    }
}

#[test]
fn society_and_realm_fields_round_trip_without_losing_departures_or_ballots()
 {
    let mut wire = state_wire();
    let a = json!({"hamlet": "home", "number": 0});
    let b = json!({"hamlet": "mill", "number": 1});
    wire["agents"][0]["left"] = json!({"how": "merged", "into": a, "at": 12, "by": "the mayor", "reason": "Shared work", "words": "scout merged into founder"});
    wire["agents"][0]["consulting"] = json!(true);
    wire["agents"][0]["conversations"] = json!([{"with": b, "kind": "letters", "count": 2, "last": null}]);
    wire["hamlets"][0]["roads"] = json!([{"a": a, "b": b, "messages": 10, "level": "rail", "lastFrom": null}]);
    wire["hamlets"][0]["votes"] = json!([{"id": 1, "motion": "Seat scout", "by": "the chief", "open": true, "overturnable": true, "report": "Voting", "voters": [{"agent": a, "cast": null}, {"agent": b, "cast": "none"}]}]);
    wire["hamlets"][0]["at"] = json!({"x": 2.5, "z": -3.5});
    wire["library"] = json!([{"name": "maps", "description": "Draw maps", "by": "scout"}]);
    wire["closed"] = json!([{"id": "mill", "name": "The Mill", "at": 13, "by": "the chief", "reason": "Finished"}]);
    server(wire.clone());
    wire["hamlets"][0]["at"] = Value::Null;
    wire["agents"][0]["left"]["into"] = Value::Null;
    server(wire);
    let wire = json!({"type": "open", "id": ID, "chat": {"with": {"hamlet": "home", "agent": "mayor"}, "from": b}});
    client(serde_json::from_value(wire.clone()).unwrap(), wire);
}

#[test]
fn a_peer_watch_selects_questions() {
    client(
        ClientMessage::Open(Open {
            id: ID.into(),
            chat: ChatRef {
                from: Some(AgentRef {
                    hamlet: "home".into(),
                    number: 2,
                }),
                kind: Some(ChatRefKind::Questions),
                past: None,
                with: address(),
            },
            before: None,
        }),
        json!({"type": "open", "id": ID, "chat": {"with": {"hamlet": "home", "agent": "mayor"}, "from": {"hamlet": "home", "number": 2}, "kind": "questions"}}),
    );
}
