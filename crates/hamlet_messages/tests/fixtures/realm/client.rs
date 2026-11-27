//! The fixtures of the calls the server makes for a client, and of
//! the world it reads.

use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::client::{
    Answer, AnswerRefusal, ReadWorld, Say, SayAnswer, SayRefusal,
};
use hamlet_messages::realm::oversight::{
    Offer, Permission, Question,
};
use hamlet_messages::realm::parcels::Measure;
use hamlet_messages::realm::world::{
    AgentSeen, GrantSeen, GrantSource, HamletSeen, Lasting,
    ObjectionSource, ReachSeen, Revision, RoadSeen, Seen, Spend,
    Starter, Status, WaitId, WaitKind, WaitSeen,
};
use hamlet_messages::realm::{HowEnded, ProfileId, Tier, ToolName};
use hamlet_messages::workspace::files::EditDiff;
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{
    AgentNumber, ReachVersion, RealmTask,
};
use hamlet_protocol::{Effort, client as protocol};

use super::super::{Fixtures, call};
use super::{HIRE, MAYOR, answers, done, hamlet, left, shown};

pub(super) fn image() -> protocol::AttachedImage {
    protocol::AttachedImage {
        at: Some(5),
        id: "ab".repeat(32).parse().unwrap(),
    }
}

pub(super) fn file() -> protocol::AttachedFile {
    protocol::AttachedFile {
        at: None,
        id: "cd".repeat(32).parse().unwrap(),
        name: "notes/plan.md".into(),
        size: 2_048,
    }
}

pub(super) fn the_clients_calls(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/say",
        &Say(protocol::Say {
            files: vec![file()],
            id: "0b5e7c4a-1d2f-4e3a-8b9c-0d1e2f3a4b5c"
                .parse()
                .unwrap(),
            images: vec![image()],
            text: "Look at this plan".into(),
            to: protocol::AgentId(12),
        }),
    );
    let said = || RealmTask("task-2".into());
    answers(
        fixtures,
        "realm/say",
        [("accepted", SayAnswer::Accepted { task: said() })],
        vec![
            (
                "no_such_agent",
                SayRefusal::NoSuchAgent {
                    address: "9".to_owned(),
                },
            ),
            (
                "left",
                SayRefusal::Left {
                    agent: shown(HIRE),
                    left: left(HowEnded::ByTheChief),
                },
            ),
            (
                "not_a_place",
                SayRefusal::NotAPlace { at: 5, length: 4 },
            ),
            (
                "file_not_sent",
                SayRefusal::FileNotSent {
                    name: "notes/plan.md".to_owned(),
                },
            ),
            (
                "not_its_size",
                SayRefusal::NotItsSize {
                    name: "notes/plan.md".to_owned(),
                },
            ),
            ("too_many_bytes", SayRefusal::TooManyBytes),
            (
                "no_image",
                SayRefusal::NoImage {
                    image: "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae".to_owned(),
                },
            ),
            (
                "not_a_file_name",
                SayRefusal::NotAFileName {
                    name: "../plan.md".to_owned(),
                },
            ),
        ],
    );
    fixtures.unprobed_args(
        "realm/answer",
        &Answer(protocol::Answer {
            choice: protocol::Choice::Reply,
            id: "1c6f8d5b-2e3a-4f4b-9cad-1e2f3a4b5c6d"
                .parse()
                .unwrap(),
            picks: None,
            text: Some("Yes, go on".into()),
            wait: "3".into(),
        }),
    );
    done(
        fixtures,
        "realm/answer",
        vec![
            (
                "nothing_waits",
                AnswerRefusal::NothingWaits { wait: "3".into() },
            ),
            (
                "not_that_way",
                AnswerRefusal::NotThatWay {
                    answers: "/allow /deny".to_owned(),
                },
            ),
            ("picks_with_text", AnswerRefusal::PicksWithText),
            ("blank_reply", AnswerRefusal::BlankReply),
            ("unequal", AnswerRefusal::Unequal),
            ("no_option", AnswerRefusal::NoOption { question: 2 }),
            (
                "takes_option",
                AnswerRefusal::TakesOption { question: 1 },
            ),
            ("no_words", AnswerRefusal::NoWords { question: 2 }),
        ],
    );
    fixtures.args("realm/read_world", &ReadWorld);
    fixtures.answer("realm/read_world", &seen());
}

pub(super) fn seen() -> Seen {
    let agent = |agent, lead, status, reach| AgentSeen {
        agent,
        hamlet: Some(hamlet()),
        lead,
        status,
        reach,
        worker: None,
        nickname: None,
        runs: Some("faux::faux-1".to_owned()),
        model: None,
        effort: None,
        fast: None,
        allowance_micros: None,
        tools_off: Vec::new(),
    };
    Seen {
        revision: Revision(42),
        agents: vec![
            AgentSeen {
                hamlet: None,
                ..agent(
                    AgentNumber(1),
                    None,
                    Status::Live,
                    ReachSeen::Applied {
                        version: ReachVersion(3),
                    },
                )
            },
            agent(
                MAYOR,
                None,
                Status::Live,
                ReachSeen::Pending {
                    applied: Some(ReachVersion(1)),
                    wanted: ReachVersion(2),
                },
            ),
            AgentSeen {
                nickname: Some("ada".into()),
                tools_off: vec![ToolName {
                    name: "web_fetch".into(),
                }],
                runs: Some("anthropic::claude-opus".into()),
                model: Some("anthropic::claude-opus".into()),
                effort: Some(Effort::High),
                fast: Some(true),
                allowance_micros: Some(2_000_000),
                ..agent(
                    HIRE,
                    Some(MAYOR),
                    Status::Hiring,
                    ReachSeen::None,
                )
            },
            agent(
                AgentNumber(13),
                Some(MAYOR),
                Status::Ended {
                    left: left(HowEnded::ByTheHand),
                },
                ReachSeen::Pending {
                    applied: None,
                    wanted: ReachVersion(1),
                },
            ),
            AgentSeen {
                worker: Some(Starter::Agent { agent: HIRE }),
                ..agent(
                    AgentNumber(15),
                    None,
                    Status::Live,
                    ReachSeen::None,
                )
            },
            AgentSeen {
                worker: Some(Starter::Chief),
                ..agent(
                    AgentNumber(16),
                    None,
                    Status::Ended {
                        left: left(HowEnded::Answered),
                    },
                    ReachSeen::None,
                )
            },
        ],
        hamlets: vec![HamletSeen {
            hamlet: hamlet(),
            name: "Riverside".into(),
            mayor: MAYOR,
            tier: Tier::Fifty,
        }],
        roads: vec![RoadSeen { a: MAYOR, b: HIRE }],
        waits: waits(),
        spend: vec![
            Spend {
                payer: Payer::Realm,
                spent_micros: 1_250_000,
                spent_tokens: 4_200,
                budget_micros: None,
                spent_ms: 90_000,
                budget_ms: None,
            },
            Spend {
                payer: Payer::Hamlet { hamlet: hamlet() },
                spent_micros: 250_000,
                spent_tokens: 4_200,
                budget_micros: Some(5_000_000),
                spent_ms: 12_000,
                budget_ms: Some(3_600_000),
            },
        ],
        grants: vec![
            GrantSeen {
                agent: HIRE,
                permission: Permission::Network,
                source: GrantSource::Grant,
            },
            GrantSeen {
                agent: MAYOR,
                permission: Permission::Network,
                source: GrantSource::Limits,
            },
        ],
    }
}

pub(super) fn waits() -> Vec<WaitSeen> {
    let wait = |wait, subject: &str, kind| WaitSeen {
        wait: WaitId(wait),
        agent: HIRE,
        subject: subject.into(),
        kind,
    };
    vec![
        wait(
            3,
            "curl https://example.com",
            WaitKind::Tool {
                call: call("call-1"),
                danger: Need::Network,
                always: Some(Lasting::Grant),
                change: None,
            },
        ),
        wait(
            6,
            "notes.md",
            WaitKind::Tool {
                call: call("call-2"),
                danger: Need::Edit,
                always: Some(Lasting::Session),
                change: Some(EditDiff {
                    diff: "- 1 hello\n+ 1 goodbye".into(),
                    patch: "--- notes.md\n+++ notes.md\n@@ -1 +1 \
                            @@\n-hello\n+goodbye\n"
                        .into(),
                    first_changed_line: Some(1),
                }),
            },
        ),
        wait(
            4,
            "Which seed?",
            WaitKind::Question {
                message: Some("Before planting".into()),
                questions: vec![Question {
                    question: "Which seed?".into(),
                    description: None,
                    options: vec![Offer {
                        title: "Oats".into(),
                        description: Some("Quick to grow".into()),
                    }],
                    text_input: true,
                }],
            },
        ),
        wait(
            5,
            "`hire#3` objects: the field is better left alone",
            WaitKind::Objection {
                reason: "the field is better left alone".into(),
                source: ObjectionSource::Agent,
            },
        ),
        wait(
            6,
            "hire#2 objects: the odd line breaks the plan",
            WaitKind::Objection {
                reason: "the odd line breaks the plan".into(),
                source: ObjectionSource::Review,
            },
        ),
        wait(7, "end hire#13: the docs are done", WaitKind::Reserved),
        wait(
            8,
            "40 MB in 1200 files: tell {}",
            WaitKind::Send {
                measure: Measure {
                    files: 1_200,
                    bytes: 40 << 20,
                },
            },
        ),
        wait(9, "the model failed: overloaded", WaitKind::Fault),
        wait(
            10,
            "profile#3 (Newbie)",
            WaitKind::Newcomer {
                profile: ProfileId(3),
            },
        ),
    ]
}
