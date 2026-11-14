//! The scenario table. A scenario is data: the lines the demo host
//! runs, when it is killed, how long it stays down, the lines each
//! restart runs, and what each restart must show. A new scenario is a
//! new row.

/// One way to kill the demo host and what must hold after it.
pub struct Scenario {
    pub name: &'static str,
    pub about: &'static str,
    /// Lines `demo serve` runs before the kill. Each waits for its
    /// reply, which acknowledges it.
    pub setup: &'static [&'static str],
    /// The line the kill follows.
    pub trigger: &'static str,
    /// Whether the trigger replies before the kill, so the kill
    /// comes after its acknowledgement.
    pub trigger_replies: bool,
    /// When the kit kills the host.
    pub kill: Kill,
    /// How long after the trigger was sent the first restart starts.
    pub down_ms: u64,
    /// Lines the first and the second restart's `demo serve` run
    /// after it boots.
    pub restart: [&'static [&'static str]; 2],
    /// Values read from the actor files once each restart exited.
    pub probes: &'static [Probe],
    /// The values the first and the second restart must show.
    pub expect: [&'static [Expect]; 2],
}

/// When the kit kills the host, once the trigger was sent and, for a
/// trigger that replies, its reply came.
pub enum Kill {
    /// At a fault point, given as `HAMLET_FAULT` is:
    /// `<point>[:<pass>]`. The host stops there and is killed.
    Fault(&'static str),
    /// At a time drawn at random from `min_ms` to `max_ms`, both
    /// included. The run's line records the time drawn.
    After { min_ms: u64, max_ms: u64 },
}

/// A value read from one actor file, under the realm folder.
pub struct Probe {
    pub name: &'static str,
    pub file: &'static str,
    pub read: Read,
}

/// How a probe reads its value.
pub enum Read {
    /// A query that returns one integer.
    Sql(&'static str),
    /// How many of the file, its write-ahead file and its
    /// shared-memory file exist.
    Files,
}

/// A value a restart must show: an actor's last reply, as
/// `counter/<key>`; the count of an actor its boot started, as
/// `boot counter/<key>`; or a probe, by its name.
pub struct Expect {
    pub value: &'static str,
    pub is: Is,
}

pub enum Is {
    /// The last count the actor acknowledged before the kill, plus
    /// this much.
    Acked(&'static str, i64),
    Exactly(i64),
    /// The restart showed it, with any value.
    Present,
    /// The restart did not show it.
    Absent,
}

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

/// Whether the command of `workspace-command` touched its file, which
/// it does only if it outlived the kill.
const LATE_FILE: Probe = Probe {
    name: "late.files",
    file: "actors/agent/1.fs/late",
    read: Read::Files,
};

/// The workspace's rows marked cut off.
const CUT_OFF_ROWS: Probe = Probe {
    name: "workspace.cut_off",
    file: "actors/workspace/local.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM calls WHERE state = 'cut_off'",
    ),
};

/// The ends agent 1 was told.
const TOLD: Probe = Probe {
    name: "agent.finished",
    file: "actors/agent/1.sqlite",
    read: Read::Sql("SELECT COUNT(*) FROM finished"),
};

/// The command is gone, its row reads cut off, a status by its id
/// answers so, and no one was told.
const COMMAND_CUT_OFF: &[Expect] = &[
    Expect {
        value: "status/1/cut_off",
        is: Is::Exactly(0),
    },
    Expect {
        value: "late.files",
        is: Is::Exactly(0),
    },
    Expect {
        value: "workspace.cut_off",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent.finished",
        is: Is::Exactly(0),
    },
];

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "counter-transaction",
        about: "kill inside an increment's open transaction, after \
                three acknowledged increments; state and SQL equal \
                the last acknowledged write",
        setup: &["increment a 3"],
        trigger: "increment a",
        trigger_replies: false,
        kill: Kill::Fault("tx-open:4"),
        down_ms: 0,
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
        setup: &["increment a 2"],
        trigger: "schedule a 1500",
        trigger_replies: true,
        kill: Kill::After {
            min_ms: 200,
            max_ms: 1000,
        },
        down_ms: 2000,
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
        setup: &["increment a 3"],
        trigger: "destroy a",
        trigger_replies: true,
        kill: Kill::Fault("run-ended"),
        down_ms: 0,
        restart: [&[], &["get a"]],
        probes: &[A_FILES],
        expect: [DESTROY_FINISHED, KEY_MADE_ANEW],
    },
    Scenario {
        name: "workspace-command",
        about: "kill the host while a command of the workspace \
                runs, after it answered Running; the command's \
                process is gone, and after each restart its row \
                reads cut off, a status by its id answers cut off, \
                and the agent was told nothing",
        setup: &["realm reach 1 1"],
        trigger: "agent 1 run c1 sleep 8; touch late",
        trigger_replies: true,
        kill: Kill::After {
            min_ms: 200,
            max_ms: 1000,
        },
        // Past the 8 s at which a command that outlived the kill
        // would touch its file, counted from the trigger.
        down_ms: 11_000,
        restart: [&["agent 1 status 1"], &["agent 1 status 1"]],
        probes: &[LATE_FILE, CUT_OFF_ROWS, TOLD],
        expect: [COMMAND_CUT_OFF, COMMAND_CUT_OFF],
    },
    Scenario {
        name: "boot-running",
        about: "kill at a random time while five counters run, each \
                after its acknowledged increments; the first \
                restart's boot starts all five with no call, each \
                reading its last acknowledged count, and the second \
                starts none",
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
        restart: [
            &[],
            &["get a", "get b", "get c", "get d", "get e"],
        ],
        probes: &[],
        expect: [FIVE_STARTED_AT_BOOT, FIVE_LEFT_STOPPED],
    },
];
