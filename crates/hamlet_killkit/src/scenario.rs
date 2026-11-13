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
    /// Lines each restart's `demo serve` runs after it boots.
    pub restart: &'static [&'static str],
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

/// A value read with SQL from one actor file, under the realm folder.
pub struct Probe {
    pub name: &'static str,
    pub file: &'static str,
    /// A query that returns one integer.
    pub sql: &'static str,
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
    sql: "SELECT COALESCE((SELECT value FROM counter WHERE id = 0), \
          0)",
};

/// The schedules counter `a` holds, read from its file.
const A_SCHEDULES: Probe = Probe {
    name: "a.schedules",
    file: "actors/counter/a.sqlite",
    sql: "SELECT COUNT(*) FROM _actor_schedule_events",
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
        restart: &["get a"],
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
        restart: &["get a"],
        probes: &[A_VALUE, A_SCHEDULES],
        expect: [SCHEDULE_FIRED_AT_BOOT, SCHEDULE_FIRED_ONCE],
    },
];
