//! The fixtures of letters between agents.

use hamlet_messages::realm::letters::{
    Ask, Enclosed, LetterRefusal, Reply, ReplyRefusal, Sent, Tell,
    Waiting,
};
use hamlet_messages::realm::{LetterId, Named};
use hamlet_messages::workspace::ConversationId;

use super::super::Fixtures;
use super::{
    HIRE, answers, by_an_agent, done, hire, paid, shown, tool,
};

pub(super) fn the_letters(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/tell",
        &paid(Tell {
            to: Named::Mayor,
            text: "The tests pass".into(),
            paths: Vec::new(),
        }),
    );
    fixtures.args(
        "realm/tell.paths",
        &paid(Tell {
            to: Named::Mayor,
            text: "The report".into(),
            paths: vec![
                Enclosed {
                    path: "/work/report.md".into(),
                },
                Enclosed {
                    path: "/work/figures".into(),
                },
            ],
        }),
    );
    fixtures.args(
        "realm/tell.no_task",
        &tool(Tell {
            to: Named::Hand,
            text: "The tests pass".into(),
            paths: Vec::new(),
        }),
    );
    fixtures.args(
        "realm/ask",
        &paid(Ask {
            to: hire(),
            text: "Which tests fail?".into(),
            conversation: ConversationId("c1".into()),
            paths: Vec::new(),
        }),
    );
    fixtures.args(
        "realm/ask.paths",
        &paid(Ask {
            to: hire(),
            text: "Is this plan right?".into(),
            conversation: ConversationId("c1".into()),
            paths: vec![Enclosed {
                path: "/work/plan.md".into(),
            }],
        }),
    );
    let letter_refusals = || {
        vec![
            ("yourself", LetterRefusal::Yourself),
            ("empty", LetterRefusal::Empty),
            (
                "no_such_agent",
                LetterRefusal::NoSuchAgent {
                    address: "hire#9".to_owned(),
                },
            ),
            ("no_road", LetterRefusal::NoRoad { to: shown(HIRE) }),
            (
                "still_hiring",
                LetterRefusal::StillHiring { to: shown(HIRE) },
            ),
            ("withdrawn", LetterRefusal::Withdrawn),
            (
                "files",
                LetterRefusal::Files {
                    words: "`/outside` cannot be read: /outside is \
                            out of this agent's reach to read"
                        .to_owned(),
                },
            ),
        ]
    };
    answers(
        fixtures,
        "realm/tell",
        [("sent", Sent::Sent { to: shown(HIRE) })],
        by_an_agent(letter_refusals()),
    );
    answers(
        fixtures,
        "realm/ask",
        [(
            "waiting",
            Waiting::Waiting {
                on: shown(HIRE),
                question: LetterId(5),
            },
        )],
        by_an_agent(letter_refusals()),
    );
    fixtures.args(
        "realm/reply",
        &Reply {
            question: LetterId(5),
            text: "Two of them".into(),
        },
    );
    done(
        fixtures,
        "realm/reply",
        vec![
            ("no_such_question", ReplyRefusal::NoSuchQuestion),
            ("not_asked", ReplyRefusal::NotAsked),
        ],
    );
}
