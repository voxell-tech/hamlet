//! The fixtures of the realm's messages, and of the calls the realm
//! makes to agents and profiles.

use std::fmt::{Debug, Display};

use hamlet_messages::address::{Party, Profile};
use hamlet_messages::agent::{
    AgentInput, AgentKind, Author, Fill, Input, InputId, Place,
    PlaceVersion, Submit,
};
use hamlet_messages::profile::Branch;
use hamlet_messages::realm::budget::{
    Budget, BudgetLimit, Charge, MaySpend, Payer, ResetBudget,
    SpendRefusal,
};
use hamlet_messages::realm::client::{
    Answer, AnswerRefusal, ReadWorld, Say, SayAnswer, SayRefusal,
};
use hamlet_messages::realm::world::{
    AgentSeen, HamletSeen, ReachSeen, Revision, RoadSeen, Seen,
    Spend, Status, WaitSeen,
};
use hamlet_messages::realm::{
    AgentRefusal, Ask, Asked, Done, End, EndRefusal, Found,
    FoundAnswer, FoundRefusal, Gate, HamletId, HamletRefusal, Hire,
    HireAnswer, HireRefusal, HowEnded, LayRefusal, LayRoad,
    LeadRefusal, Left, LetterId, LetterRefusal, Named, ProfileId,
    Rank, Refusable, Reply, ReplyRefusal, RequestRoad,
    RequestRoadRefusal, Seat, SeatRefusal, SetLimits, SetTier,
    SetTools, SocietyTool, Standing, TakeUpRefusal, TakeUpRoad, Tell,
    Tier, TierRefusal, Tool, ToolsRefusal,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, ReachVersion, RealmTask,
};
use hamlet_protocol::client as protocol;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::cases::{Cases, Union, family, named, tagged, union};
use super::{AGENT, Fixtures, call, task};

const HIRE: AgentNumber = AgentNumber(12);
const MAYOR: AgentNumber = AgentNumber(2);

/// Every enum the realm adds.
pub fn enums() -> Vec<Cases> {
    vec![
        tagged::<Done<HamletRefusal>>(),
        tagged::<FoundAnswer<FoundRefusal>>(),
        tagged::<HireAnswer<HireRefusal>>(),
        tagged::<Standing>(),
        tagged::<Gate>(),
        tagged::<Rank>(),
        tagged::<Named>(),
        tagged::<HowEnded>(),
        tagged::<FoundRefusal>(),
        tagged::<HireRefusal>(),
        tagged::<EndRefusal>(),
        tagged::<SeatRefusal>(),
        tagged::<LetterRefusal>(),
        tagged::<ReplyRefusal>(),
        tagged::<LayRefusal>(),
        tagged::<TakeUpRefusal>(),
        tagged::<RequestRoadRefusal>(),
        tagged::<ToolsRefusal>(),
        tagged::<TierRefusal>(),
        tagged::<HamletRefusal>(),
        tagged::<Payer>(),
        tagged::<BudgetLimit>(),
        tagged::<Budget>(),
        tagged::<SayAnswer>(),
        tagged::<SayRefusal>(),
        tagged::<AnswerRefusal>(),
        tagged::<Status>(),
        tagged::<ReachSeen>(),
        tagged::<Input>(),
        named::<SocietyTool>(),
        named::<Tier>(),
        named::<AgentKind>(),
    ]
}

/// Every untagged union the realm adds, with its families.
pub fn unions() -> Vec<Union> {
    vec![
        union!(SpendRefusal {
            Standing: Standing,
            Budget: Budget,
        }),
        agent_refusal::<LetterRefusal>(),
        agent_refusal::<RequestRoadRefusal>(),
        lead_refusal::<HireRefusal>(),
        lead_refusal::<EndRefusal>(),
        lead_refusal::<LayRefusal>(),
        lead_refusal::<TakeUpRefusal>(),
        lead_refusal::<ToolsRefusal>(),
    ]
}

fn agent_refusal<R: DeserializeOwned>() -> Union {
    union!(AgentRefusal<R> {
        Standing: Standing,
        Gate: Gate,
        Act: R,
    })
}

fn lead_refusal<R: DeserializeOwned>() -> Union {
    union!(LeadRefusal<R> {
        Standing: Standing,
        Gate: Gate,
        Rank: Rank,
        Act: R,
    })
}

pub fn the_realms_messages(fixtures: &mut Fixtures) {
    the_founding_and_the_hires(fixtures);
    the_letters(fixtures);
    the_roads_and_the_tools(fixtures);
    the_limits_and_the_spending(fixtures);
    the_clients_calls(fixtures);
    the_calls_to_agents_and_profiles(fixtures);
}

/// Checks an answer's arms under `name`, and `refused` with each
/// reason.
fn answers<A>(
    fixtures: &mut Fixtures,
    name: &str,
    arms: impl IntoIterator<Item = (&'static str, A)>,
    refusals: Vec<(&'static str, A::Why)>,
) where
    A: Refusable + Serialize + DeserializeOwned + PartialEq + Debug,
{
    for (arm, answer) in arms {
        fixtures.answer(&format!("{name}.{arm}"), &answer);
    }
    for (case, why) in refusals {
        fixtures.answer(
            &format!("{name}.refused.{case}"),
            &A::refused(why),
        );
    }
}

fn done<W>(
    fixtures: &mut Fixtures,
    name: &str,
    refusals: Vec<(&'static str, W)>,
) where
    W: Serialize + DeserializeOwned + PartialEq + Debug + Display,
{
    answers(fixtures, name, [("done", Done::Done)], refusals);
}

/// The refusals of an agent's act: the caller's own, then the act's.
fn by_an_agent<R>(
    tool: SocietyTool,
    acts: Vec<(&'static str, R)>,
) -> Vec<(&'static str, AgentRefusal<R>)> {
    callers(tool)
        .into_iter()
        .chain(
            acts.into_iter()
                .map(|(case, why)| (case, AgentRefusal::Act(why))),
        )
        .collect()
}

/// The refusals of an agent's act that needs a lead's powers.
fn by_a_lead<R>(
    tool: SocietyTool,
    acts: Vec<(&'static str, R)>,
) -> Vec<(&'static str, LeadRefusal<R>)> {
    callers(tool)
        .into_iter()
        .chain([(
            "not_a_lead",
            LeadRefusal::Rank(Rank::NotALead { hire: HIRE }),
        )])
        .chain(
            acts.into_iter()
                .map(|(case, why)| (case, LeadRefusal::Act(why))),
        )
        .collect()
}

/// The refusals of the calling agent itself.
fn callers<U: From<Standing> + From<Gate>>(
    tool: SocietyTool,
) -> [(&'static str, U); 2] {
    [
        ("you_left", you_left().into()),
        ("tool_off", Gate::ToolOff { tool }.into()),
    ]
}

fn you_left() -> Standing {
    Standing::YouLeft {
        how: HowEnded::ByALead { lead: MAYOR },
        why: Some("the docs are done".into()),
    }
}

fn left(how: HowEnded) -> Left {
    Left {
        how,
        why: Some("the docs are done".into()),
    }
}

fn asked<T>(act: T) -> Asked<T> {
    Asked {
        id: "6f1c2a3e-8b4d-4c5e-9f60-718293a4b5c6".parse().unwrap(),
        act,
    }
}

fn tool<T>(act: T) -> Tool<T> {
    Tool {
        call: call("call-5"),
        act,
    }
}

fn hamlet() -> HamletId {
    HamletId("5f0d2c1b-7a3e-4d9f-8b6a-1c2d3e4f5a6b".into())
}

/// The task that pays for a turn no chief's line started.
fn standing_task() -> RealmTask {
    RealmTask("standing-12".into())
}

fn hire() -> Named {
    Named::Agent { agent: HIRE }
}

fn the_founding_and_the_hires(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_found",
        &asked(Found {
            name: "Riverside".into(),
            mayor: Some(ProfileId(2)),
            tier: Tier::Fifty,
            purpose: Some("Keep the docs in order".into()),
        }),
    );
    fixtures.args(
        "realm/chief_found.blank",
        &asked(Found {
            name: "Riverside".into(),
            mayor: None,
            tier: Tier::Hundred,
            purpose: None,
        }),
    );
    fixtures.args(
        "realm/found",
        &tool(Found {
            name: "Harbor".into(),
            mayor: None,
            tier: Tier::FiveHundred,
            purpose: Some("Run the release".into()),
        }),
    );
    for name in ["realm/chief_found", "realm/found"] {
        answers(
            fixtures,
            name,
            [(
                "founded",
                FoundAnswer::Founded {
                    hamlet: hamlet(),
                    mayor: MAYOR,
                },
            )],
            vec![
                ("empty_name", FoundRefusal::EmptyName),
                (
                    "no_such_profile",
                    FoundRefusal::NoSuchProfile {
                        profile: ProfileId(9),
                    },
                ),
            ],
        );
    }
    fixtures.args(
        "realm/chief_hire",
        &asked(Hire {
            hamlet: Some(hamlet()),
            under: Some(Named::MayorOf { hamlet: hamlet() }),
            profile: Some(ProfileId(2)),
            purpose: Some("Write the tests".into()),
        }),
    );
    fixtures.args(
        "realm/chief_hire.blank",
        &asked(Hire {
            hamlet: None,
            under: None,
            profile: None,
            purpose: None,
        }),
    );
    fixtures.args(
        "realm/hire",
        &tool(Hire {
            hamlet: None,
            under: Some(hire()),
            profile: None,
            purpose: Some("Check the links".into()),
        }),
    );
    let hire_refusals = || {
        vec![
            ("no_hamlet", HireRefusal::NoHamlet),
            ("no_such_hamlet", HireRefusal::NoSuchHamlet),
            ("no_such_agent", HireRefusal::NoSuchAgent),
            (
                "lead_left",
                HireRefusal::LeadLeft {
                    lead: HIRE,
                    left: left(HowEnded::ByTheChief),
                },
            ),
            (
                "full",
                HireRefusal::Full {
                    hamlet: hamlet(),
                    most: 50,
                },
            ),
            (
                "no_such_profile",
                HireRefusal::NoSuchProfile {
                    profile: ProfileId(9),
                },
            ),
        ]
    };
    answers(
        fixtures,
        "realm/chief_hire",
        [("hired", HireAnswer::Hired { agent: HIRE })],
        hire_refusals(),
    );
    answers(
        fixtures,
        "realm/hire",
        [("hired", HireAnswer::Hired { agent: HIRE })],
        by_a_lead(SocietyTool::Hire, hire_refusals()),
    );
    fixtures.args(
        "realm/chief_end",
        &asked(End {
            hire: hire(),
            why: Some("The tests are written".into()),
        }),
    );
    fixtures.args(
        "realm/end",
        &tool(End {
            hire: hire(),
            why: None,
        }),
    );
    let end_refusals = || {
        vec![
            ("no_such_agent", EndRefusal::NoSuchAgent),
            ("the_hand", EndRefusal::TheHand),
            ("the_mayor", EndRefusal::TheMayor),
            (
                "left",
                EndRefusal::Left {
                    hire: HIRE,
                    left: Left {
                        how: HowEnded::ByTheHand,
                        why: None,
                    },
                },
            ),
        ]
    };
    done(fixtures, "realm/chief_end", end_refusals());
    done(
        fixtures,
        "realm/end",
        by_a_lead(SocietyTool::End, end_refusals()),
    );
    fixtures.args("realm/chief_seat", &asked(Seat { hire: hire() }));
    fixtures.args(
        "realm/seat",
        &tool(Seat {
            hire: Named::Agent {
                agent: AgentNumber(13),
            },
        }),
    );
    for name in ["realm/chief_seat", "realm/seat"] {
        done(
            fixtures,
            name,
            vec![
                ("no_such_agent", SeatRefusal::NoSuchAgent),
                ("not_a_hire", SeatRefusal::NotAHire),
                (
                    "left",
                    SeatRefusal::Left {
                        hire: HIRE,
                        left: left(HowEnded::ByALead { lead: MAYOR }),
                    },
                ),
            ],
        );
    }
}

fn the_letters(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/tell",
        &tool(Tell {
            to: Named::Mayor,
            text: "The tests pass".into(),
            task: task(),
        }),
    );
    fixtures.args(
        "realm/tell.no_task",
        &tool(Tell {
            to: Named::Hand,
            text: "The tests pass".into(),
            task: None,
        }),
    );
    fixtures.args(
        "realm/ask",
        &tool(Ask {
            to: hire(),
            text: "Which tests fail?".into(),
            conversation: ConversationId("c1".into()),
            task: task(),
        }),
    );
    let letter_refusals = || {
        vec![
            ("yourself", LetterRefusal::Yourself),
            ("empty", LetterRefusal::Empty),
            ("no_such_agent", LetterRefusal::NoSuchAgent),
            ("no_road", LetterRefusal::NoRoad { to: HIRE }),
            ("still_hiring", LetterRefusal::StillHiring { to: HIRE }),
            (
                "left",
                LetterRefusal::Left {
                    to: HIRE,
                    left: left(HowEnded::ByTheChief),
                },
            ),
        ]
    };
    done(
        fixtures,
        "realm/tell",
        by_an_agent(SocietyTool::Tell, letter_refusals()),
    );
    done(
        fixtures,
        "realm/ask",
        by_an_agent(SocietyTool::Ask, letter_refusals()),
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

fn the_roads_and_the_tools(fixtures: &mut Fixtures) {
    let lay = || LayRoad {
        a: hire(),
        b: Named::Agent {
            agent: AgentNumber(13),
        },
    };
    fixtures.args("realm/chief_lay_road", &asked(lay()));
    fixtures.args("realm/lay_road", &tool(lay()));
    let lay_refusals = || {
        vec![
            ("no_such_agent", LayRefusal::NoSuchAgent),
            (
                "not_two_hires_of_one_hamlet",
                LayRefusal::NotTwoHiresOfOneHamlet,
            ),
            (
                "full",
                LayRefusal::Full {
                    hamlet: hamlet(),
                    most: 100,
                },
            ),
        ]
    };
    done(fixtures, "realm/chief_lay_road", lay_refusals());
    done(
        fixtures,
        "realm/lay_road",
        by_a_lead(SocietyTool::LayRoad, lay_refusals()),
    );
    let take_up = || TakeUpRoad {
        a: hire(),
        b: Named::Mayor,
    };
    fixtures.args("realm/chief_take_up_road", &asked(take_up()));
    fixtures.args("realm/take_up_road", &tool(take_up()));
    let take_up_refusals =
        || vec![("no_such_agent", TakeUpRefusal::NoSuchAgent)];
    done(fixtures, "realm/chief_take_up_road", take_up_refusals());
    done(
        fixtures,
        "realm/take_up_road",
        by_a_lead(SocietyTool::TakeUpRoad, take_up_refusals()),
    );
    fixtures.args(
        "realm/request_road",
        &tool(RequestRoad {
            to: Named::Agent {
                agent: AgentNumber(13),
            },
            why: "To ask about the tests".into(),
            task: task(),
        }),
    );
    done(
        fixtures,
        "realm/request_road",
        by_an_agent(
            SocietyTool::RequestRoad,
            vec![
                ("no_such_agent", RequestRoadRefusal::NoSuchAgent),
                ("no_lead", RequestRoadRefusal::NoLead),
            ],
        ),
    );
    fixtures.args(
        "realm/chief_set_tools",
        &asked(SetTools {
            hire: hire(),
            off: vec![
                SocietyTool::Found,
                SocietyTool::Hire,
                SocietyTool::End,
                SocietyTool::Seat,
                SocietyTool::Tell,
            ],
            on: vec![
                SocietyTool::Ask,
                SocietyTool::LayRoad,
                SocietyTool::TakeUpRoad,
                SocietyTool::RequestRoad,
                SocietyTool::SetTools,
            ],
        }),
    );
    fixtures.args(
        "realm/set_tools",
        &tool(SetTools {
            hire: hire(),
            off: vec![SocietyTool::Tell],
            on: Vec::new(),
        }),
    );
    let tools_refusals = || {
        vec![
            ("no_such_agent", ToolsRefusal::NoSuchAgent),
            ("not_a_hire", ToolsRefusal::NotAHire),
        ]
    };
    done(fixtures, "realm/chief_set_tools", tools_refusals());
    done(
        fixtures,
        "realm/set_tools",
        by_a_lead(SocietyTool::SetTools, tools_refusals()),
    );
}

fn the_limits_and_the_spending(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_set_tier",
        &asked(SetTier {
            hamlet: hamlet(),
            tier: Tier::TwoHundred,
        }),
    );
    done(
        fixtures,
        "realm/chief_set_tier",
        vec![
            ("no_such_hamlet", TierRefusal::NoSuchHamlet),
            ("too_small", TierRefusal::TooSmall { living: 120 }),
        ],
    );
    fixtures.args(
        "realm/chief_set_limits",
        &asked(SetLimits {
            hamlet: Some(hamlet()),
            budget: vec![
                BudgetLimit::Cost { micros: 5_000_000 },
                BudgetLimit::Time { ms: 3_600_000 },
            ],
        }),
    );
    fixtures.args(
        "realm/chief_set_limits.realm",
        &asked(SetLimits {
            hamlet: None,
            budget: vec![BudgetLimit::Cost { micros: 20_000_000 }],
        }),
    );
    fixtures.args(
        "realm/chief_reset_budget",
        &asked(ResetBudget {
            payer: Payer::Hamlet { hamlet: hamlet() },
        }),
    );
    fixtures.args(
        "realm/chief_reset_budget.realm",
        &asked(ResetBudget {
            payer: Payer::Realm,
        }),
    );
    for name in ["realm/chief_set_limits", "realm/chief_reset_budget"]
    {
        done(
            fixtures,
            name,
            vec![("no_such_hamlet", HamletRefusal::NoSuchHamlet)],
        );
    }
    fixtures.args(
        "realm/may_spend",
        &MaySpend {
            task: standing_task(),
        },
    );
    done(
        fixtures,
        "realm/may_spend",
        vec![
            (
                "you_left",
                SpendRefusal::Standing(Standing::YouLeft {
                    how: HowEnded::ByTheChief,
                    why: None,
                }),
            ),
            (
                "no_such_task",
                SpendRefusal::Budget(Budget::NoSuchTask),
            ),
            (
                "over_cost",
                SpendRefusal::Budget(Budget::OverCost {
                    payer: Payer::Hamlet { hamlet: hamlet() },
                    spent_micros: 5_000_120,
                    budget_micros: 5_000_000,
                }),
            ),
            (
                "over_time",
                SpendRefusal::Budget(Budget::OverTime {
                    payer: Payer::Realm,
                    spent_ms: 3_600_400,
                    budget_ms: 3_600_000,
                }),
            ),
        ],
    );
    fixtures.args(
        "realm/charge",
        &Charge {
            call: call("model-call-1"),
            task: standing_task(),
            micros: 1_250,
            took_ms: 830,
        },
    );
    fixtures.answer("realm/charge", &());
}

fn image() -> protocol::AttachedImage {
    protocol::AttachedImage {
        at: Some(5),
        id: "ab".repeat(32).parse().unwrap(),
    }
}

fn file() -> protocol::AttachedFile {
    protocol::AttachedFile {
        at: None,
        id: "cd".repeat(32).parse().unwrap(),
        name: "notes/plan.md".into(),
        size: 2_048,
    }
}

fn the_clients_calls(fixtures: &mut Fixtures) {
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
        [
            ("taken", SayAnswer::Taken { task: said() }),
            ("not_taken", SayAnswer::NotTaken { task: said() }),
        ],
        vec![
            ("no_such_agent", SayRefusal::NoSuchAgent),
            ("still_hiring", SayRefusal::StillHiring { agent: HIRE }),
            (
                "left",
                SayRefusal::Left {
                    agent: HIRE,
                    left: left(HowEnded::ByTheChief),
                },
            ),
        ],
    );
    // Not probed: the client protocol's own type keeps its own rules.
    fixtures.unprobed_args(
        "realm/answer",
        &Answer(protocol::Answer {
            choice: protocol::Choice::Reply,
            id: "1c6f8d5b-2e3a-4f4b-9cad-1e2f3a4b5c6d"
                .parse()
                .unwrap(),
            picks: None,
            text: Some("Yes, go on".into()),
            wait: "wait-3".into(),
        }),
    );
    done(
        fixtures,
        "realm/answer",
        vec![
            (
                "nothing_waits",
                AnswerRefusal::NothingWaits {
                    wait: "wait-3".into(),
                },
            ),
            ("not_offered", AnswerRefusal::NotOffered),
        ],
    );
    fixtures.args("realm/read_world", &ReadWorld);
    fixtures.answer("realm/read_world", &seen());
}

fn seen() -> Seen {
    let agent = |agent, lead, status, reach| AgentSeen {
        agent,
        hamlet: Some(hamlet()),
        lead,
        status,
        reach,
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
            agent(HIRE, Some(MAYOR), Status::Hiring, ReachSeen::None),
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
        ],
        hamlets: vec![HamletSeen {
            hamlet: hamlet(),
            name: "Riverside".into(),
            mayor: MAYOR,
            tier: Tier::Fifty,
        }],
        roads: vec![RoadSeen { a: MAYOR, b: HIRE }],
        waits: vec![WaitSeen {
            agent: HIRE,
            call: call("call-1"),
            subject: "curl https://example.com".into(),
        }],
        spend: vec![
            Spend {
                payer: Payer::Realm,
                spent_micros: 1_250_000,
                budget_micros: None,
                spent_ms: 90_000,
                budget_ms: None,
            },
            Spend {
                payer: Payer::Hamlet { hamlet: hamlet() },
                spent_micros: 250_000,
                budget_micros: Some(5_000_000),
                spent_ms: 12_000,
                budget_ms: Some(3_600_000),
            },
        ],
    }
}

fn place() -> Place {
    Place {
        version: PlaceVersion(4),
        lead: Some(MAYOR),
        hires: vec![AgentNumber(13)],
        roads: vec![AgentNumber(14)],
        tools_off: vec![SocietyTool::Found, SocietyTool::Seat],
    }
}

fn the_calls_to_agents_and_profiles(fixtures: &mut Fixtures) {
    let hands_place = || Place {
        version: PlaceVersion(1),
        lead: None,
        hires: Vec::new(),
        roads: Vec::new(),
        tools_off: Vec::new(),
    };
    // The input `agent/<n>` is created with.
    for (name, kind, place) in [
        ("hand", AgentKind::Hand, hands_place()),
        ("hire", AgentKind::Hire, place()),
    ] {
        fixtures.input(
            &format!("agent/{name}"),
            &AgentInput {
                kind,
                task: standing_task(),
                place,
            },
        );
    }
    let author = || Author {
        agent: HIRE,
        name: Some("Ada".into()),
    };
    for (name, task, input) in [
        (
            "line",
            task(),
            Input::Line {
                text: "Write the tests".into(),
                images: Vec::new(),
                files: Vec::new(),
            },
        ),
        (
            "line.attached",
            task(),
            Input::Line {
                text: "Look at this plan".into(),
                images: vec![image()],
                files: vec![file()],
            },
        ),
        (
            "letter",
            None,
            Input::Letter {
                from: author(),
                text: "The tests pass".into(),
            },
        ),
        (
            "question",
            task(),
            Input::Question {
                letter: LetterId(5),
                from: Author {
                    agent: AGENT,
                    name: None,
                },
                text: "Which tests fail?".into(),
            },
        ),
        (
            "answer",
            task(),
            Input::Answer {
                question: LetterId(5),
                from: author(),
                text: "Two of them".into(),
                conversation: ConversationId("c1".into()),
            },
        ),
    ] {
        fixtures.args(
            &format!("agent/submit.{name}"),
            &Submit {
                id: InputId("input-1".into()),
                task,
                input,
            },
        );
    }
    fixtures.answer("agent/submit", &());
    fixtures.args("agent/place", &place());
    fixtures.args("agent/place.hand", &hands_place());
    fixtures.answer("agent/place", &());
    fixtures.args(
        "agent/fill",
        &Fill {
            profile: ProfileId(2),
            branch: "hire-12".into(),
        },
    );
    fixtures.answer("agent/fill", &());
    fixtures.args(
        "profile/branch",
        &Branch {
            name: "hire-12".into(),
        },
    );
    fixtures.answer("profile/branch", &());
    fixtures.answer(
        "address/profile",
        &Profile(ProfileId(2)).address().unwrap(),
    );
}
