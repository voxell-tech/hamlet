//! Protocol values for tests of the window, and a loopback server
//! that sends them.

use futures::{SinkExt, StreamExt};
use hamlet_protocol::client as protocol;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio_tungstenite::tungstenite::Message;

/// A realm with its Hand and a first hamlet `home` under the lead
/// `agent#2`, an agent of the profile `ada`, at revision zero.
pub fn state() -> protocol::State {
    serde_json::from_value(json!({
        "epoch": 1, "revision": 0, "chief": "chief", "confinement": {"kind": "sandbox", "name": "test"},
        "hand": 1, "hamlets": [hamlet("home", 2)],
        "agents": [hand(1), agent(2, "home", true)],
        "profiles": [profile(2, "ada", &[2])],
        "waits": [], "grants": [], "providers": [],
        "settings": {
            "model": "faux::faux-1", "reflectEvery": 0, "allowNet": false, "effort": null,
            "endpoints": [], "unset": false, "newcomers": "chief",
            "character": "A profile new to this realm.",
            "link": {"state": "untested", "models": null, "words": null},
            "workspace": {"url": null, "running": null}
        },
        "limits": {
            "channelsBetween": true,
            "budget": {"period": "day", "cost": 5, "minutes": null}
        },
        "realm": "/tmp/hamlet-test", "library": [], "closed": [],
        "actions": [], "terminals": []
    })).unwrap()
}

/// A hamlet with the default limits and no folder, under the lead
/// `lead`.
pub fn hamlet(id: &str, lead: u64) -> protocol::Hamlet {
    serde_json::from_value(json!({
        "id": id, "name": id, "folder": null, "before": [], "mode": "manual",
        "modeWords": "Agents ask you before each edit and command.",
        "channels": [], "at": null, "lead": lead, "tier": 50, "chiefs": ["chief"],
        "budget": {
            "period": "day", "periodWords": "today", "gone": 0,
            "spent": {"tokens": 0, "micros": 0, "ms": 0},
            "caps": {"micros": 5000000, "ms": null},
            "summary": "Hamlet today: $0.00 of $5.00", "lines": [],
            "left": 1, "remains": "$5.00"
        },
        "limits": {
            "budget": {"period": "day", "cost": 5, "minutes": null},
            "channel": {"price": 0.01, "most": 24, "growsOverDays": 7},
            "reserved": {"add": false, "channels": false, "end": false, "merge": false},
            "model": null, "toldDays": 7, "protectedMinutes": 0,
            "asking": {"review": "remote", "questions": 3, "refusals": 3}
        }
    })).unwrap()
}

/// The Hand.
pub fn hand(id: u64) -> protocol::Agent {
    serde_json::from_value(json!({
        "id": id, "kind": "hand", "address": "hand", "name": null, "hamlet": null,
        "profile": null, "parent": null, "pin": null, "purpose": "", "since": 0, "lead": false,
        "model": "faux::faux-1", "ownModel": null, "effort": null, "fast": false,
        "busy": false, "faulted": false,
        "activity": {"doing": "rest", "doingWords": "At rest", "step": 0, "failures": 0},
        "saying": {"text": "", "thought": false},
        "spent": {"tokens": 0, "micros": 0, "ms": 0, "cost": "$0.00"},
        "consulting": false, "conversations": [], "skills": []
    })).unwrap()
}

/// The agent `agent#id` of the profile with its own id, in `hamlet`:
/// its lead under the Hand, or an agent under the first lead.
pub fn agent(id: u64, hamlet: &str, lead: bool) -> protocol::Agent {
    serde_json::from_value(json!({
        "id": id, "kind": "hamlet", "address": format!("agent#{id}"), "name": null,
        "hamlet": hamlet, "profile": id,
        "parent": if lead { json!(1) } else { json!(2) },
        "pin": null, "purpose": "", "since": 0, "lead": lead,
        "model": "faux::faux-1", "ownModel": null, "effort": null, "fast": false,
        "busy": false, "faulted": false,
        "activity": {"doing": "rest", "doingWords": "At rest", "step": 0, "failures": 0},
        "saying": {"text": "", "thought": false},
        "spent": {"tokens": 0, "micros": 0, "ms": 0, "cost": "$0.00"},
        "consulting": false, "conversations": [], "skills": []
    })).unwrap()
}

/// A living profile of the chief's with `agents`.
pub fn profile(
    id: u64,
    name: &str,
    agents: &[u64],
) -> protocol::Profile {
    serde_json::from_value(json!({
        "id": id, "name": name, "chief": "chief", "since": 0,
        "arrival": "housed", "arrivalBy": null,
        "house": {"x": 40.0, "z": 0.0, "turn": 0.0},
        "skills": [], "versions": [], "agents": agents
    }))
    .unwrap()
}

/// An approval raised by the first hamlet's lead.
pub fn wait(id: &str) -> protocol::Wait {
    serde_json::from_value(json!({
        "id": id, "agent": 2, "by": "agent#2",
        "about": "pwd", "subject": "pwd", "change": null,
        "answers": ["allow", "always", "deny"], "since": 1000, "asked": 1,
        "chat": {"with": 2},
        "detail": {"kind": "approval", "danger": "command",
                   "says": "wants to run", "always": "session"}
    })).unwrap()
}

/// An opened channel whose last sender is its first end.
pub fn channel(
    a: protocol::AgentId,
    b: protocol::AgentId,
    messages: u64,
) -> protocol::Channel {
    protocol::Channel {
        a,
        b,
        kind: protocol::ChannelKind::Opened,
        messages,
        level: protocol::ChannelLevel::Paved,
        last_from: Some(a),
    }
}

/// The advertised actions named `names`.
pub fn actions(names: &[&str]) -> Vec<protocol::Action> {
    names
        .iter()
        .map(|name| protocol::Action {
            name: (*name).into(),
            usage: String::new(),
            description: String::new(),
        })
        .collect()
}

/// A loopback server on `runtime` for one window: it reads the
/// window's hello, sends `frames` and keeps the connection open. Its
/// `ws://` address.
pub fn serve(
    runtime: &Handle,
    frames: Vec<protocol::ServerMessage>,
) -> String {
    let listener =
        runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    runtime.spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        socket.next().await.unwrap().unwrap();
        for frame in frames {
            let text = serde_json::to_string(&frame).unwrap();
            socket.send(Message::text(text)).await.unwrap();
        }
        std::future::pending::<()>().await;
    });
    url
}
