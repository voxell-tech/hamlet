//! Wire messages through the public types.

use hamlet_protocol::client::*;
use serde_json::{Value, json};

const ID: &str = "00000000-0000-4000-8000-000000000001";

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
fn hello_carries_the_version_and_token() {
    client(
        ClientMessage::Hello(Hello {
            v: V,
            token: "test-token".into(),
        }),
        json!({"type": "hello", "v": V, "token": "test-token"}),
    );
}

#[test]
fn chief_requests_carry_addresses_ids_and_choices() {
    client(
        ClientMessage::Say(Say {
            id: id(),
            to: address(),
            text: "Hello".into(),
        }),
        json!({"type": "say", "id": ID, "to": {"hamlet": "home", "agent": "mayor"}, "text": "Hello"}),
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
            agent: Some(address()),
        }),
        json!({"type": "stop", "id": ID, "agent": {"hamlet": "home", "agent": "mayor"}}),
    );
    client(
        ClientMessage::Rewind(Rewind {
            id: id(),
            chat: chat(),
            item: "entry:0".into(),
        }),
        json!({"type": "rewind", "id": ID, "chat": {"with": {"hamlet": "home", "agent": "mayor"}, "past": 2}, "item": "entry:0"}),
    );
    client(
        ClientMessage::New(New {
            id: id(),
            to: address(),
        }),
        json!({"type": "new", "id": ID, "to": {"hamlet": "home", "agent": "mayor"}}),
    );
    client(
        ClientMessage::Resume(Resume {
            id: id(),
            to: address(),
            number: 2,
        }),
        json!({"type": "resume", "id": ID, "to": {"hamlet": "home", "agent": "mayor"}, "number": 2}),
    );
    client(
        ClientMessage::Retry(Retry {
            id: id(),
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
            agent: address(),
            text: "Careful".into(),
        }),
        json!({"type": "edit", "id": ID, "doc": "character", "agent": {"hamlet": "home", "agent": "mayor"}, "text": "Careful"}),
    );
    client(
        ClientMessage::Edit(Edit::Notes {
            id: id(),
            agent: address(),
            text: "Remember".into(),
        }),
        json!({"type": "edit", "id": ID, "doc": "notes", "agent": {"hamlet": "home", "agent": "mayor"}, "text": "Remember"}),
    );
    for value in [json!(2.0), Value::Null] {
        client(
            ClientMessage::Edit(Edit::Limits(EditLimits::Set {
                id: id(),
                hamlet: "home".into(),
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
            id: id(),
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
            id: id(),
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
            id: id(),
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
            id: id(),
            chat: chat(),
            before: Some("entry:0".into()),
        }),
        json!({"type": "open", "id": ID, "chat": {"with": {"hamlet": "home", "agent": "mayor"}, "past": 2}, "before": "entry:0"}),
    );
    client(
        ClientMessage::Open(Open {
            id: id(),
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
            agent: address(),
        }),
        json!({"type": "read", "id": ID, "what": "prompt", "agent": {"hamlet": "home", "agent": "mayor"}}),
    );
    client(
        ClientMessage::Read(Read::History {
            id: id(),
            agent: address(),
        }),
        json!({"type": "read", "id": ID, "what": "history", "agent": {"hamlet": "home", "agent": "mayor"}}),
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
            agent: address(),
            full: Some(true),
        }),
        json!({"type": "read", "id": ID, "what": "papers", "agent": {"hamlet": "home", "agent": "mayor"}, "full": true}),
    );
    client(
        ClientMessage::Read(Read::Papers {
            id: id(),
            agent: address(),
            full: None,
        }),
        json!({"type": "read", "id": ID, "what": "papers", "agent": {"hamlet": "home", "agent": "mayor"}}),
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
        "votes": {
            "passYes": 2, "passOf": 3, "leastVoters": 3, "sealed": true, "groundedOnly": false,
            "ballotMinutes": 5, "openMinutes": 60, "retryMinutes": 60
        },
        "hamlets": {"most": 6, "roadsBetween": true, "removalMayors": 3},
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
        },
        "roads": [], "votes": [], "at": null
    })
}

fn agent_wire() -> Value {
    json!({
        "ref": {"hamlet": "home", "number": 0}, "name": "founder", "purpose": "Help the chief",
        "since": 1700000000000_i64, "mayor": true, "model": "faux/test", "busy": true, "faulted": false,
        "activity": {"doing": "read-file", "doingWords": "Reading a file", "step": 2, "failures": 0},
        "saying": {"text": "Looking", "thought": true},
        "spent": {"tokens": 100, "micros": 1250000, "ms": 200, "cost": "$1.25"},
        "consulting": false, "conversations": [], "skills": ["release"]
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
        "id": "wait-1", "by": "founder", "agent": {"hamlet": "home", "number": 0},
        "about": "NOTES.md", "subject": "NOTES.md", "change": change_wire(),
        "answers": ["allow", "always", "deny", "go", "withdraw", "reply", "dismiss"],
        "since": 1700000000000_u64, "asked": 1, "chat": {"with": {"hamlet": "home", "agent": "mayor"}},
        "detail": {"kind": "approval", "danger": "edit", "says": "wants to edit", "always": "session"}
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
        "hamlets": [hamlet_wire()], "agents": [agent_wire()], "waits": [wait_wire()],
        "grants": grants_wire(), "settings": settings_wire(), "providers": providers_wire(),
        "houses": "/home/chief/.hamlet/houses", "library": [], "closed": [],
        "deeds": [{"name": "mode", "usage": "<manual|auto>", "description": "Whether agents ask before they change the folder"}],
        "starters": [{"name": "web-search", "description": "Searches the web"}]
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
    assert_eq!(state.agents[0].agent.number, 0);
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
        state.waits[0].change.as_ref().unwrap().lines[0].mark,
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
            json!({"type": "turn", "chat": {"with": {"hamlet": "home", "agent": "mayor"}}, "number": 2, "ops": [operation]}),
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
        "number": 2, "items": items, "earlier": "entry:0", "last": {"what": "Reading", "at": 10},
        "working": true, "pending": 1, "usage": usage_wire()
    }));
    server(json!({
        "type": "transcript", "chat": {"with": {"hamlet": "home", "agent": "mayor"}},
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
    agent["agents"][0]["later"] = json!(true);
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
        json!({"type": "hello", "v": V, "token": "t", "later": 1}),
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
            id: id(),
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

#[test]
fn the_bazaar_takes_skills_and_founds_from_templates() {
    client(
        ClientMessage::TakeSkill(TakeSkill {
            files: vec![SkillText {
                path: "SKILL.md".into(),
                text: "---\nname: maps\n---\n".into(),
            }],
            from: "skills.sh/acme/maps".into(),
            id: id(),
            name: "maps".into(),
        }),
        json!({"type": "take-skill", "id": ID, "name": "maps", "from": "skills.sh/acme/maps", "files": [{"path": "SKILL.md", "text": "---\nname: maps\n---\n"}]}),
    );
    let settler = |name: &str| Settler {
        character: "Calm.".into(),
        name: name.into(),
        notes: "# Notes\n".into(),
        purpose: "Works.".into(),
        skills: vec![SettlerSkill {
            body: "Dig.".into(),
            description: "Digs.".into(),
            name: "dig".into(),
        }],
    };
    let settler_wire = |name: &str| json!({"name": name, "purpose": "Works.", "character": "Calm.", "notes": "# Notes\n", "skills": [{"name": "dig", "description": "Digs.", "body": "Dig."}]});
    client(
        ClientMessage::FoundFrom(FoundFrom {
            id: id(),
            template: Template {
                agents: vec![settler("miller")],
                mayor: settler("lead"),
                name: "mill".into(),
                purpose: "Mill.".into(),
                title: "The Mill".into(),
            },
        }),
        json!({"type": "found-from", "id": ID, "template": {"name": "mill", "title": "The Mill", "purpose": "Mill.", "mayor": settler_wire("lead"), "agents": [settler_wire("miller")]}}),
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
