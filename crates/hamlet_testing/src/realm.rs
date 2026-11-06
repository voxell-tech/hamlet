//! Protocol values for tests of the window, and a loopback server
//! that sends them.

use futures::{SinkExt, StreamExt};
use hamlet_protocol::client as protocol;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio_tungstenite::tungstenite::Message;

/// A realm with its Hand and a first hamlet `home` under the mayor
/// `ada`, at revision zero.
pub fn state() -> protocol::State {
    serde_json::from_value(json!({
        "epoch": 1, "revision": 0, "chief": "chief", "confinement": {"kind": "sandbox", "name": "test"},
        "hand": 1, "hamlets": [hamlet("home", 2)],
        "agents": [hand(1), agent(2, "home", 0, "ada")],
        "scholars": [scholar(1, "ada", &[2])],
        "waits": [], "grants": [], "providers": [],
        "settings": {
            "model": "faux::faux-1", "reflectEvery": 0, "allowNet": false, "effort": null,
            "endpoints": [], "unset": false, "newcomers": "chief",
            "character": "A scholar new to this realm.",
            "link": {"state": "untested", "models": null, "words": null},
            "workspace": {"url": null, "running": null}
        },
        "limits": {
            "roadsBetween": true,
            "budget": {"period": "day", "cost": 5, "minutes": null}
        },
        "realm": "/tmp/hamlet-test", "library": [], "closed": [],
        "deeds": [], "starters": []
    })).unwrap()
}

/// A hamlet with the default limits and no folder, under the mayor
/// `mayor`.
pub fn hamlet(id: &str, mayor: u64) -> protocol::Hamlet {
    serde_json::from_value(json!({
        "id": id, "name": id, "folder": null, "before": [], "mode": "manual",
        "modeWords": "Hires ask you before each edit and command.",
        "roads": [], "at": null, "mayor": mayor, "tier": 50, "chiefs": ["chief"],
        "budget": {
            "period": "day", "periodWords": "today", "gone": 0,
            "spent": {"tokens": 0, "micros": 0, "ms": 0},
            "caps": {"micros": 5000000, "ms": null},
            "summary": "Hamlet today: $0.00 of $5.00", "lines": [],
            "left": 1, "remains": "$5.00"
        },
        "limits": {
            "budget": {"period": "day", "cost": 5, "minutes": null},
            "task": {"messages": 12, "cost": 1, "minutes": null},
            "road": {"price": 0.01, "most": 24, "growsOverDays": 7},
            "reserved": {"hire": false, "roads": false, "end": false, "merge": false},
            "models": [], "model": null, "depth": 3, "toldDays": 7, "protectedMinutes": 0,
            "asking": {"review": "remote", "questions": 3, "refusals": 3}
        }
    })).unwrap()
}

/// The Hand.
pub fn hand(id: u64) -> protocol::Agent {
    serde_json::from_value(json!({
        "id": id, "kind": "hand", "name": "hand", "hamlet": null, "number": null,
        "scholar": null, "parent": null, "pin": null, "purpose": "", "since": 0, "mayor": false,
        "model": "faux::faux-1", "busy": false, "faulted": false,
        "activity": {"doing": "rest", "doingWords": "At rest", "step": 0, "failures": 0},
        "saying": {"text": "", "thought": false},
        "spent": {"tokens": 0, "micros": 0, "ms": 0, "cost": "$0.00"},
        "consulting": false, "conversations": [], "skills": []
    })).unwrap()
}

/// A hire `name@hamlet`, its hamlet's mayor for number zero.
pub fn agent(
    id: u64,
    hamlet: &str,
    number: u64,
    name: &str,
) -> protocol::Agent {
    serde_json::from_value(json!({
        "id": id, "kind": "hire", "name": format!("{name}@{hamlet}"),
        "hamlet": hamlet, "number": number, "scholar": id,
        "parent": if number == 0 { json!(1) } else { json!(2) },
        "pin": null, "purpose": "", "since": 0, "mayor": number == 0,
        "model": "faux::faux-1", "busy": false, "faulted": false,
        "activity": {"doing": "rest", "doingWords": "At rest", "step": 0, "failures": 0},
        "saying": {"text": "", "thought": false},
        "spent": {"tokens": 0, "micros": 0, "ms": 0, "cost": "$0.00"},
        "consulting": false, "conversations": [], "skills": []
    })).unwrap()
}

/// A living scholar of the chief's with `hires`.
pub fn scholar(
    id: u64,
    name: &str,
    hires: &[u64],
) -> protocol::Scholar {
    serde_json::from_value(json!({
        "id": id, "name": name, "chief": "chief", "since": 0,
        "arrival": "housed", "arrivalBy": null,
        "house": {"x": 40.0, "z": 0.0, "turn": 0.0},
        "skills": [], "versions": [], "hires": hires
    }))
    .unwrap()
}

/// An approval raised by the first hamlet's mayor.
pub fn wait(id: &str) -> protocol::Wait {
    serde_json::from_value(json!({
        "id": id, "agent": 2, "by": "ada@home",
        "about": "pwd", "subject": "pwd", "change": null,
        "answers": ["allow", "always", "deny"], "since": 1000, "asked": 1,
        "chat": {"with": 2},
        "detail": {"kind": "approval", "danger": "command",
                   "says": "wants to run", "always": "session"}
    })).unwrap()
}

/// A laid road whose last sender is its first end.
pub fn road(
    a: protocol::AgentId,
    b: protocol::AgentId,
    messages: u64,
) -> protocol::Road {
    protocol::Road {
        a,
        b,
        kind: protocol::RoadKind::Laid,
        messages,
        level: protocol::RoadLevel::Paved,
        last_from: Some(a),
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
