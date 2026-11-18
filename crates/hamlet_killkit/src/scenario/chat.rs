//! The agent's chat features across a kill: a compaction, `/new` and
//! `/retry`, on the real agent against the realm actor.

use super::agent::{TELL_RESULTS, TELLS_CUT_OFF};
use super::realm::{HAND_CHARGES, MAYOR_LINE_TASKS};
use super::{
    Demo, Expect, Is, Kill, Probe, Scenario, exactly, hand, mayor,
    realm,
};

/// `chat-compact`'s faux script. Its model's window makes the third
/// long line's turn compact. Its answers are short, so the summary is
/// too, and a restarted Node host, which begins the script again,
/// answers the restarts' requests with the first steps.
const COMPACT_SCRIPT: &str = r#"{
  "models": [{ "id": "faux-compact", "window": 16000 }],
  "responses": [
    "Noted.",
    "Noted.",
    "Summary: the chief said two long lines of filler.",
    { "calls": [{ "name": "tell",
      "arguments": { "to": "hire#2", "text": "after the summary" } }] }
  ]
}"#;

const COMPACTIONS: Probe = hand(
    "agent.compactions",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.compaction'",
);

const LINES: Probe = hand(
    "agent.lines",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'hamlet.line'",
);

const ANSWERS: Probe = hand(
    "agent.answers",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.assistant'",
);

/// Answers after the summary, in the conversation it summarised,
/// whose request held the summary and the two long lines it kept:
/// about 26,100 tokens with the prompt's 3,500. The first long line
/// too would make it about 37,400.
const ANSWERS_ON_SUMMARY: Probe = hand(
    "agent.answers_on_summary",
    "SELECT COUNT(*) FROM pi_entries AS answer JOIN pi_entries AS \
     summary ON summary.conversation_id = answer.conversation_id \
     AND summary.id < answer.id WHERE summary.record ->> '$.kind' = \
     'pi.compaction' AND answer.record ->> '$.kind' = \
     'pi.assistant' AND answer.record ->> \
     '$.model[0].usage.totalTokens' BETWEEN 20000 AND 30000",
);

const SESSIONS: Probe = hand(
    "agent.sessions",
    "SELECT COUNT(*) FROM chat WHERE kind = 'chief'",
);

const NEW_NOTICES: Probe = hand(
    "agent.new_notices",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'hamlet.notice' AND record ->> '$.data.text' = 'Started a new \
     session.'",
);

const LINES_IN_NEW: Probe = hand(
    "agent.lines_in_new",
    "SELECT COUNT(*) FROM pi_entries JOIN chat ON chat.conversation \
     = pi_entries.conversation_id WHERE chat.kind = 'chief' AND \
     chat.session = 2 AND chat.current = 1 AND record ->> '$.kind' \
     = 'hamlet.line'",
);

const ANSWERS_IN_NEW: Probe = hand(
    "agent.answers_in_new",
    "SELECT COUNT(*) FROM pi_entries JOIN chat ON chat.conversation \
     = pi_entries.conversation_id WHERE chat.kind = 'chief' AND \
     chat.session = 2 AND chat.current = 1 AND record ->> '$.kind' \
     = 'pi.assistant'",
);

/// The reflections the session `/new` left asked for.
const REFLECTIONS: Probe = hand(
    "agent.reflections",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'hamlet.reflection'",
);

const RETRIES: Probe = mayor(
    "agent.retries",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'hamlet.retry'",
);

const RETRIES_ANSWERED: Probe = mayor(
    "agent.retries_answered",
    "SELECT COUNT(*) FROM pi_entries WHERE record ->> '$.kind' = \
     'pi.assistant' AND record ->> '$.model[0].content[0].text' = \
     'Heard: Carry on from where you stopped.'",
);

const MAYOR_CHARGES: Probe = realm(
    "realm.charges/2",
    "SELECT COUNT(*) FROM charge WHERE agent = 2",
);

/// The tasks the realm opened for a retry.
const RETRY_TASKS: Probe = realm(
    "realm.retry_tasks",
    "SELECT COUNT(*) FROM task WHERE origin = 'retry'",
);

/// Charged: 3 long lines, the summary, the post-tell request, a line
/// per restart. Answered: all but the summary, 2 before it.
const fn compacted_once(n: i64) -> [Expect; 9] {
    [
        exactly("took/2/letter", 1),
        exactly("realm.charges/1", 5 + n),
        exactly("agent.compactions", 1),
        exactly("agent.lines", 3 + n),
        exactly("agent.answers", 4 + n),
        exactly("agent.answers_on_summary", 2 + n),
        exactly("agent.tell_results", 1),
        exactly("agent.tells_cut_off", 0),
        exactly("agent.reflections", 0),
    ]
}

/// After restart `n`, each saying one line in the new session: the
/// line before `/new`, the reflection it asked of the session it
/// left, and a line per restart.
const fn new_once(n: i64) -> [Expect; 7] {
    [
        exactly("realm.charges/1", 2 + n),
        exactly("agent.reflections", 1),
        exactly("agent.sessions", 2),
        exactly("agent.new_notices", 1),
        exactly("agent.lines_in_new", n),
        exactly("agent.answers_in_new", n),
        Expect {
            value: "new/1",
            is: Is::Absent,
        },
    ]
}

/// The retry the first restart asks again, answered once.
const fn retried_once(again: Is) -> [Expect; 6] {
    [
        Expect {
            value: "retry/2",
            is: again,
        },
        exactly("realm.retry_tasks", 1),
        exactly("realm.charges/2", 1),
        exactly("agent.retries", 1),
        exactly("agent.retries_answered", 1),
        exactly("realm.line_tasks/2", 1),
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

const FIRST: &str = text(&long_line("realm say 1 l1 first: "));
const SECOND: &str = text(&long_line("realm say 1 l2 second: "));
const THIRD: &str = text(&long_line("realm say 1 l3 third: "));

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "chat-compact",
        about: "the agent's third long line compacts its \
                conversation, and the Node host kills itself once \
                the realm answered the tell the model asks for \
                after the summary; the next Node host's start \
                resumes the turn at the cut-off tell, the \
                conversation goes on after its one summary, and no \
                entry is kept twice",
        demo: Demo::RealmActor {
            faux: Some(COMPACT_SCRIPT),
        },
        setup: &[
            "realm found h1",
            "realm live 2",
            "pi pid",
            "pi die-after tell",
            FIRST,
            "until 1 1 assistant: Noted.",
            SECOND,
            "until 1 2 assistant: Noted.",
        ],
        trigger: THIRD,
        trigger_replies: true,
        kill: Kill::NodeHost,
        down_ms: 0,
        due: None,
        restart: [
            &[
                "pi pid",
                // The next Node host's answer after the tell's
                // result, and the mayor's to the letter.
                "until 1 3 assistant: Noted.",
                "agent 2 await letter 1",
                "until 2 1 assistant: Noted.",
                "realm say 1 a1 after",
                "until 1 1 assistant: Summary:",
                "pi pid",
            ],
            &[
                "pi pid",
                "agent 2 await letter 1",
                "realm say 1 a2 once more",
                "until 1 4 assistant: Noted.",
                "pi pid",
            ],
        ],
        probes: &[
            HAND_CHARGES,
            COMPACTIONS,
            LINES,
            ANSWERS,
            ANSWERS_ON_SUMMARY,
            TELL_RESULTS,
            TELLS_CUT_OFF,
            REFLECTIONS,
        ],
        expect: [&compacted_once(1), &compacted_once(2)],
    },
    Scenario {
        name: "chat-new",
        about: "kill the demo host once the agent's /new kept the \
                new session and its notice, as the reflection it \
                asks of the session it left opens its task; nothing \
                asks /new again, and after each restart the agent \
                has two sessions, the notice once, one reflection, \
                and the restart's line answered in the new session",
        demo: Demo::RealmActor { faux: None },
        setup: &[
            "realm live 1",
            "pi pid",
            "realm say 1 s1 hello",
            "until 1 1 assistant: Heard: hello",
        ],
        trigger: "pi new 1",
        trigger_replies: false,
        kill: Kill::Fault("called/open_task"),
        down_ms: 0,
        due: None,
        restart: [
            &[
                "pi pid",
                "until 1 1 assistant: Heard: Reflection",
                "realm say 1 s2 again",
                "until 1 1 assistant: Heard: again",
                "pi pid",
            ],
            &[
                "pi pid",
                "realm say 1 s3 once more",
                "until 1 1 assistant: Heard: once more",
                "pi pid",
            ],
        ],
        probes: &[
            HAND_CHARGES,
            REFLECTIONS,
            SESSIONS,
            NEW_NOTICES,
            LINES_IN_NEW,
            ANSWERS_IN_NEW,
        ],
        expect: [&new_once(1), &new_once(2)],
    },
    Scenario {
        name: "chat-retry",
        about: "kill the demo host inside the mayor's /retry of a \
                turn its hamlet's spent budget failed, while it \
                waits for the realm's open_task; nothing makes the \
                call again and no retry input is kept, the chief's \
                /retry at the first restart is admitted, and after \
                each restart the mayor holds one retry input and \
                its answer",
        demo: Demo::RealmActor { faux: None },
        setup: &[
            "realm found h1",
            "realm live 2",
            "pi pid",
            "realm budget h1 0",
            "realm say 2 s1 hello",
            "until 2 1 assistant: failed:",
            "realm budget h1 1000000",
        ],
        trigger: "pi retry 2",
        trigger_replies: false,
        kill: Kill::Fault("called/open_task"),
        down_ms: 0,
        due: None,
        restart: [
            &[
                "pi pid",
                "pi answered retry 2",
                "until 2 1 assistant: Heard: Carry on",
                "pi pid",
            ],
            &["pi pid"],
        ],
        probes: &[
            RETRIES,
            RETRIES_ANSWERED,
            MAYOR_CHARGES,
            RETRY_TASKS,
            MAYOR_LINE_TASKS,
        ],
        expect: [
            &retried_once(Is::Exactly(1)),
            &retried_once(Is::Absent),
        ],
    },
];
