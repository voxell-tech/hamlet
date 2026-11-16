//! The realm actor's scenarios, run by `demo --realm-actor`.

use super::{Demo, Expect, Is, Kill, Probe, Read, Scenario};

const OPEN_OBJECTIONS: Probe = Probe {
    name: "realm.objections",
    file: "actors/realm/local.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM wait WHERE kind = 'objection' AND \
         closed_at IS NULL",
    ),
};

/// The letters and answers the realm keeps for a held task.
const HELD: Probe = Probe {
    name: "realm.held",
    file: "actors/realm/local.sqlite",
    read: Read::Sql("SELECT COUNT(*) FROM held"),
};

/// The letters hire 3 took.
const LETTERS: Probe = Probe {
    name: "agent.letters",
    file: "actors/agent/3.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM input WHERE kind = 'letter'",
    ),
};

/// The objection still waits: the hire may not spend on its task, and
/// the letter is kept, not given.
const STILL_HELD: &[Expect] = &[
    Expect {
        value: "may/3",
        is: Is::Exactly(0),
    },
    Expect {
        value: "realm.objections",
        is: Is::Exactly(1),
    },
    Expect {
        value: "realm.held",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent.letters",
        is: Is::Exactly(0),
    },
];

/// One go ends the wait, a second finds nothing waiting, and the
/// kept letter reaches the hire once.
const RULED_ONCE: &[Expect] = &[
    Expect {
        value: "may/3",
        is: Is::Exactly(0),
    },
    Expect {
        value: "answered/1",
        is: Is::Exactly(1),
    },
    Expect {
        value: "refused/1",
        is: Is::Exactly(1),
    },
    Expect {
        value: "took/3/letter",
        is: Is::Exactly(1),
    },
    Expect {
        value: "realm.objections",
        is: Is::Exactly(0),
    },
    Expect {
        value: "agent.letters",
        is: Is::Exactly(1),
    },
];

pub const SCENARIOS: &[Scenario] = &[Scenario {
    name: "realm-objection",
    about: "a hire objects to a chief's line, its mayor's letter on \
            that task is kept, and the host is killed at a random \
            time after; after each restart the objection's wait is \
            open and the hire's spending held, one go at the second \
            ends it, a second go finds nothing waiting, and the \
            letter arrives once",
    demo: Demo::RealmActor,
    setup: &[
        "realm found docs",
        "realm live 2",
        "realm hire 2 h1 write the docs",
        "realm live 3",
        "realm say 3 l1 tidy the index",
        "agent 3 object o1 line:3 better left alone",
    ],
    trigger: "agent 2 tell t1 hire#3 task=line:3 the file is in /tmp",
    trigger_replies: true,
    kill: Kill::After {
        min_ms: 0,
        max_ms: 500,
    },
    down_ms: 0,
    restart: [
        &["agent 3 may-spend line:3"],
        &[
            "agent 3 may-spend line:3",
            "realm answer 1 go",
            "realm answer 1 go",
            "agent 3 await letter 1",
        ],
    ],
    probes: &[OPEN_OBJECTIONS, HELD, LETTERS],
    expect: [STILL_HELD, RULED_ONCE],
}];
