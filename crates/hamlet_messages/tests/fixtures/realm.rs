//! The fixtures of the realm's messages, and of the calls the realm
//! makes to agents and profiles.

use std::fmt::{Debug, Display};

use hamlet_messages::agent::{AgentKind, HeldEnd, Input};
use hamlet_messages::profile::VersionName;
use hamlet_messages::realm::budget::{
    Budget, BudgetLimit, Payer, SpendRefusal,
};
use hamlet_messages::realm::client::{
    AnswerRefusal, SayAnswer, SayRefusal,
};
use hamlet_messages::realm::hamlets::{
    Found, FoundAnswer, FoundRefusal,
};
use hamlet_messages::realm::letters::{
    LetterRefusal, ReplyRefusal, Sent, Waiting,
};
use hamlet_messages::realm::oversight::Hold;
use hamlet_messages::realm::parcels::{ParcelFile, ParcelId};
use hamlet_messages::realm::prompt::ConversationKind;
use hamlet_messages::realm::roads::{
    AgreeRefusal, Agreed, LayRefusal, RequestRoadRefusal, RoadDone,
    TakeUpRefusal,
};
use hamlet_messages::realm::world::{
    GrantSource, Lasting, ObjectionSource, ReachSeen, Starter,
    Status, WaitId, WaitKind,
};
use hamlet_messages::realm::{
    AgentName, AgentRefusal, Asked, Done, DoneTo, End, EndRefusal,
    Gate, HamletId, HamletRefusal, Hire, HireAnswer, HireRefusal,
    HowEnded, LeadRefusal, Left, Limit, Named, OnlyAHire, ProfileId,
    Rank, Refusable, ReservedAct, Review, Seat, Seated, SocietyTool,
    Standing, Tier, TierRefusal, Tool, ToolName,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, RealmTask,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use self::agents::{
    the_calls_to_agents_and_profiles, the_prompt_sections,
};
use self::client::the_clients_calls;
use self::letters::the_letters;
use self::limits::the_limits_and_the_spending;
use self::roads::the_roads_and_the_tools;
use super::cases::{Cases, Union, family, named, tagged, union};
use super::{Fixtures, call, task};

mod agents;
mod client;
mod letters;
mod lifecycle;
mod limits;
mod parcels;
mod profiles;
mod roads;
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
            version: Some(VersionName("v2".into())),
            tier: Tier::Fifty,
            purpose: Some("Keep the docs in order".into()),
        }),
    );
    fixtures.args(
        "realm/chief_found.blank",
        &asked(Found {
            name: "Riverside".into(),
            mayor: None,
            version: None,
            tier: Tier::Hundred,
            purpose: None,
        }),
    );
    fixtures.args(
        "realm/found",
        &tool(Found {
            name: "Harbor".into(),
            mayor: None,
            version: None,
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
                (
                    "no_version",
                    FoundRefusal::NoVersion {
                        label: profiles::label(),
                        version: VersionName("v9".to_owned()),
                    },
                ),
                (
                    "unanswered",
                    FoundRefusal::Unanswered {
                        label: profiles::label(),
                        version: VersionName("v9".to_owned()),
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
            (
                "on_chiefs_task",
                EndRefusal::OnChiefsTask { name: shown(HIRE) },
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
