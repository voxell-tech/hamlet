//! The real agent on a faux model, against the demo's stand-in realm.
//! Each scenario's first line is a `pi` command, which fails unless
//! `HAMLET_DEMO_AGENT` is `pi`.

use super::{Demo, Expect, Is, Kill, Probe, Read, Scenario};

/// The value that holds the Node host's process id: `pi pid`'s
/// answer.
const NODE_PID: &str = "node/pid";

const TELL_RESULTS: Probe = Probe {
    name: "agent.tell_results",
    file: "actors/agent/1.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' \
         = 'pi.tool-result' AND record ->> '$.model[0].toolName' = \
         'tell'",
    ),
};

/// A `tell` result the model was told was cut off: a repeat the realm
/// refused, rather than answered from what it kept.
const TELLS_CUT_OFF: Probe = Probe {
    name: "agent.tells_cut_off",
    file: "actors/agent/1.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' \
         = 'pi.tool-result' AND record ->> '$.model[0].toolName' = \
         'tell' AND record ->> '$.model[0].content[0].text' LIKE \
         '%was cut off%'",
    ),
};

const TELLS_KEPT: Probe = Probe {
    name: "realm.tells",
    file: "actors/realm/local.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM acts WHERE action = 'tell'",
    ),
};

const EDITS_CUT_OFF: Probe = Probe {
    name: "agent.edits_cut_off",
    file: "actors/agent/1.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' \
         = 'pi.tool-result' AND record ->> '$.model[0].toolName' = \
         'edit' AND record ->> '$.model[0].content[0].text' LIKE \
         '%Its outcome is unknown%'",
    ),
};

const EDITS: Probe = Probe {
    name: "agent.edits",
    file: "actors/agent/1.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' \
         = 'pi.tool-result' AND record ->> '$.model[0].toolName' = \
         'edit'",
    ),
};

const WAKES: Probe = Probe {
    name: "agent.wakes",
    file: "actors/agent/1.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' \
         = 'hamlet.wake'",
    ),
};

/// The tell's model request and the one after its result, each
/// charged once.
const TOLD_ONCE: &[Expect] = &[
    Expect {
        value: "charges/1",
        is: Is::Exactly(2),
    },
    Expect {
        value: "calls/tell",
        is: Is::Exactly(2),
    },
    Expect {
        value: "acts/tell",
        is: Is::Exactly(1),
    },
    Expect {
        value: "realm.tells",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent.tell_results",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent.tells_cut_off",
        is: Is::Exactly(0),
    },
];

/// The write's two requests, the edit's request and the one after
/// the edit's result.
const EDIT_CUT_OFF: &[Expect] = &[
    Expect {
        value: "charges/1",
        is: Is::Exactly(4),
    },
    Expect {
        value: "calls/tell",
        is: Is::Absent,
    },
    Expect {
        value: "agent.edits",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent.edits_cut_off",
        is: Is::Exactly(1),
    },
];

/// The wake's two requests and the one the timer's input began.
const WOKEN_ONCE: &[Expect] = &[
    Expect {
        value: "charges/1",
        is: Is::Exactly(3),
    },
    Expect {
        value: "agent.wakes",
        is: Is::Exactly(1),
    },
];

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "agent-tell",
        about: "the stand-in realm kills the Node host once it kept \
                the agent's tell, before it answers; the next Node \
                host's start of the agent resumes the turn at the \
                cut-off tell with no call from outside, the tell \
                runs again under the same task id, the realm \
                answers it from what it kept and holds the letter \
                once, and the turn ends",
        demo: Demo::Linked { pid: NODE_PID },
        setup: &["pi make 1", "pi pid", "pi die-in-tell"],
        trigger: "pi say 1 tell 2: hello",
        trigger_replies: true,
        kill: Kill::NodeHost,
        down_ms: 0,
        restart: [
            &["pi charges 1 2 30000", "pi recorded", "pi pid"],
            &["pi charges 1 2 3000", "pi recorded", "pi pid"],
        ],
        probes: &[TELL_RESULTS, TELLS_CUT_OFF, TELLS_KEPT],
        expect: [TOLD_ONCE, TOLD_ONCE],
    },
    Scenario {
        name: "agent-mid-turn",
        about: "the Node host kills itself in the agent's edit of a \
                file of its own, once the edit is marked begun and \
                before the file is written; the next Node host's \
                start of the agent resumes the turn at the cut-off \
                edit with no call from outside, does not run the \
                edit again, tells the model its outcome is unknown, \
                and the turn ends",
        demo: Demo::Linked { pid: NODE_PID },
        setup: &[
            "pi make 1",
            "pi pid",
            "pi say 1 write own:memory/notes.md: draft",
            "pi charges 1 2 20000",
        ],
        trigger: "pi say 1 die in edit own:memory/notes.md: draft \
                  -> final",
        trigger_replies: true,
        kill: Kill::NodeHost,
        down_ms: 0,
        restart: [
            &["pi charges 1 4 30000", "pi recorded", "pi pid"],
            &["pi charges 1 4 3000", "pi recorded", "pi pid"],
        ],
        probes: &[EDITS, EDITS_CUT_OFF],
        expect: [EDIT_CUT_OFF, EDIT_CUT_OFF],
    },
    Scenario {
        name: "agent-timer",
        about: "kill the demo host about a second after the agent \
                set a timer 20 s ahead, and start it again after \
                the timer was due with both hosts down; the timer's \
                input arrives once at the first start, and not \
                again at the second",
        demo: Demo::Linked { pid: NODE_PID },
        setup: &[
            "pi make 1",
            "pi pid",
            "pi say 1 wake 20: stretch",
            "pi charges 1 2 20000",
        ],
        trigger: "pi pid",
        trigger_replies: true,
        kill: Kill::After {
            min_ms: 0,
            max_ms: 200,
        },
        down_ms: 22000,
        restart: [
            &["pi charges 1 3 30000", "pi recorded", "pi pid"],
            &["pi charges 1 3 3000", "pi recorded", "pi pid"],
        ],
        probes: &[WAKES],
        expect: [WOKEN_ONCE, WOKEN_ONCE],
    },
];
