//! The fixtures of the realm's messages, and of the calls the realm
//! makes to agents and profiles.

use std::fmt::{Debug, Display};

use hamlet_messages::address::{Party, Profile};
use hamlet_messages::agent::{
    AgentInput, AgentKind, Author, Brief, BuiltIn, DropInbox, Erase,
    Fill, HeldCall, HeldEnd, InboxCursor, InboxPart, Input, InputId,
    OnTask, Place, PlaceVersion, ReadInbox, Retire, Retired, Submit,
    TakeInbox,
};
use hamlet_messages::bytes::Base64;
use hamlet_messages::profile::{
    Branch, CommitId, Missing, Piece, Sha1, VersionName,
};
use hamlet_messages::realm::budget::{
    Budget, BudgetLimit, Charge, MaySpend, Payer, Period,
    ResetBudget, SpendRefusal,
};
use hamlet_messages::realm::client::{
    Answer, AnswerRefusal, ReadWorld, Say, SayAnswer, SayRefusal,
};
use hamlet_messages::realm::oversight::{
    Hold, Offer, Permission, Question,
};
use hamlet_messages::realm::parcels::{
    Measure, ParcelFile, ParcelId,
};
use hamlet_messages::realm::prompt::{
    ConversationKind, PlaceText, ReadPlace,
};
use hamlet_messages::realm::world::{
    AgentSeen, GrantSeen, GrantSource, HamletSeen, Lasting,
    ObjectionSource, ReachSeen, Revision, RoadSeen, Seen, Spend,
    Starter, Status, WaitId, WaitKind, WaitSeen,
};
use hamlet_messages::realm::{
    AgentName, AgentRefusal, AgreeRefusal, AgreeRoad, Agreed, Ask,
    Asked, Done, DoneTo, Enclosed, End, EndRefusal, Found,
    FoundAnswer, FoundRefusal, Gate, HamletId, HamletRefusal, Hire,
    HireAnswer, HireRefusal, HowEnded, LayRefusal, LayRoad,
    LeadRefusal, Left, LetterId, LetterRefusal, Limit, Named,
    OnlyAHire, ProfileId, Rank, Refusable, Reply, ReplyRefusal,
    RequestRoad, RequestRoadRefusal, ReservedAct, Review, RoadDone,
    Seat, Seated, Sent, SetLimits, SetTier, SetTools, SocietyTool,
    Standing, TakeUpRefusal, TakeUpRoad, Tell, Tier, TierRefusal,
    Tool, ToolName, Waiting,
};
use hamlet_messages::workspace::files::EditDiff;
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, ReachVersion, RealmTask,
};
use hamlet_protocol::{Effort, client as protocol};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::cases::{Cases, Union, family, named, tagged, union};
use super::{AGENT, Fixtures, call, task};

mod lifecycle;
mod parcels;
mod profiles;
mod society;

const HIRE: AgentNumber = AgentNumber(12);
const MAYOR: AgentNumber = AgentNumber(2);

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
        tagged::<OnlyAHire>(),
        tagged::<LetterRefusal>(),
        tagged::<ReplyRefusal>(),
        tagged::<LayRefusal>(),
        tagged::<TakeUpRefusal>(),
        tagged::<RequestRoadRefusal>(),
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
        tagged::<Agreed<AgentRefusal<AgreeRefusal>>>(),
        tagged::<AgreeRefusal>(),
        tagged::<Limit>(),
        named::<Review>(),
        named::<ReservedAct>(),
        tagged::<WaitKind>(),
        named::<ObjectionSource>(),
        tagged::<Starter>(),
        tagged::<Sent<AgentRefusal<LetterRefusal>>>(),
        tagged::<Waiting<AgentRefusal<LetterRefusal>>>(),
        named::<ConversationKind>(),
        named::<Lasting>(),
        named::<GrantSource>(),
        named::<HeldEnd>(),
    ]
    .into_iter()
    .chain(society::enums())
    .chain(parcels::enums())
    .chain(profiles::enums())
    .chain(lifecycle::enums())
    .collect()
}

pub fn unions() -> Vec<Union> {
    vec![
        union!(SpendRefusal {
            Standing: Standing,
            Budget: Budget,
            Held: Hold,
        }),
        agent_refusal::<LetterRefusal>(),
        agent_refusal::<RequestRoadRefusal>(),
        lead_refusal::<HireRefusal>(),
        lead_refusal::<EndRefusal>(),
        lead_refusal::<LayRefusal>(),
        lead_refusal::<TakeUpRefusal>(),
        lead_refusal::<OnlyAHire>(),
        agent_refusal::<AgreeRefusal>(),
    ]
    .into_iter()
    .chain(society::unions())
    .chain(parcels::unions())
    .chain(profiles::unions())
    .chain(lifecycle::unions())
    .collect()
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
    the_society_tools_named_in_a_gate(fixtures);
    the_limits_and_the_spending(fixtures);
    the_clients_calls(fixtures);
    the_calls_to_agents_and_profiles(fixtures);
    the_prompt_sections(fixtures);
    society::the_oversight_and_the_settings(fixtures);
    parcels::the_parcels(fixtures);
    profiles::the_profiles_through_the_realm(fixtures);
    lifecycle::the_acts_across_actors(fixtures);
}

fn answers<A>(
    fixtures: &mut Fixtures,
    name: &str,
    arms: impl IntoIterator<Item = (&'static str, A)>,
    refusals: Vec<(&'static str, A::Why)>,
) where
    A: Refusable
        + Serialize
        + DeserializeOwned
        + PartialEq
        + Debug
        + 'static,
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
    W: Serialize
        + DeserializeOwned
        + PartialEq
        + Debug
        + Display
        + 'static,
{
    answers(fixtures, name, [("done", Done::Done)], refusals);
}

/// The answers of an act on the hire `HIRE`, the refusals beside.
fn done_to<W>(
    fixtures: &mut Fixtures,
    name: &str,
    refusals: Vec<(&'static str, W)>,
) where
    W: Serialize
        + DeserializeOwned
        + PartialEq
        + Debug
        + Display
        + 'static,
{
    let done = DoneTo::Done { agent: shown(HIRE) };
    answers(fixtures, name, [("done", done)], refusals);
}

/// The answers of an act on the road between `HIRE` and agent 13.
fn road_done<W>(
    fixtures: &mut Fixtures,
    name: &str,
    refusals: Vec<(&'static str, W)>,
) where
    W: Serialize
        + DeserializeOwned
        + PartialEq
        + Debug
        + Display
        + 'static,
{
    let done = RoadDone::Done {
        a: shown(HIRE),
        b: shown(AgentNumber(13)),
    };
    answers(fixtures, name, [("done", done)], refusals);
}

fn shown(agent: AgentNumber) -> AgentName {
    AgentName {
        agent,
        name: format!("hire#{} (Ada)", agent.0),
    }
}

/// The refusals of an agent's act.
fn by_an_agent<R>(
    acts: Vec<(&'static str, R)>,
) -> Vec<(&'static str, AgentRefusal<R>)> {
    callers()
        .into_iter()
        .chain(
            acts.into_iter()
                .map(|(case, why)| (case, AgentRefusal::Act(why))),
        )
        .collect()
}

/// The refusals of an agent's act that needs a lead's powers.
fn by_a_lead<R>(
    acts: Vec<(&'static str, R)>,
) -> Vec<(&'static str, LeadRefusal<R>)> {
    callers()
        .into_iter()
        .chain([(
            "not_a_lead",
            LeadRefusal::Rank(Rank::NotALead { hire: shown(HIRE) }),
        )])
        .chain(
            acts.into_iter()
                .map(|(case, why)| (case, LeadRefusal::Act(why))),
        )
        .collect()
}

/// The refusals of the calling agent itself.
fn callers<U: From<Standing> + From<Gate>>() -> [(&'static str, U); 5]
{
    [
        ("you_left", you_left().into()),
        ("not_a_member", Standing::NotAMember.into()),
        ("reserved", Gate::Reserved { wait: WaitId(4) }.into()),
        (
            "already_refused",
            Gate::AlreadyRefused {
                reason: "not now".into(),
            }
            .into(),
        ),
        ("past_the_count", Gate::PastTheCount { refused: 3 }.into()),
    ]
}

fn you_left() -> Standing {
    Standing::YouLeft {
        how: HowEnded::ByALead { lead: shown(MAYOR) },
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
        task: None,
        conversation: None,
        act,
    }
}

/// `act` called in a turn on a task.
fn paid<T>(act: T) -> Tool<T> {
    Tool {
        task: task(),
        conversation: Some(ConversationId("c1".into())),
        ..tool(act)
    }
}

fn hamlet() -> HamletId {
    HamletId("5f0d2c1b-7a3e-4d9f-8b6a-1c2d3e4f5a6b".into())
}

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
                (
                    "bad_name",
                    FoundRefusal::BadName {
                        name: "a:b".to_owned(),
                    },
                ),
                (
                    "no_such_profile",
                    FoundRefusal::NoSuchProfile {
                        profile: ProfileId(9),
                    },
                ),
                ("no_purpose", FoundRefusal::NoPurpose),
            ],
        );
    }
    fixtures.args(
        "realm/chief_hire",
        &asked(Hire {
            hamlet: Some(hamlet()),
            under: Some(Named::MayorOf { hamlet: hamlet() }),
            profile: Some(ProfileId(2)),
            version: None,
            purpose: Some("Write the tests".into()),
            name: None,
        }),
    );
    fixtures.args(
        "realm/chief_hire.blank",
        &asked(Hire {
            hamlet: None,
            under: None,
            profile: None,
            version: None,
            purpose: None,
            name: None,
        }),
    );
    fixtures.args(
        "realm/chief_hire.named",
        &asked(Hire {
            hamlet: Some(hamlet()),
            under: None,
            profile: None,
            version: None,
            purpose: Some("Write the tests".into()),
            name: Some("ada".into()),
        }),
    );
    fixtures.args(
        "realm/hire",
        &tool(Hire {
            hamlet: None,
            under: Some(hire()),
            profile: None,
            version: None,
            purpose: Some("Check the links".into()),
            name: None,
        }),
    );
    fixtures.args(
        "realm/hire.in_a_turn",
        &Tool {
            task: Some(standing_task()),
            conversation: Some(ConversationId("c1".into())),
            ..tool(Hire {
                hamlet: None,
                under: None,
                profile: None,
                version: None,
                purpose: Some("Check the links".into()),
                name: None,
            })
        },
    );
    let hire_refusals = || {
        vec![
            ("no_hamlet", HireRefusal::NoHamlet),
            (
                "no_such_hamlet",
                HireRefusal::NoSuchHamlet { id: hamlet() },
            ),
            (
                "hamlet_erased",
                HireRefusal::HamletErased { hamlet: hamlet() },
            ),
            (
                "hamlet_removed",
                HireRefusal::HamletRemoved { hamlet: hamlet() },
            ),
            (
                "no_such_agent",
                HireRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            (
                "other_hamlet",
                HireRefusal::OtherHamlet { lead: shown(MAYOR) },
            ),
            (
                "not_a_hire",
                HireRefusal::NotAHire {
                    name: "hand".into(),
                },
            ),
            (
                "lead_left",
                HireRefusal::LeadLeft {
                    lead: shown(HIRE),
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
            (
                "apart",
                HireRefusal::Apart {
                    label: profiles::label(),
                },
            ),
            (
                "no_version",
                HireRefusal::NoVersion {
                    label: profiles::label(),
                    version: VersionName("v9".to_owned()),
                },
            ),
            (
                "unanswered",
                HireRefusal::Unanswered {
                    label: profiles::label(),
                    version: VersionName("v9".to_owned()),
                },
            ),
        ]
    };
    answers(
        fixtures,
        "realm/chief_hire",
        [("hired", HireAnswer::Hired { agent: shown(HIRE) })],
        hire_refusals(),
    );
    answers(
        fixtures,
        "realm/hire",
        [("hired", HireAnswer::Hired { agent: shown(HIRE) })],
        by_a_lead(
            hire_refusals()
                .into_iter()
                .chain([("no_purpose", HireRefusal::NoPurpose)])
                .collect(),
        ),
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
    fixtures.args(
        "realm/end.worker",
        &tool(End {
            hire: Named::Worker {
                worker: AgentNumber(15),
            },
            why: None,
        }),
    );
    let end_refusals = || {
        vec![
            (
                "no_such_agent",
                EndRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            ("the_hand", EndRefusal::TheHand),
            ("the_mayor", EndRefusal::TheMayor),
            (
                "left",
                EndRefusal::Left {
                    hire: shown(HIRE),
                    left: Left {
                        how: HowEnded::ByTheHand,
                        why: None,
                    },
                },
            ),
        ]
    };
    done(fixtures, "realm/chief_end", end_refusals());
    done_to(fixtures, "realm/end", by_a_lead(end_refusals()));
    fixtures.args(
        "realm/chief_seat",
        &asked(Seat {
            hire: hire(),
            why: None,
        }),
    );
    fixtures.args(
        "realm/seat",
        &tool(Seat {
            hire: Named::Agent {
                agent: AgentNumber(13),
            },
            why: None,
        }),
    );
    fixtures.args(
        "realm/seat.why",
        &tool(Seat {
            hire: hire(),
            why: Some(WHY.into()),
        }),
    );
    done(fixtures, "realm/chief_seat", only_a_hire());
    let seated = Seated::Done {
        agent: shown(HIRE),
        hamlet: hamlet(),
    };
    answers(
        fixtures,
        "realm/seat",
        [("done", seated)],
        only_a_hire(),
    );
}

fn the_letters(fixtures: &mut Fixtures) {
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

const WHY: &str = "To keep the tests apart";

fn only_a_hire() -> Vec<(&'static str, OnlyAHire)> {
    vec![
        (
            "no_such_agent",
            OnlyAHire::NoSuchAgent {
                address: "hire#9".into(),
            },
        ),
        (
            "not_a_hire",
            OnlyAHire::NotAHire {
                name: "worker#4".into(),
            },
        ),
        (
            "left",
            OnlyAHire::Left {
                hire: shown(HIRE),
                left: left(HowEnded::ByALead { lead: shown(MAYOR) }),
            },
        ),
    ]
}

fn the_society_tools_named_in_a_gate(fixtures: &mut Fixtures) {
    fixtures.answer(
        "realm/society_tools",
        &vec![
            SocietyTool::Found,
            SocietyTool::Hire,
            SocietyTool::End,
            SocietyTool::Seat,
            SocietyTool::Tell,
            SocietyTool::Ask,
            SocietyTool::LayRoad,
            SocietyTool::TakeUpRoad,
            SocietyTool::RequestRoad,
            SocietyTool::SetTools,
            SocietyTool::AskChief,
            SocietyTool::Object,
            SocietyTool::StartWorker,
            SocietyTool::SetAllowance,
            SocietyTool::ReadBudget,
            SocietyTool::AgreeRoad,
            SocietyTool::SetNickname,
            SocietyTool::SetModel,
            SocietyTool::ReadRecord,
            SocietyTool::CreateProfile,
            SocietyTool::ReadProfile,
            SocietyTool::EditProfile,
            SocietyTool::Tag,
            SocietyTool::Revert,
            SocietyTool::AskLibrarian,
            SocietyTool::SubmitSkill,
            SocietyTool::MergeHires,
            SocietyTool::RemoveHamlet,
        ],
    );
}

fn the_roads_and_the_tools(fixtures: &mut Fixtures) {
    let lay = || LayRoad {
        a: hire(),
        b: Named::Agent {
            agent: AgentNumber(13),
        },
        why: None,
    };
    fixtures.args("realm/chief_lay_road", &asked(lay()));
    fixtures.args("realm/lay_road", &tool(lay()));
    fixtures.args(
        "realm/lay_road.why",
        &tool(LayRoad {
            why: Some(WHY.into()),
            ..lay()
        }),
    );
    let lay_refusals = || {
        vec![
            (
                "no_such_agent",
                LayRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            ("same_ends", LayRefusal::SameEnds),
            ("not_hires", LayRefusal::NotHires),
            (
                "other_hamlet",
                LayRefusal::OtherHamlet { hire: shown(MAYOR) },
            ),
            (
                "left",
                LayRefusal::Left {
                    hire: shown(HIRE),
                    left: left(HowEnded::ByTheHand),
                },
            ),
            (
                "road_exists",
                LayRefusal::RoadExists {
                    a: shown(HIRE),
                    b: shown(AgentNumber(13)),
                },
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
    road_done(fixtures, "realm/lay_road", by_a_lead(lay_refusals()));
    fixtures.args(
        "realm/agree_road",
        &tool(AgreeRoad {
            mine: hire(),
            theirs: Named::Agent {
                agent: AgentNumber(31),
            },
            why: Some(WHY.into()),
        }),
    );
    fixtures.args(
        "realm/agree_road.mayor_of",
        &tool(AgreeRoad {
            mine: hire(),
            theirs: Named::MayorOf { hamlet: hamlet() },
            why: None,
        }),
    );
    answers(
        fixtures,
        "realm/agree_road",
        [
            (
                "waiting",
                Agreed::Waiting {
                    mine: shown(HIRE),
                    theirs: shown(AgentNumber(31)),
                },
            ),
            (
                "laid",
                Agreed::Laid {
                    mine: shown(HIRE),
                    theirs: shown(AgentNumber(31)),
                },
            ),
        ],
        by_an_agent(vec![
            ("not_a_mayor", AgreeRefusal::NotAMayor),
            (
                "no_such_agent",
                AgreeRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            ("no_roads_between", AgreeRefusal::NoRoadsBetween),
            ("not_hires", AgreeRefusal::NotHires),
            ("same_hamlet", AgreeRefusal::SameHamlet),
            (
                "road_exists",
                AgreeRefusal::RoadExists {
                    a: shown(HIRE),
                    b: shown(AgentNumber(31)),
                },
            ),
        ]),
    );
    let take_up = || TakeUpRoad {
        a: hire(),
        b: Named::Mayor,
        why: None,
    };
    fixtures.args("realm/chief_take_up_road", &asked(take_up()));
    fixtures.args("realm/take_up_road", &tool(take_up()));
    fixtures.args(
        "realm/take_up_road.why",
        &tool(TakeUpRoad {
            why: Some(WHY.into()),
            ..take_up()
        }),
    );
    let take_up_refusals = || {
        vec![
            (
                "no_such_agent",
                TakeUpRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            (
                "no_road",
                TakeUpRefusal::NoRoad {
                    a: shown(HIRE),
                    b: shown(MAYOR),
                },
            ),
        ]
    };
    done(fixtures, "realm/chief_take_up_road", take_up_refusals());
    road_done(
        fixtures,
        "realm/take_up_road",
        by_a_lead(take_up_refusals()),
    );
    fixtures.args(
        "realm/request_road",
        &paid(RequestRoad {
            to: Named::Agent {
                agent: AgentNumber(13),
            },
            why: Some("To ask about the tests".into()),
        }),
    );
    fixtures.args(
        "realm/request_road.no_why",
        &tool(RequestRoad {
            to: Named::Mayor,
            why: None,
        }),
    );
    done(
        fixtures,
        "realm/request_road",
        by_an_agent(vec![
            (
                "no_such_agent",
                RequestRoadRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            ("no_lead", RequestRoadRefusal::NoLead),
            (
                "still_hiring",
                RequestRoadRefusal::StillHiring { lead: shown(HIRE) },
            ),
        ]),
    );
    fixtures.args(
        "realm/chief_set_tools",
        &asked(SetTools {
            hire: hire(),
            off: vec![named_tool("tell"), named_tool("bash")],
            on: vec![named_tool("$Set")],
            why: None,
        }),
    );
    fixtures.args(
        "realm/set_tools",
        &tool(SetTools {
            hire: hire(),
            off: vec![named_tool("tell")],
            on: Vec::new(),
            why: None,
        }),
    );
    fixtures.args(
        "realm/set_tools.why",
        &tool(SetTools {
            hire: hire(),
            off: Vec::new(),
            on: vec![named_tool("tell")],
            why: Some(WHY.into()),
        }),
    );
    done(fixtures, "realm/chief_set_tools", only_a_hire());
    done_to(fixtures, "realm/set_tools", by_a_lead(only_a_hire()));
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
            (
                "no_such_hamlet",
                TierRefusal::NoSuchHamlet { id: hamlet() },
            ),
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
            limits: Vec::new(),
        }),
    );
    fixtures.args(
        "realm/chief_set_limits.realm",
        &asked(SetLimits {
            hamlet: None,
            budget: vec![BudgetLimit::Cost { micros: 20_000_000 }],
            limits: Vec::new(),
        }),
    );
    fixtures.args(
        "realm/chief_set_limits.no_caps",
        &asked(SetLimits {
            hamlet: Some(hamlet()),
            budget: vec![BudgetLimit::NoCost, BudgetLimit::NoTime],
            limits: Vec::new(),
        }),
    );
    fixtures.args(
        "realm/chief_set_limits.limits",
        &asked(SetLimits {
            hamlet: Some(hamlet()),
            budget: vec![BudgetLimit::Period {
                period: Period::Month,
            }],
            limits: vec![
                Limit::Questions { most: 3 },
                Limit::Refusals { most: 2 },
                Limit::Review {
                    review: Review::Always,
                },
                Limit::Review {
                    review: Review::Remote,
                },
                Limit::Review {
                    review: Review::Never,
                },
                Limit::ProtectedMinutes { minutes: 30 },
                Limit::ToldDays { days: 7 },
                Limit::Reserved {
                    act: ReservedAct::End,
                    reserved: true,
                },
                Limit::Reserved {
                    act: ReservedAct::Hire,
                    reserved: false,
                },
                Limit::Reserved {
                    act: ReservedAct::Merge,
                    reserved: true,
                },
                Limit::Reserved {
                    act: ReservedAct::Roads,
                    reserved: true,
                },
                Limit::RoadPrice { micros: 10_000 },
                Limit::RoadsMost { most: 100 },
                Limit::RoadsGrowOverDays { days: 30 },
            ],
        }),
    );
    fixtures.args(
        "realm/chief_set_limits.roads_between",
        &asked(SetLimits {
            hamlet: None,
            budget: vec![BudgetLimit::Period {
                period: Period::Day,
            }],
            limits: vec![Limit::RoadsBetween { allowed: true }],
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
    done(
        fixtures,
        "realm/chief_set_limits",
        vec![
            (
                "no_such_hamlet",
                HamletRefusal::NoSuchHamlet { id: hamlet() },
            ),
            ("misshapen_limits", HamletRefusal::MisshapenLimits),
        ],
    );
    done(
        fixtures,
        "realm/chief_reset_budget",
        vec![(
            "no_such_hamlet",
            HamletRefusal::NoSuchHamlet { id: hamlet() },
        )],
    );
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
                "not_a_member",
                SpendRefusal::Standing(Standing::NotAMember),
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
            (
                "over_allowance",
                SpendRefusal::Budget(Budget::OverAllowance {
                    hire: HIRE,
                    name: "hire#12 (Mira)".into(),
                    spent_micros: 2_000_300,
                    allowance_micros: 2_000_000,
                }),
            ),
            (
                "held",
                SpendRefusal::Held(Hold::Objection {
                    task: standing_task(),
                }),
            ),
            (
                "withdrawn",
                SpendRefusal::Held(Hold::Withdrawn {
                    task: standing_task(),
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
            tokens: None,
        },
    );
    fixtures.args(
        "realm/charge.tokens",
        &Charge {
            call: call("model-call-2"),
            task: standing_task(),
            micros: 2_500,
            took_ms: 1_200,
            tokens: Some(1_480),
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
            (
                "no_such_agent",
                SayRefusal::NoSuchAgent {
                    address: "9".to_owned(),
                },
            ),
            (
                "still_hiring",
                SayRefusal::StillHiring { agent: shown(HIRE) },
            ),
            (
                "left",
                SayRefusal::Left {
                    agent: shown(HIRE),
                    left: left(HowEnded::ByTheChief),
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
            ("not_offered", AnswerRefusal::NotOffered),
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

fn seen() -> Seen {
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

fn waits() -> Vec<WaitSeen> {
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
    ]
}

fn the_prompt_sections(fixtures: &mut Fixtures) {
    for (name, conversation, peer) in [
        ("realm/read_place", ConversationKind::Chat, None),
        (
            "realm/read_place.letters",
            ConversationKind::Letters,
            Some(HIRE),
        ),
        (
            "realm/read_place.questions",
            ConversationKind::Questions,
            Some(HIRE),
        ),
        ("realm/read_place.other", ConversationKind::Other, None),
    ] {
        fixtures.args(name, &ReadPlace { conversation, peer });
    }
    fixtures.answer(
        "realm/read_place",
        &PlaceText {
            place: "# Your place\nYou are `hire#12 (Ada)`.".into(),
            realm: Some(
                "# The realm\nNo hamlet is founded yet.".into(),
            ),
            profiles: Some(
                "# The city's scholars\n- `scholar#2`".into(),
            ),
        },
    );
    fixtures.answer(
        "realm/read_place.place_only",
        &PlaceText {
            place: "# Your place\nYou are `hand`.".into(),
            realm: None,
            profiles: None,
        },
    );
}

fn place() -> Place {
    Place {
        version: PlaceVersion(4),
        lead: Some(MAYOR),
        hires: vec![AgentNumber(13)],
        roads: vec![AgentNumber(14)],
        tools_off: vec![named_tool("found"), named_tool("bash")],
        folder: Some("/work/orchard".into()),
        manual: true,
        agrees_roads: false,
    }
}

fn the_calls_to_agents_and_profiles(fixtures: &mut Fixtures) {
    let hands_place = || Place {
        version: PlaceVersion(1),
        lead: None,
        hires: Vec::new(),
        roads: Vec::new(),
        tools_off: Vec::new(),
        folder: None,
        manual: false,
        agrees_roads: false,
    };
    for (name, kind, place, copies) in [
        ("hand", AgentKind::Hand, hands_place(), Default::default()),
        ("hire", AgentKind::Hire, place(), super::chat::copies()),
    ] {
        fixtures.input(
            &format!("agent/{name}"),
            &AgentInput {
                kind,
                task: standing_task(),
                place,
                brief: None,
                copies,
            },
        );
    }
    for (name, kind, brief) in [
        (
            "worker",
            AgentKind::Worker,
            Brief {
                instructions: None,
                text: "Check the links in the docs".into(),
                tools: vec![BuiltIn {
                    name: "read".into(),
                }],
                model: None,
            },
        ),
        (
            "errand",
            AgentKind::Errand,
            Brief {
                instructions: Some("Review the chief's line.".into()),
                text: "Write the tests".into(),
                tools: Vec::new(),
                model: Some("faux::faux-1".into()),
            },
        ),
    ] {
        fixtures.input(
            &format!("agent/{name}"),
            &AgentInput {
                kind,
                task: standing_task(),
                place: hands_place(),
                brief: Some(brief),
                copies: Default::default(),
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
                note: None,
                chief: Some("ada".into()),
            },
        ),
        (
            "line.reviewed",
            task(),
            Input::Line {
                text: "Write the tests".into(),
                images: Vec::new(),
                files: Vec::new(),
                note: Some("hire#2 reviewed this line: fine".into()),
                chief: None,
            },
        ),
        (
            "notice",
            task(),
            Input::Notice {
                text: "The chief answers: use the staging server"
                    .into(),
                conversation: Some(ConversationId("c1".into())),
                held: Some(HeldCall {
                    call: CallId("t7".into()),
                    end: HeldEnd::Ran,
                }),
            },
        ),
        (
            "notice.blocked",
            task(),
            Input::Notice {
                text: "the chief refused it: keep it".into(),
                conversation: Some(ConversationId("c1".into())),
                held: Some(HeldCall {
                    call: CallId("t8".into()),
                    end: HeldEnd::Blocked,
                }),
            },
        ),
        (
            "notice.aborted",
            task(),
            Input::Notice {
                text: "the call's wait for a chief lapsed: the turn \
                       was stopped"
                    .into(),
                conversation: Some(ConversationId("c1".into())),
                held: Some(HeldCall {
                    call: CallId("t9".into()),
                    end: HeldEnd::Aborted,
                }),
            },
        ),
        (
            "worker_answer",
            task(),
            Input::WorkerAnswer {
                from: Author {
                    agent: AgentNumber(15),
                    name: Some("worker#15".into()),
                },
                text: "Three links are broken".into(),
                conversation: Some(ConversationId("c1".into())),
            },
        ),
        (
            "line.attached",
            task(),
            Input::Line {
                text: "Look at this plan".into(),
                images: vec![image()],
                files: vec![file()],
                note: None,
                chief: None,
            },
        ),
        (
            "letter",
            None,
            Input::Letter {
                from: author(),
                text: "The tests pass".into(),
                files: Vec::new(),
            },
        ),
        (
            "letter.files",
            None,
            Input::Letter {
                from: author(),
                text: "The report".into(),
                files: vec![
                    parcel_file(0, "from-hire-12/report.md", 2_048),
                    parcel_file(
                        1,
                        "from-hire-12/figures/plot.png",
                        6 << 30,
                    ),
                ],
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
                files: Vec::new(),
            },
        ),
        (
            "question.files",
            task(),
            Input::Question {
                letter: LetterId(6),
                from: author(),
                text: "Is this plan right?".into(),
                files: vec![parcel_file(
                    0,
                    "from-hire-12/plan.md",
                    512,
                )],
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
        },
    );
    fixtures.answer("agent/fill", &());
    fixtures.args("agent/retire", &Retire);
    fixtures.answer(
        "agent/retire",
        &Retired {
            tip: CommitId(Sha1([0xcd; 20])),
        },
    );
    fixtures.args("agent/erase", &Erase);
    fixtures.answer("agent/erase", &());
    fixtures.args("agent/read_inbox", &ReadInbox { after: None });
    let cursor = || InboxCursor {
        path: "scan.pdf".into(),
        offset: 737_216,
    };
    fixtures.args(
        "agent/read_inbox.after",
        &ReadInbox {
            after: Some(cursor()),
        },
    );
    let letter = || Piece {
        path: "from-chief/letter.md".into(),
        offset: 0,
        bytes: Base64(b"a letter\n".to_vec()),
        last: true,
        executable: false,
    };
    fixtures.answer(
        "agent/read_inbox",
        &InboxPart {
            files: vec![letter()],
            next: Some(cursor()),
        },
    );
    fixtures.answer(
        "agent/read_inbox.last",
        &InboxPart {
            files: vec![letter()],
            next: None,
        },
    );
    fixtures.args(
        "agent/take_inbox",
        &TakeInbox {
            from: AgentNumber(4),
            files: vec![letter()],
        },
    );
    fixtures.answer("agent/take_inbox", &());
    fixtures.args(
        "agent/drop_inbox",
        &DropInbox {
            paths: vec!["from-chief/letter.md".into()],
        },
    );
    fixtures.answer("agent/drop_inbox", &());
    fixtures.args(
        "agent/on_task",
        &OnTask {
            tasks: vec![RealmTask("7".into())],
            within_ms: 300_000,
        },
    );
    fixtures.answer("agent/on_task", &true);
    fixtures.args(
        "profile/branch",
        &Branch {
            name: "hires/12".into(),
            from: None,
        },
    );
    fixtures.args(
        "profile/branch.from",
        &Branch {
            name: "hires/12".into(),
            from: Some(VersionName("v1".into())),
        },
    );
    done(
        fixtures,
        "profile/branch",
        vec![(
            "no_version",
            Missing::NoVersion {
                profile: ProfileId(2),
                version: VersionName("v1".into()),
            },
        )],
    );
    fixtures.answer(
        "address/profile",
        &Profile(ProfileId(2)).address().unwrap(),
    );
}

fn named_tool(name: &str) -> ToolName {
    ToolName { name: name.into() }
}

fn parcel_file(file: u32, path: &str, size: u64) -> ParcelFile {
    ParcelFile {
        parcel: ParcelId(8),
        file,
        path: path.into(),
        size,
    }
}
