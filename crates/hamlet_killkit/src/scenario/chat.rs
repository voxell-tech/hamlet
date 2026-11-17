//! The agent's chat features across a kill: a compaction, `/new` and
//! `/retry`. Each scenario's first line is a `pi` command, which
//! fails unless `HAMLET_DEMO_AGENT` is `pi`.
//!
//! `chat-compact` needs `HAMLET_DEMO_FAUX` to name a file that holds
//! this faux script. Its model's window makes the third long line's
//! turn compact. Its answers are short, so the summary is too, and a
//! restarted Node host, which begins the script again, answers the
//! restarts' lines with the first two.
//!
//! ```json
//! {
//!   "models": [{ "id": "faux-compact", "window": 16000 }],
//!   "responses": [
//!     "Noted.",
//!     "Noted.",
//!     "Summary: the chief said two long lines of filler.",
//!     { "calls": [{ "name": "tell",
//!       "arguments": { "to": "hire#2", "text": "after the summary" } }] }
//!   ]
//! }
//! ```

use super::{Demo, Expect, Is, Kill, Probe, Read, Scenario};

const NODE_PID: &str = "node/pid";

const fn agent(name: &'static str, sql: &'static str) -> Probe {
    Probe {
        name,
        file: "actors/agent/1.sqlite",
        read: Read::Sql(sql),
    }
}

const fn exactly(value: &'static str, n: i64) -> Expect {
    Expect {
        value,
        is: Is::Exactly(n),
    }
}

const COMPACTIONS: Probe = agent(
    "agent.compactions",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.compaction'",
);

const LINES: Probe = agent(
    "agent.lines",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'hamlet.line'",
);

const ANSWERS: Probe = agent(
    "agent.answers",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.assistant'",
);

/// Answers after the summary, in the conversation it summarised,
/// whose request held the summary and the two long lines it kept:
/// about 25,500 tokens with the prompt's 2,900. The first long line
/// too would make it about 36,700.
const ANSWERS_ON_SUMMARY: Probe = agent(
    "agent.answers_on_summary",
    "SELECT COUNT(*) FROM pi_entries AS answer JOIN pi_entries AS \
     summary ON summary.conversation_id = answer.conversation_id \
     AND summary.id < answer.id WHERE summary.record ->> '$.kind' = \
     'pi.compaction' AND answer.record ->> '$.kind' = \
     'pi.assistant' AND answer.record ->> \
     '$.model[0].usage.totalTokens' BETWEEN 20000 AND 30000",
);

const TELL_RESULTS: Probe = agent(
    "agent.tell_results",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.tool-result' AND record ->> '$.model[0].toolName' = 'tell'",
);

/// A `tell` result the model was told was interrupted: a repeat the
/// realm refused, rather than answered from what it kept.
const TELLS_CUT_OFF: Probe = agent(
    "agent.tells_cut_off",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.tool-result' AND record ->> '$.model[0].toolName' = 'tell' \
     AND record ->> '$.data.diagnostics[0].code' = \
     'tool_interrupted'",
);

const SESSIONS: Probe = agent(
    "agent.sessions",
    "SELECT COUNT(*) FROM chat WHERE kind = 'chief'",
);

const NEW_NOTICES: Probe = agent(
    "agent.new_notices",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'hamlet.notice' AND record ->> '$.data.text' = 'Started a new \
     session.'",
);

const LINES_IN_NEW: Probe = agent(
    "agent.lines_in_new",
    "SELECT COUNT(*) FROM pi_entries JOIN chat ON chat.conversation \
     = pi_entries.conversation_id WHERE chat.kind = 'chief' AND \
     chat.session = 2 AND chat.current = 1 AND record ->> '$.kind' \
     = 'hamlet.line'",
);

const ANSWERS_IN_NEW: Probe = agent(
    "agent.answers_in_new",
    "SELECT COUNT(*) FROM pi_entries JOIN chat ON chat.conversation \
     = pi_entries.conversation_id WHERE chat.kind = 'chief' AND \
     chat.session = 2 AND chat.current = 1 AND record ->> '$.kind' \
     = 'pi.assistant'",
);

const RETRIES: Probe = agent(
    "agent.retries",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'hamlet.retry'",
);

const RETRIES_ANSWERED: Probe = agent(
    "agent.retries_answered",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.assistant' AND record ->> '$.model[0].content[0].text' = \
     'Heard: Carry on from where you stopped.'",
);

/// Charged: 3 long lines, the summary, the post-tell request, a line
/// per restart. Answered: all but the summary, 2 before it.
const fn compacted_once(n: i64) -> [Expect; 9] {
    [
        exactly("charges/1", 5 + n),
        exactly("calls/tell", 2),
        exactly("acts/tell", 1),
        exactly("agent.compactions", 1),
        exactly("agent.lines", 3 + n),
        exactly("agent.answers", 4 + n),
        exactly("agent.answers_on_summary", 2 + n),
        exactly("agent.tell_results", 1),
        exactly("agent.tells_cut_off", 0),
    ]
}

/// After restart `n`, each saying one line in the new session.
const fn new_once(n: i64) -> [Expect; 6] {
    [
        exactly("charges/1", 1 + n),
        exactly("calls/lapse", 1),
        exactly("agent.sessions", 2),
        exactly("agent.new_notices", 1),
        exactly("agent.lines_in_new", n),
        exactly("agent.answers_in_new", n),
    ]
}

/// The cut-off retry's `open_task` and the one of the retry the
/// first restart asks again.
const fn retried_once(again: Is) -> [Expect; 6] {
    [
        Expect {
            value: "retry/1",
            is: again,
        },
        exactly("calls/open_task", 2),
        exactly("calls/turn_failed", 1),
        exactly("charges/1", 1),
        exactly("agent.retries", 1),
        exactly("agent.retries_answered", 1),
    ]
}

/// The length of each long line `chat-compact` says: about 11,000 of
/// the faux model's tokens, of four characters each. Two of them are
/// more than the 20,000 recent tokens compaction keeps, so the third
/// line's turn compacts the first.
const LONG: usize = 45_000;

const fn long_line(said: &str) -> [u8; LONG] {
    let said = said.as_bytes();
    let filler = b"filler ";
    let mut line = [0; LONG];
    let mut at = 0;
    while at < LONG {
        line[at] = if at < said.len() {
            said[at]
        } else {
            filler[(at - said.len()) % filler.len()]
        };
        at += 1;
    }
    line
}

const fn text(line: &'static [u8]) -> &'static str {
    match std::str::from_utf8(line) {
        Ok(text) => text,
        Err(_) => panic!("a long line is not UTF-8"),
    }
}

const FIRST: &str = text(&long_line("pi say 1 first: "));
const SECOND: &str = text(&long_line("pi say 1 second: "));
const THIRD: &str = text(&long_line("pi say 1 third: "));

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "chat-compact",
        about: "with HAMLET_DEMO_FAUX naming the faux script of \
                chat.rs, the agent's third long line compacts its \
                conversation, and the stand-in realm kills the Node \
                host once it kept the tell the model asks for after \
                the summary; the next Node host's start resumes the \
                turn at the cut-off tell, the conversation goes on \
                after its one summary, and no entry is kept twice",
        demo: Demo::Linked { pid: NODE_PID },
        setup: &[
            "pi make 1",
            "pi pid",
            "pi die-in-tell",
            FIRST,
            "pi charges 1 1 20000",
            SECOND,
            "pi charges 1 2 20000",
        ],
        trigger: THIRD,
        trigger_replies: true,
        kill: Kill::NodeHost,
        down_ms: 0,
        due: None,
        restart: [
            &[
                // The next Node host's answer after the tell's
                // result.
                "pi charges 1 5 30000",
                "pi say 1 after",
                "pi charges 1 6 30000",
                "pi recorded",
                "pi pid",
            ],
            &[
                "pi say 1 once more",
                "pi charges 1 7 30000",
                "pi recorded",
                "pi pid",
            ],
        ],
        probes: &[
            COMPACTIONS,
            LINES,
            ANSWERS,
            ANSWERS_ON_SUMMARY,
            TELL_RESULTS,
            TELLS_CUT_OFF,
        ],
        expect: [&compacted_once(1), &compacted_once(2)],
    },
    Scenario {
        name: "chat-new",
        about: "kill the demo host inside the agent's /new, once \
                the new session and its notice are kept, as it \
                tells the realm the session it left has ended; \
                nothing asks /new again, and after each restart the \
                agent has two sessions, the notice once, and the \
                restart's line answered in the new session",
        demo: Demo::Linked { pid: NODE_PID },
        setup: &[
            "pi make 1",
            "pi pid",
            "pi say 1 hello",
            "pi charges 1 1 20000",
        ],
        trigger: "pi new 1",
        trigger_replies: false,
        kill: Kill::Fault("called/lapse"),
        down_ms: 0,
        due: None,
        restart: [
            &[
                "pi say 1 again",
                "pi charges 1 2 30000",
                "pi recorded",
                "pi pid",
            ],
            &[
                "pi say 1 once more",
                "pi charges 1 3 30000",
                "pi recorded",
                "pi pid",
            ],
        ],
        probes: &[
            SESSIONS,
            NEW_NOTICES,
            LINES_IN_NEW,
            ANSWERS_IN_NEW,
        ],
        expect: [&new_once(1), &new_once(2)],
    },
    Scenario {
        name: "chat-retry",
        about: "kill the demo host inside the agent's /retry of a \
                turn the realm's budget failed, while it waits for \
                the realm's open_task; nothing makes the call again \
                and no retry input is kept, the chief's /retry at \
                the first restart is admitted, and after each \
                restart the agent holds one retry input and its \
                answer",
        demo: Demo::Linked { pid: NODE_PID },
        setup: &[
            "pi make 1",
            "pi pid",
            "pi refuse on",
            "pi say 1 hello",
            // No charge comes: this waits out the 10 s the refused
            // turn takes to fail.
            "pi charges 1 1 10000",
            "pi refuse off",
        ],
        trigger: "pi retry 1",
        trigger_replies: false,
        kill: Kill::Fault("called/open_task"),
        down_ms: 0,
        due: None,
        restart: [
            &[
                "pi answered retry 1",
                "pi charges 1 1 30000",
                "pi recorded",
                "pi pid",
            ],
            &["pi charges 1 1 3000", "pi recorded", "pi pid"],
        ],
        probes: &[RETRIES, RETRIES_ANSWERED],
        expect: [
            &retried_once(Is::Exactly(1)),
            &retried_once(Is::Absent),
        ],
    },
];
