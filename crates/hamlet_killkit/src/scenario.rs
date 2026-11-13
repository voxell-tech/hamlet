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
    /// How many restarts there are.
    pub restarts: Restarts,
    /// Values read from the actor files once each restart exited.
    pub probes: &'static [Probe],
    /// The values the first and the second restart must show, each
    /// as a list of groups.
    pub expect: [&'static [&'static [Expect]]; 2],
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
    /// The host ends by itself, by a signal, as an abort ends it. The
    /// kit only waits, and counts it as an abort.
    Exit,
}

/// How many times the kit restarts the host after the kill.
pub enum Restarts {
    /// Twice. Each restart must exit cleanly.
    Two,
    /// Until a restart exits cleanly, at most `max` times, then once
    /// more. A restart that ends by a signal is counted as an abort,
    /// and one that exits with an error code fails the run. The
    /// first clean restart is checked as the first, the one after it
    /// as the second, and each shows the aborts so far, the kill's
    /// own included, as `aborts`.
    UntilClean { max: u32 },
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
/// `boot counter/<key>`; the sends a drain left unconfirmed, as
/// `outbox counter/<key>`; the aborts, as `aborts`; or a probe, by
/// its name.
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

const A_FILE: &str = "actors/counter/a.sqlite";
const B_FILE: &str = "actors/counter/b.sqlite";

/// The `ledger` rows of counter `a`: the sends it committed.
const A_LEDGER: Probe = Probe {
    name: "a.ledger",
    file: A_FILE,
    sql: "SELECT COUNT(*) FROM ledger",
};

/// The sends of counter `a` that their receiver has not confirmed.
const A_OUTBOX: Probe = Probe {
    name: "a.outbox",
    file: A_FILE,
    sql: "SELECT COUNT(*) FROM _hamlet_outbox",
};

/// The count of counter `b`, read from its file.
const B_VALUE: Probe = Probe {
    name: "b.value",
    file: B_FILE,
    sql: "SELECT COALESCE((SELECT value FROM counter WHERE id = 0), \
          0)",
};

/// The `received` rows of counter `b`: the sends it applied.
const B_RECEIVED: Probe = Probe {
    name: "b.received",
    file: B_FILE,
    sql: "SELECT COUNT(*) FROM received",
};

/// The highest send counter `b` handled from its one sender.
const B_HANDLED: Probe = Probe {
    name: "b.handled",
    file: B_FILE,
    sql: "SELECT COALESCE(SUM(handled_in), 0) FROM _hamlet_inbound",
};

/// The runs that ended without success before the run that applied
/// each send, summed over `b`'s `received` rows.
const B_FAILED_RUNS: Probe = Probe {
    name: "b.failed_runs",
    file: B_FILE,
    sql: "SELECT COALESCE(SUM(failed_runs), 0) FROM received",
};

/// The deliveries counter `b` was answered a repeat for.
const B_REPEATS: Probe = Probe {
    name: "b.repeats",
    file: B_FILE,
    sql: "SELECT COUNT(*) FROM repeats",
};

/// The sends counter `b` set aside.
const B_DEAD_LETTERS: Probe = Probe {
    name: "b.dead_letters",
    file: B_FILE,
    sql: "SELECT COUNT(*) FROM _hamlet_dead_letters",
};

/// The failed runs of the sends counter `b` set aside, summed.
const B_DEAD_RUNS: Probe = Probe {
    name: "b.dead_runs",
    file: B_FILE,
    sql: "SELECT COALESCE(SUM(attempts), 0) FROM \
          _hamlet_dead_letters",
};

/// What every restart of a send scenario runs: wait until `a`'s
/// sends are confirmed, then read `b`.
const SEND_RESTART: &[&str] = &["drain a", "get b"];

/// The send of 5 from `a` to `b` is applied once and confirmed:
/// `b` counts 5 more and holds one `received` row, `a` one `ledger`
/// row and an empty outbox.
const SENT_ONCE: &[Expect] = &[
    Expect {
        value: "outbox counter/a",
        is: Is::Exactly(0),
    },
    Expect {
        value: "a.outbox",
        is: Is::Exactly(0),
    },
    Expect {
        value: "a.ledger",
        is: Is::Exactly(1),
    },
    Expect {
        value: "counter/b",
        is: Is::Acked("counter/b", 5),
    },
    Expect {
        value: "b.value",
        is: Is::Acked("counter/b", 5),
    },
    Expect {
        value: "b.received",
        is: Is::Exactly(1),
    },
    Expect {
        value: "b.handled",
        is: Is::Exactly(1),
    },
];

/// No delivery to `b` was answered as a repeat.
const NO_REPEAT: &[Expect] = &[Expect {
    value: "b.repeats",
    is: Is::Exactly(0),
}];

/// One delivery to `b` was answered as a repeat.
const ONE_REPEAT: &[Expect] = &[Expect {
    value: "b.repeats",
    is: Is::Exactly(1),
}];

/// The run that applied the send counts one run before it, the one
/// the kill cut.
const ONE_CUT_RUN: &[Expect] = &[Expect {
    value: "b.failed_runs",
    is: Is::Exactly(1),
}];

/// Nothing was sent: `a` holds no `ledger` row and an empty outbox,
/// and `b` never received.
const NOT_SENT: &[Expect] = &[
    Expect {
        value: "outbox counter/a",
        is: Is::Exactly(0),
    },
    Expect {
        value: "a.outbox",
        is: Is::Exactly(0),
    },
    Expect {
        value: "a.ledger",
        is: Is::Exactly(0),
    },
    Expect {
        value: "counter/b",
        is: Is::Acked("counter/b", 0),
    },
    Expect {
        value: "b.value",
        is: Is::Acked("counter/b", 0),
    },
    Expect {
        value: "b.received",
        is: Is::Exactly(0),
    },
    Expect {
        value: "b.handled",
        is: Is::Exactly(0),
    },
];

/// The probes of every send scenario.
const SEND_PROBES: &[Probe] = &[
    A_LEDGER,
    A_OUTBOX,
    B_VALUE,
    B_RECEIVED,
    B_HANDLED,
    B_FAILED_RUNS,
    B_REPEATS,
];

/// The poison send was set aside by `b` after the five aborts its
/// handler caused, and the host stayed up: `a` holds its `ledger`
/// row and an empty outbox, `b` applied nothing, moved past the
/// send, and holds one dead letter of five failed runs.
const POISON_SET_ASIDE: &[Expect] = &[
    Expect {
        value: "aborts",
        is: Is::Exactly(5),
    },
    Expect {
        value: "outbox counter/a",
        is: Is::Exactly(0),
    },
    Expect {
        value: "a.outbox",
        is: Is::Exactly(0),
    },
    Expect {
        value: "a.ledger",
        is: Is::Exactly(1),
    },
    Expect {
        value: "counter/b",
        is: Is::Acked("counter/b", 0),
    },
    Expect {
        value: "b.value",
        is: Is::Acked("counter/b", 0),
    },
    Expect {
        value: "b.received",
        is: Is::Exactly(0),
    },
    Expect {
        value: "b.handled",
        is: Is::Exactly(1),
    },
    Expect {
        value: "b.dead_letters",
        is: Is::Exactly(1),
    },
    Expect {
        value: "b.dead_runs",
        is: Is::Exactly(5),
    },
];

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
        restarts: Restarts::Two,
        probes: &[A_VALUE],
        expect: [&[COUNTER_TRANSACTION], &[COUNTER_TRANSACTION]],
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
        restarts: Restarts::Two,
        probes: &[A_VALUE, A_SCHEDULES],
        expect: [&[SCHEDULE_FIRED_AT_BOOT], &[SCHEDULE_FIRED_ONCE]],
    },
    Scenario {
        name: "send-deliver",
        about: "kill once a send's transaction committed, before \
                its delivery starts; it is delivered once, with no \
                repeat",
        setup: &["get b"],
        trigger: "send a b",
        trigger_replies: false,
        kill: Kill::Fault("send-committed"),
        down_ms: 0,
        restart: SEND_RESTART,
        restarts: Restarts::Two,
        probes: SEND_PROBES,
        expect: [&[SENT_ONCE, NO_REPEAT], &[SENT_ONCE, NO_REPEAT]],
    },
    Scenario {
        name: "send-before-commit",
        about: "kill inside the sender's transaction, after its \
                send; nothing is sent and the receiver never \
                receives",
        setup: &["get b"],
        trigger: "send a b",
        trigger_replies: false,
        kill: Kill::Fault("send-tx-open"),
        down_ms: 0,
        restart: SEND_RESTART,
        restarts: Restarts::Two,
        probes: SEND_PROBES,
        expect: [&[NOT_SENT, NO_REPEAT], &[NOT_SENT, NO_REPEAT]],
    },
    Scenario {
        name: "send-receiver-mid",
        about: "kill inside the receiver's receive transaction; the \
                send is applied once, after one cut run",
        setup: &["get b"],
        trigger: "send a b",
        trigger_replies: false,
        kill: Kill::Fault("receive-tx-open"),
        down_ms: 0,
        restart: SEND_RESTART,
        restarts: Restarts::Two,
        probes: SEND_PROBES,
        expect: [
            &[SENT_ONCE, ONE_CUT_RUN, NO_REPEAT],
            &[SENT_ONCE, ONE_CUT_RUN, NO_REPEAT],
        ],
    },
    Scenario {
        name: "send-before-ack",
        about: "kill after the receiver applied a send, before the \
                sender confirms it; the first restart delivers it \
                again as one repeat, the second delivers nothing",
        setup: &["get b"],
        trigger: "send a b",
        trigger_replies: false,
        kill: Kill::Fault("send-acked"),
        down_ms: 0,
        restart: SEND_RESTART,
        restarts: Restarts::Two,
        probes: SEND_PROBES,
        expect: [&[SENT_ONCE, ONE_REPEAT], &[SENT_ONCE, ONE_REPEAT]],
    },
    Scenario {
        name: "poison",
        about: "a send whose handler aborts the host at every run; \
                the host is restarted until it stays up, after five \
                aborts, with the send set aside by its receiver",
        setup: &["get b"],
        trigger: "poison a b",
        trigger_replies: false,
        kill: Kill::Exit,
        down_ms: 0,
        restart: SEND_RESTART,
        restarts: Restarts::UntilClean { max: 8 },
        probes: &[
            A_LEDGER,
            A_OUTBOX,
            B_VALUE,
            B_RECEIVED,
            B_HANDLED,
            B_DEAD_LETTERS,
            B_DEAD_RUNS,
        ],
        expect: [&[POISON_SET_ASIDE], &[POISON_SET_ASIDE]],
    },
];
