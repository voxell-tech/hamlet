//! The realm actor's scenarios, run by `demo --realm-actor` with the
//! real agent on a faux model.

use super::agent::{TELL_RESULTS, TELLS_CUT_OFF};
use super::{
    Demo, Expect, Is, Kill, Probe, Read, Scenario, exactly, mayor,
    realm,
};

/// The model requests the realm charged the Hand.
pub(super) const HAND_CHARGES: Probe = realm(
    "realm.charges/1",
    "SELECT COUNT(*) FROM charge WHERE agent = 1",
);

/// The tasks the chief's lines to the mayor opened.
pub(super) const MAYOR_LINE_TASKS: Probe = realm(
    "realm.line_tasks/2",
    "SELECT COUNT(*) FROM task WHERE origin = 'chief' AND agent = 2",
);

/// The lines the mayor took.
const MAYOR_LINES: Probe = mayor(
    "agent.lines/2",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'hamlet.line'",
);

const LETTERS_TO_MAYOR: Probe = realm(
    "realm.letters/1/2",
    "SELECT COUNT(*) FROM letter WHERE sender = 1 AND receiver = 2",
);

/// The Hand's `tell` calls the realm ran, each kept once by its id.
const TELL_MARKERS: Probe = realm(
    "realm.tell_markers/1",
    "SELECT COUNT(*) FROM marker WHERE caller = 'agent/1' AND \
     action = 'tell'",
);

const OPEN_OBJECTIONS: Probe = realm(
    "realm.objections",
    "SELECT COUNT(*) FROM wait WHERE kind = 'objection' AND \
     closed_at IS NULL",
);

/// The letters and answers the realm keeps for a held task.
const HELD: Probe = realm("realm.held", "SELECT COUNT(*) FROM held");

/// The letters hire 3 took.
const LETTERS: Probe = Probe {
    name: "agent.letters",
    file: "actors/agent/3.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' \
         = 'hamlet.letter'",
    ),
};

/// The line said once, its repeat answered from what the realm kept.
const SAID_ONCE: &[Expect] = &[
    Expect {
        value: "taken/s1",
        is: Is::Present,
    },
    exactly("took/2/line", 1),
    exactly("realm.line_tasks/2", 1),
    exactly("agent.lines/2", 1),
];

/// The tell's model request and the one after its result, each
/// charged once; the letter given once.
const TOLD_ONCE: &[Expect] = &[
    exactly("took/2/letter", 1),
    exactly("realm.charges/1", 2),
    exactly("realm.letters/1/2", 1),
    exactly("realm.tell_markers/1", 1),
    exactly("agent.tell_results", 1),
    exactly("agent.tells_cut_off", 0),
];

/// The objection still waits: the hire may not spend on its task, and
/// the letter is kept, not given.
const STILL_HELD: &[Expect] = &[
    exactly("may/3", 0),
    exactly("realm.objections", 1),
    exactly("realm.held", 1),
    exactly("agent.letters", 0),
];

/// One go ends the wait and lets the hire spend again, a second
/// finds nothing waiting, and the realm gives the kept letter once.
const RULED_ONCE: &[Expect] = &[
    exactly("may/3", 1),
    exactly("answered/1", 1),
    exactly("refused/1", 1),
    exactly("took/3/letter", 1),
    exactly("realm.objections", 0),
    exactly("agent.letters", 1),
];

/// The mayor's answer to the chief's line, which a stop would cut
/// off.
const MAYOR_ANSWERED: &str = "until 2 1 assistant: Heard: hello";

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "realm-say",
        about: "kill the demo host once the realm committed a \
                chief's line to the mayor, before it answers; the \
                chief says the line again under the same request id \
                at each restart, and the realm holds one task for \
                it and the mayor one line",
        demo: Demo::RealmActor { faux: None },
        setup: &["realm found h1", "realm live 2", "pi pid"],
        trigger: "realm say 2 s1 hello",
        trigger_replies: false,
        kill: Kill::Fault("committed/say"),
        down_ms: 0,
        due: None,
        restart: [
            &[
                "pi pid",
                "realm say 2 s1 hello",
                "agent 2 await line 1",
                MAYOR_ANSWERED,
                "pi pid",
            ],
            &[
                "pi pid",
                "realm say 2 s1 hello",
                "agent 2 await line 1",
                MAYOR_ANSWERED,
                "pi pid",
            ],
        ],
        probes: &[MAYOR_LINE_TASKS, MAYOR_LINES],
        expect: [SAID_ONCE, SAID_ONCE],
    },
    Scenario {
        name: "realm-tell",
        about: "the Node host kills itself once the realm answered \
                the Hand's tell to the mayor, before the tool's \
                result is kept; the next Node host's start of the \
                Hand resumes the turn at the cut-off tell with no \
                call from outside, the tell runs again under the \
                same id, the realm answers it from what it kept and \
                holds the letter once, and the turn ends",
        demo: Demo::RealmActor { faux: None },
        setup: &[
            "realm found h1",
            "realm live 2",
            "pi pid",
            "pi die-after tell",
        ],
        trigger: "realm say 1 s1 tell 2: hello",
        trigger_replies: true,
        kill: Kill::NodeHost,
        down_ms: 0,
        due: None,
        restart: [
            &[
                "pi pid",
                "until 1 1 assistant: Done:",
                "agent 2 await letter 1",
                "until 2 1 assistant: Heard:",
                "pi pid",
            ],
            &[
                "pi pid",
                "until 1 1 assistant: Done:",
                "agent 2 await letter 1",
                "until 2 1 assistant: Heard:",
                "pi pid",
            ],
        ],
        probes: &[
            HAND_CHARGES,
            LETTERS_TO_MAYOR,
            TELL_MARKERS,
            TELL_RESULTS,
            TELLS_CUT_OFF,
        ],
        expect: [TOLD_ONCE, TOLD_ONCE],
    },
    Scenario {
        name: "realm-objection",
        about: "a hire objects to a chief's line, its mayor's \
                letter on that task is kept, and the host is killed \
                at a random time after; after each restart the \
                objection's wait is open and the hire's spending \
                held, one go at the second ends it and lets the \
                hire spend, a second go finds nothing waiting, and \
                the realm sends the letter once",
        demo: Demo::RealmActor { faux: None },
        setup: &[
            "realm found docs",
            "realm live 2",
            "realm hire 2 h1 write the docs",
            "realm live 3",
            "pi pid",
            "realm say 3 l1 tidy the index",
            "until 3 1 assistant: Heard: tidy the index",
            "agent 3 object o1 line:3 better left alone",
        ],
        trigger: "agent 2 tell t1 hire#3 task=line:3 the file is in \
                  /tmp",
        trigger_replies: true,
        kill: Kill::After {
            min_ms: 0,
            max_ms: 500,
        },
        down_ms: 0,
        due: None,
        restart: [
            &["pi pid", "agent 3 may-spend line:3", "pi pid"],
            &[
                "pi pid",
                "agent 3 may-spend line:3",
                "realm answer 1 go",
                "realm answer 1 go",
                "agent 3 await letter 1",
                "until 3 1 assistant: Heard: The chief rules",
                "until 3 1 assistant: Heard: A letter",
                "agent 3 may-spend line:3",
                "pi pid",
            ],
        ],
        probes: &[OPEN_OBJECTIONS, HELD, LETTERS],
        expect: [STILL_HELD, RULED_ONCE],
    },
];
