//! Protocol values for tests of the window.

use hamlet_protocol::client as protocol;
use serde_json::json;

/// A realm with its first hamlet and founder, at revision zero.
pub fn state() -> protocol::State {
    serde_json::from_value(json!({
        "epoch": 1, "revision": 0, "chief": "chief", "sandbox": "test",
        "hamlets": [hamlet("home")], "agents": [agent("home", 0, "founder")],
        "waits": [], "grants": [], "providers": [],
        "settings": {
            "model": "faux::faux-1", "reflectEvery": 0, "allowNet": false,
            "endpoints": [], "unset": false,
            "link": {"state": "untested", "models": null, "words": null},
            "workspace": {"url": null, "running": null}
        },
        "houses": "/tmp/hamlet-test/houses", "library": [], "closed": [],
        "deeds": [], "starters": []
    })).unwrap()
}

/// A hamlet with the default limits and no folder.
pub fn hamlet(id: &str) -> protocol::Hamlet {
    serde_json::from_value(json!({
        "id": id, "name": id, "folder": null, "before": [], "mode": "manual",
        "modeWords": "Agents ask you before each edit and command.",
        "roads": [], "votes": [], "at": null,
        "budget": {
            "period": "day", "periodWords": "today", "gone": 0,
            "spent": {"tokens": 0, "micros": 0, "ms": 0},
            "caps": {"micros": 5000000, "ms": null},
            "summary": "Hamlet today: $0.00 of $5.00", "lines": [],
            "left": 1, "remains": "$5.00"
        },
        "limits": {
            "votes": {
                "passYes": 2, "passOf": 3, "leastVoters": 3, "sealed": true, "groundedOnly": false,
                "ballotMinutes": 5, "openMinutes": 60, "retryMinutes": 60
            },
            "hamlets": {"most": 6, "roadsBetween": true, "removalMayors": 3},
            "budget": {"period": "day", "cost": 5, "minutes": null, "tokens": null},
            "task": {"messages": 12, "cost": 1, "minutes": null, "tokens": null},
            "road": {"tokens": 0, "cost": 0.01, "minutes": 0, "growsOverDays": 7},
            "mostAgents": 12, "mostRoads": 24, "models": [], "model": null, "pinned": {},
            "mayor": {"createAgents": true, "buildRoads": true, "removeAgents": true, "mergeAgents": true, "toldDays": 7},
            "leaving": {"protectedMinutes": 0, "mergedNotes": 6000},
            "asking": {"mayorReview": "remote", "questions": 3, "refusals": 3},
            "permissions": {"network": [], "unconfined": []}
        }
    })).unwrap()
}

/// An agent whose mayor flag is set for number zero.
pub fn agent(
    hamlet: &str,
    number: i64,
    name: &str,
) -> protocol::Agent {
    serde_json::from_value(json!({
        "ref": {"hamlet": hamlet, "number": number}, "name": name,
        "purpose": "", "since": 0, "mayor": number == 0,
        "model": "faux::faux-1", "busy": false, "faulted": false,
        "activity": {"doing": "rest", "doingWords": "At rest", "step": 0, "failures": 0},
        "saying": {"text": "", "thought": false},
        "spent": {"tokens": 0, "micros": 0, "ms": 0, "cost": "$0.00"},
        "consulting": false, "conversations": [], "skills": []
    })).unwrap()
}

/// An approval raised by the first hamlet's founder.
pub fn wait(id: &str) -> protocol::Wait {
    serde_json::from_value(json!({
        "id": id, "agent": {"hamlet": "home", "number": 0}, "by": "founder",
        "about": "pwd", "subject": "pwd", "change": null,
        "answers": ["allow", "always", "deny"], "since": 1000, "asked": 1,
        "chat": {"with": {"hamlet": "home", "agent": "mayor"}},
        "detail": {"kind": "approval", "danger": "command",
                   "says": "wants to run", "always": "session"}
    })).unwrap()
}

/// A road whose last sender is its first end.
pub fn road(
    a: protocol::AgentRef,
    b: protocol::AgentRef,
    messages: u64,
) -> protocol::Road {
    protocol::Road {
        a: a.clone(),
        b,
        messages,
        level: protocol::RoadLevel::Paved,
        last_from: Some(a),
    }
}

/// An agent's stable reference within a hamlet.
pub fn reference(hamlet: &str, number: u64) -> protocol::AgentRef {
    protocol::AgentRef {
        hamlet: hamlet.into(),
        number,
    }
}

/// The advertised deeds named `names`.
pub fn deeds(names: &[&str]) -> Vec<protocol::Deed> {
    names
        .iter()
        .map(|name| protocol::Deed {
            name: (*name).into(),
            usage: String::new(),
            description: String::new(),
        })
        .collect()
}
