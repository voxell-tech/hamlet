//! The real agent on a faux model, the Hand, against the realm actor.

use super::realm::HAND_CHARGES;
use super::{Demo, Expect, Kill, Probe, Read, Scenario, exactly};

pub(super) const fn hand(
    name: &'static str,
    sql: &'static str,
) -> Probe {
    Probe {
        name,
        file: "actors/agent/1.sqlite",
        read: Read::Sql(sql),
    }
}

pub(super) const TELL_RESULTS: Probe = hand(
    "agent.tell_results",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.tool-result' AND record ->> '$.model[0].toolName' = 'tell'",
);

/// A `tell` result the model was told was interrupted: a repeat the
/// realm refused, rather than answered from what it kept.
pub(super) const TELLS_CUT_OFF: Probe = hand(
    "agent.tells_cut_off",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.tool-result' AND record ->> '$.model[0].toolName' = 'tell' \
     AND record ->> '$.data.diagnostics[0].code' = \
     'tool_interrupted'",
);

const EDITS_CUT_OFF: Probe = hand(
    "agent.edits_cut_off",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.tool-result' AND record ->> '$.model[0].toolName' = 'edit' \
     AND record ->> '$.data.diagnostics[0].code' = \
     'tool_interrupted'",
);

/// The agent's reads of its notes that found the edit made once. The
/// edit makes `draft` into `draft, final`, so an edit made again
/// finds `draft` still and adds a second `final`.
const READS_EDITED_ONCE: Probe = hand(
    "agent.reads_edited_once",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.tool-result' AND record ->> '$.model[0].toolName' = 'read' \
     AND record ->> '$.model[0].content[0].text' = 'draft, final'",
);

const EDITS: Probe = hand(
    "agent.edits",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.tool-result' AND record ->> '$.model[0].toolName' = 'edit'",
);

const WAKES: Probe = hand(
    "agent.wakes",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'hamlet.wake'",
);

/// After restart `n`: the write's two requests, the edit's request,
/// the one after the edit's result, and two for each restart's read
/// of the notes.
const fn edit_cut_off(n: i64) -> [Expect; 4] {
    [
        exactly("realm.charges/1", 4 + 2 * n),
        exactly("agent.edits", 1),
        exactly("agent.edits_cut_off", 1),
        exactly("agent.reads_edited_once", n),
    ]
}

/// The wake's two requests and the one the timer's input began.
const WOKEN_ONCE: &[Expect] = &[
    exactly("took/1/wake", 1),
    exactly("realm.charges/1", 3),
    exactly("agent.wakes", 1),
];

/// The timer's input answered, which a stop would cut off.
const WAKE_ANSWERED: &str = "until 1 1 assistant: Heard:";

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "agent-mid-turn",
        about: "the Node host kills itself in the agent's edit of a \
                file of its own, once the file is written and \
                before the edit's result is kept; the next Node \
                host's start of the agent resumes the turn at the \
                cut-off edit with no call from outside, does not \
                run the edit again, tells the model it was \
                interrupted, and the turn ends; the agent's read of \
                the file at each restart finds the edit made once",
        demo: Demo::RealmActor { faux: None },
        setup: &[
            "realm live 1",
            "pi pid",
            "realm say 1 w1 write own:memory/notes.md: draft",
            "until 1 1 assistant: Done:",
        ],
        trigger: "realm say 1 e1 die in edit own:memory/notes.md: \
                  draft -> draft, final",
        trigger_replies: true,
        kill: Kill::NodeHost,
        down_ms: 0,
        due: None,
        restart: [
            &[
                "pi pid",
                "pi pid",
                "until 1 2 assistant: Done:",
                "realm say 1 r1 read own:memory/notes.md",
                "until 1 3 assistant: Done:",
                "pi pid",
            ],
            &[
                "pi pid",
                "realm say 1 r2 read own:memory/notes.md",
                "until 1 4 assistant: Done:",
                "pi pid",
            ],
        ],
        probes: &[
            HAND_CHARGES,
            EDITS,
            EDITS_CUT_OFF,
            READS_EDITED_ONCE,
        ],
        expect: [&edit_cut_off(1), &edit_cut_off(2)],
    },
    Scenario {
        name: "agent-timer",
        about: "kill the demo host at a random time after the agent \
                set a timer 20 s ahead, and start it again after \
                the timer was due with both hosts down; the timer's \
                input arrives once at the first start, and not \
                again at the second",
        demo: Demo::RealmActor { faux: None },
        setup: &[
            "realm live 1",
            "pi pid",
            "realm say 1 w1 wake 20: stretch",
            "until 1 1 assistant: Done:",
        ],
        trigger: "pi pid",
        trigger_replies: true,
        kill: Kill::After {
            min_ms: 0,
            max_ms: 200,
        },
        down_ms: 22000,
        due: None,
        restart: [
            &[
                "pi pid",
                "agent 1 await wake 1",
                WAKE_ANSWERED,
                "pi pid",
            ],
            &[
                "pi pid",
                "agent 1 await wake 1",
                WAKE_ANSWERED,
                "pi pid",
            ],
        ],
        probes: &[HAND_CHARGES, WAKES],
        expect: [WOKEN_ONCE, WOKEN_ONCE],
    },
];
