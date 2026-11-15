use super::{Demo, Expect, Is, Kill, Probe, Read, Scenario};

/// The count of counter `a`, read from its file.
const A_VALUE: Probe = Probe {
    name: "a.value",
    file: "actors/counter/a.sqlite",
    read: Read::Sql(
        "SELECT COALESCE((SELECT value FROM counter WHERE id = 0), \
         0)",
    ),
};

/// The schedules counter `a` holds, read from its file.
const A_SCHEDULES: Probe = Probe {
    name: "a.schedules",
    file: "actors/counter/a.sqlite",
    read: Read::Sql("SELECT COUNT(*) FROM _actor_schedule_events"),
};

/// How many of counter `a`'s files exist.
const A_FILES: Probe = Probe {
    name: "a.files",
    file: "actors/counter/a.sqlite",
    read: Read::Files,
};

const COUNTER_TRANSACTION: &[Expect] = &[
    Expect {
        value: "counter/a",
        is: Is::Acked("counter/a", 0),
    },
    Expect {
        value: "a.value",
        is: Is::Acked("counter/a", 0),
    },
];

/// The first start after the one-shot came due is boot's own, and
/// the one-shot fired in it. The start runs the one-shot on a task of
/// its own, so the replies may come before it; the file, read once
/// the host stopped, holds its end.
const SCHEDULE_FIRED_AT_BOOT: &[Expect] = &[
    Expect {
        value: "boot counter/a",
        is: Is::Present,
    },
    Expect {
        value: "a.value",
        is: Is::Acked("counter/a", 1),
    },
    Expect {
        value: "a.schedules",
        is: Is::Exactly(0),
    },
];

/// The one-shot fired once: the next boot starts nothing.
const SCHEDULE_FIRED_ONCE: &[Expect] = &[
    Expect {
        value: "boot counter/a",
        is: Is::Absent,
    },
    Expect {
        value: "counter/a",
        is: Is::Acked("counter/a", 1),
    },
    Expect {
        value: "a.value",
        is: Is::Acked("counter/a", 1),
    },
    Expect {
        value: "a.schedules",
        is: Is::Exactly(0),
    },
];

/// The boot finished the destroy: it started nothing, and removed the
/// actor's three files.
const DESTROY_FINISHED: &[Expect] = &[
    Expect {
        value: "boot counter/a",
        is: Is::Absent,
    },
    Expect {
        value: "a.files",
        is: Is::Exactly(0),
    },
];

/// A new start of the key reads none of the destroyed actor's data.
const KEY_MADE_ANEW: &[Expect] = &[
    Expect {
        value: "boot counter/a",
        is: Is::Absent,
    },
    Expect {
        value: "counter/a",
        is: Is::Exactly(0),
    },
];

/// Boot started each of the five counters a kill left running, with
/// no call, and each read its last acknowledged count.
const FIVE_STARTED_AT_BOOT: &[Expect] = &[
    Expect {
        value: "boot counter/a",
        is: Is::Acked("counter/a", 0),
    },
    Expect {
        value: "boot counter/b",
        is: Is::Acked("counter/b", 0),
    },
    Expect {
        value: "boot counter/c",
        is: Is::Acked("counter/c", 0),
    },
    Expect {
        value: "boot counter/d",
        is: Is::Acked("counter/d", 0),
    },
    Expect {
        value: "boot counter/e",
        is: Is::Acked("counter/e", 0),
    },
];

/// The first restart stopped them cleanly: the next boot starts
/// none, and each still reads its last acknowledged count.
const FIVE_LEFT_STOPPED: &[Expect] = &[
    Expect {
        value: "boot counter/a",
        is: Is::Absent,
    },
    Expect {
        value: "boot counter/b",
        is: Is::Absent,
    },
    Expect {
        value: "boot counter/c",
        is: Is::Absent,
    },
    Expect {
        value: "boot counter/d",
        is: Is::Absent,
    },
    Expect {
        value: "boot counter/e",
        is: Is::Absent,
    },
    Expect {
        value: "counter/a",
        is: Is::Acked("counter/a", 0),
    },
    Expect {
        value: "counter/b",
        is: Is::Acked("counter/b", 0),
    },
    Expect {
        value: "counter/c",
        is: Is::Acked("counter/c", 0),
    },
    Expect {
        value: "counter/d",
        is: Is::Acked("counter/d", 0),
    },
    Expect {
        value: "counter/e",
        is: Is::Acked("counter/e", 0),
    },
];

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "counter-transaction",
        about: "kill inside an increment's open transaction, after \
                three acknowledged increments; state and SQL equal \
                the last acknowledged write",
        demo: Demo::Local,
        setup: &["increment a 3"],
        trigger: "increment a",
        trigger_replies: false,
        kill: Kill::Fault("tx-open:4"),
        down_ms: 0,
        due: None,
        restart: [&["get a"], &["get a"]],
        probes: &[A_VALUE],
        expect: [COUNTER_TRANSACTION, COUNTER_TRANSACTION],
    },
    Scenario {
        name: "schedule-before-due",
        about: "kill at a random time after a one-shot's schedule \
                is acknowledged and before it is due, and restart \
                after it is due; it fires exactly once, at the \
                first start",
        demo: Demo::Local,
        setup: &["increment a 2"],
        trigger: "schedule a 1500",
        trigger_replies: true,
        kill: Kill::After {
            min_ms: 200,
            max_ms: 1000,
        },
        down_ms: 2000,
        due: None,
        restart: [&["get a"], &["get a"]],
        probes: &[A_VALUE, A_SCHEDULES],
        expect: [SCHEDULE_FIRED_AT_BOOT, SCHEDULE_FIRED_ONCE],
    },
    Scenario {
        name: "destroy-cut",
        about: "kill after a destroy is acknowledged, as its stop \
                marks the run ended and before the actor's files \
                are removed; the first restart finishes the destroy \
                and starts nothing, and a new start of the key at \
                the second reads 0",
        demo: Demo::Local,
        setup: &["increment a 3"],
        trigger: "destroy a",
        trigger_replies: true,
        kill: Kill::Fault("run-ended"),
        down_ms: 0,
        due: None,
        restart: [&[], &["get a"]],
        probes: &[A_FILES],
        expect: [DESTROY_FINISHED, KEY_MADE_ANEW],
    },
    Scenario {
        name: "boot-running",
        about: "kill at a random time while five counters run, each \
                after its acknowledged increments; the first \
                restart's boot starts all five with no call, each \
                reading its last acknowledged count, and the second \
                starts none",
        demo: Demo::Local,
        setup: &[
            "increment a 1",
            "increment b 2",
            "increment c 3",
            "increment d 4",
        ],
        trigger: "increment e 5",
        trigger_replies: true,
        kill: Kill::After {
            min_ms: 50,
            max_ms: 500,
        },
        down_ms: 0,
        due: None,
        restart: [
            &[],
            &["get a", "get b", "get c", "get d", "get e"],
        ],
        probes: &[],
        expect: [FIVE_STARTED_AT_BOOT, FIVE_LEFT_STOPPED],
    },
];
