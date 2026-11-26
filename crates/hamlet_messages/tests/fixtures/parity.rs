//! The fixtures of what a client reads of the realm, the library, a
//! profile and an agent, and of a chief's line with attachments.

use hamlet_messages::agent::chat::{
    CheckConnection, EditMemory, ImagesTaken, ProviderList,
    ReadPapers, ReadProviders, TakenImages,
};
use hamlet_messages::agent::{LineFile, LineImage};
use hamlet_messages::library::Shelved;
use hamlet_messages::realm::client::{
    ReadState, RememberSecret, World,
};
use hamlet_messages::realm::parcels::{ParcelFile, ParcelId};
use hamlet_messages::realm::world::Revision;
use hamlet_protocol::client::{
    self as protocol, AttachedFile, AttachedImage, LoginMethod,
};

use super::Fixtures;
use super::cases::{Cases, named};

const IMAGE: &str =
    "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
const FILE: &str =
    "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae";

pub fn enums() -> Vec<Cases> {
    vec![named::<LoginMethod>()]
}

pub fn line_image() -> LineImage {
    LineImage {
        image: AttachedImage {
            at: Some(4),
            id: IMAGE.parse().unwrap(),
        },
        file: ParcelFile {
            parcel: ParcelId(7),
            file: 0,
            path: format!("images/{IMAGE}.png").into(),
            size: 2048,
        },
    }
}

pub fn line_file() -> LineFile {
    LineFile {
        attached: AttachedFile {
            at: None,
            id: FILE.parse().unwrap(),
            name: "notes/plan.md".into(),
            size: 512,
        },
        file: ParcelFile {
            parcel: ParcelId(7),
            file: 1,
            path: "notes/plan.md".into(),
            size: 512,
        },
    }
}

pub fn what_a_client_reads(fixtures: &mut Fixtures) {
    fixtures.args("realm/read_state", &ReadState);
    fixtures.args(
        "realm/remember_secret",
        &RememberSecret {
            secret: "sk-typed-at-a-sign-in".to_owned(),
        },
    );
    fixtures.protocol_answer("realm/read_state", &state());
    fixtures.answer(
        "realm/world",
        &World {
            revision: Revision(43),
        },
    );
    fixtures.answer("library/shelved", &Shelved { version: 3 });
    fixtures.args("agent/check_connection", &CheckConnection);
    fixtures.args("agent/images_taken", &ImagesTaken);
    fixtures.answer(
        "agent/images_taken",
        &TakenImages { per_line: Some(20) },
    );
    fixtures.answer(
        "agent/images_taken.none",
        &TakenImages { per_line: None },
    );
    fixtures.args("agent/read_providers", &ReadProviders);
    fixtures.answer(
        "agent/read_providers",
        &ProviderList {
            providers: vec![protocol::ProviderView {
                configured: true,
                id: "anthropic".into(),
                label: "Anthropic".into(),
                methods: vec![
                    LoginMethod::Key,
                    LoginMethod::Subscription,
                ],
                name: "Anthropic".into(),
            }],
        },
    );
    fixtures.args("agent/read_papers", &ReadPapers);
    fixtures.answer(
        "agent/read_papers",
        &protocol::Papers {
            character: "Careful with numbers.".into(),
            learned: vec!["memory/units.md".into()],
            memory: vec![protocol::MemoryFile {
                description: "Which units the realm uses".into(),
                path: "units.md".into(),
                text: "---\ndescription: Which units the realm \
                       uses\n---\nMetric.\n"
                    .into(),
            }],
            skills: vec![protocol::Skill {
                description: "Search the web".into(),
                name: "web-search".into(),
            }],
        },
    );
    fixtures.args(
        "agent/edit_memory",
        &EditMemory {
            path: "memory/units.md".into(),
            text: "Metric.".into(),
        },
    );
    fixtures.answer("agent/edit_memory", &());
}

fn state() -> protocol::State {
    serde_json::from_str(STATE).unwrap()
}

const STATE: &str = r#"{
    "agents": [
        {
            "activity": {
                "doing": "rest",
                "doingWords": "At rest",
                "failures": 0,
                "step": 0
            },
            "busy": false,
            "consulting": false,
            "conversations": [],
            "effort": null,
            "fast": false,
            "faulted": false,
            "hamlet": null,
            "id": 1,
            "kind": "hand",
            "mayor": false,
            "model": "faux::faux-1",
            "name": "hand",
            "nickname": null,
            "ownModel": null,
            "parent": null,
            "pin": null,
            "purpose": "",
            "saying": {
                "text": "",
                "thought": false
            },
            "scholar": 1,
            "since": 1790000000000,
            "skills": [],
            "spent": {
                "cost": "$0.00",
                "micros": 0,
                "ms": 0,
                "tokens": 0
            }
        }
    ],
    "chief": "chief",
    "closed": [],
    "confinement": {
        "kind": "sandbox",
        "name": "bwrap"
    },
    "deeds": [
        {
            "description": "Talk to the Hand",
            "name": "hand",
            "usage": ""
        }
    ],
    "epoch": 0,
    "grants": [],
    "hamlets": [],
    "hand": 1,
    "library": [],
    "limits": {
        "budget": {
            "cost": 50.0,
            "minutes": null,
            "period": "day"
        },
        "roadsBetween": true
    },
    "providers": [],
    "realm": "/realm",
    "revision": 42,
    "scholars": [],
    "settings": {
        "allowNet": false,
        "character": "You are new to this realm.",
        "effort": null,
        "endpoints": [],
        "link": {
            "models": null,
            "state": "untested",
            "words": null
        },
        "model": "",
        "newcomers": "chief",
        "reflectEvery": 0,
        "unset": true,
        "workspace": {
            "running": null,
            "url": null
        }
    },
    "waits": []
}"#;
