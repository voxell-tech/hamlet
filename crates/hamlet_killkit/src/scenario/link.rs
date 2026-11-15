use super::{Demo, Expect, Is, Kill, Probe, Read, Scenario};

/// The value that holds the Node host's process id in the linked
/// scenarios: agent 1's answer to `pid`.
const NODE_PID: &str = "agent/1/pid";

const NOTES: Probe = Probe {
    name: "agent.notes",
    file: "actors/agent/1.sqlite",
    read: Read::Sql("SELECT COUNT(*) FROM notes"),
};

const JOBS: Probe = Probe {
    name: "agent.jobs",
    file: "actors/agent/1.sqlite",
    read: Read::Sql("SELECT COUNT(*) FROM jobs"),
};

const FIRED: Probe = Probe {
    name: "agent.fired",
    file: "actors/agent/1.sqlite",
    read: Read::Sql("SELECT COUNT(*) FROM fired"),
};

const ONE_SHOTS: Probe = Probe {
    name: "agent.one_shots",
    file: "actors/agent/1.sqlite",
    read: Read::Sql("SELECT COUNT(*) FROM _actor_schedule_events"),
};

/// One call failed, with `link.lost`. The new Node host's start of
/// the agent finished the job the call had kept, with one note, and
/// told the realm once; the next call added one more note.
const LOST_ONCE_THEN_RESUMED: &[Expect] = &[
    Expect {
        value: "error link.lost",
        is: Is::Exactly(1),
    },
    Expect {
        value: "heard/1",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent/1/note",
        is: Is::Acked("agent/1/note", 2),
    },
    Expect {
        value: "agent.notes",
        is: Is::Acked("agent/1/note", 2),
    },
    Expect {
        value: "agent.jobs",
        is: Is::Exactly(0),
    },
];

/// Both hosts stopped and started again: the same notes, and the job
/// heard of once.
const RESUMED_ONCE: &[Expect] = &[
    Expect {
        value: "heard/1",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent/1/count",
        is: Is::Acked("agent/1/note", 2),
    },
    Expect {
        value: "agent.notes",
        is: Is::Acked("agent/1/note", 2),
    },
    Expect {
        value: "agent.jobs",
        is: Is::Exactly(0),
    },
];

/// The acknowledged notes, and the one whose commit had returned in
/// the Node host when the Rust host was killed.
const COMMITTED_NOTES_KEPT: &[Expect] = &[
    Expect {
        value: "agent/1/count",
        is: Is::Acked("agent/1/note", 1),
    },
    Expect {
        value: "agent.notes",
        is: Is::Acked("agent/1/note", 1),
    },
];

const ONE_SHOT_FIRED_ONCE: &[Expect] = &[
    Expect {
        value: "agent/1/fired",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent.fired",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent.one_shots",
        is: Is::Exactly(0),
    },
];

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "link-kill-node",
        about: "the Node host kills itself in a call from the \
                realm, once the call's action has kept a job and \
                before it answers; the call fails with link.lost, \
                the Rust host starts a new Node host, whose start \
                of the agent finishes the job and tells the realm \
                with no call from outside, and calls answer again",
        demo: Demo::Linked { pid: NODE_PID },
        setup: &[
            "make 1",
            "tell 1 note",
            "tell 1 note",
            "tell 1 note",
            "tell 1 pid",
        ],
        trigger: "tell 1 begin",
        trigger_replies: false,
        kill: Kill::NodeHost,
        down_ms: 0,
        restart: [
            &["link", "heard 1 10000", "tell 1 note", "tell 1 pid"],
            &["heard 1 0", "tell 1 count", "tell 1 pid"],
        ],
        probes: &[NOTES, JOBS],
        expect: [LOST_ONCE_THEN_RESUMED, RESUMED_ONCE],
    },
    Scenario {
        name: "link-kill-rust",
        about: "kill the Rust host once its realm's fourth call to \
                an agent has answered and before the realm answers; \
                the Node host is gone within 2 s, the next start \
                runs a Node host of its own, and the agent holds \
                the three acknowledged notes and the fourth",
        demo: Demo::Linked { pid: NODE_PID },
        setup: &[
            "make 1",
            "tell 1 note",
            "tell 1 note",
            "tell 1 note",
            "tell 1 pid",
        ],
        trigger: "tell 1 note",
        trigger_replies: false,
        kill: Kill::Fault("told/note:4"),
        down_ms: 0,
        restart: [
            &["tell 1 count", "tell 1 pid"],
            &["tell 1 count", "tell 1 pid"],
        ],
        probes: &[NOTES],
        expect: [COMMITTED_NOTES_KEPT, COMMITTED_NOTES_KEPT],
    },
    Scenario {
        name: "ts-schedule",
        about: "kill the Rust host once a TypeScript agent has \
                armed a one-shot 3 s ahead, and start it again \
                after the one-shot was due with both hosts down; it \
                fires once at the first start, and not again at the \
                second",
        demo: Demo::Linked { pid: NODE_PID },
        setup: &["make 1", "tell 1 pid"],
        trigger: "tell 1 arm 3000",
        trigger_replies: false,
        kill: Kill::Fault("told/arm"),
        down_ms: 4500,
        restart: [
            &["tell 1 fired 1 10000", "tell 1 pid"],
            &["tell 1 fired 2 500", "tell 1 pid"],
        ],
        probes: &[FIRED, ONE_SHOTS],
        expect: [ONE_SHOT_FIRED_ONCE, ONE_SHOT_FIRED_ONCE],
    },
];
