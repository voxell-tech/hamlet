use std::fmt::Debug;

use hamlet_messages::agent::{
    AgentInput, AgentKind, Brief, Place, PlaceVersion,
};
use hamlet_messages::realm::budget::{
    Budget, ReadBudget, SpendRefusal,
};
use hamlet_messages::realm::choices::{
    ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::documents::{
    EditSettings, ReadCopies, Setting,
};
use hamlet_messages::realm::oversight::{AskChief, Object};
use hamlet_messages::realm::workers::{
    Ended, ErrandDone, SetAllowance, StartWorker,
};
use hamlet_messages::realm::{
    AgreeRoad, Ask, Done, End, HowEnded, LayRoad, Refusable,
    RequestRoad, SetTools, Standing, TakeUpRoad, ToolCall,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, ErrandNumber, RealmTask,
};

use super::{
    Callee, MAYOR, Realm, THREE, charge, error_code, hamlet_with,
    hire, made_until, may_spend, standing, tell, to, tool, until,
};

fn errand(n: u32) -> AgentNumber {
    ErrandNumber::new(n).unwrap().agent()
}

fn brief() -> Brief {
    Brief {
        instructions: Some(
            "Answer PASS or OBJECT: <reason>.".to_owned(),
        ),
        text: "the chief told hire#3: Plain line.".to_owned(),
        tools: Vec::new(),
        model: None,
    }
}

/// Keeps errand `n`'s row and its step as a start commits them; the
/// realm drives the step when it starts again.
async fn seed(realm: &Realm, n: u32, task: &RealmTask, cap: u64) {
    let task = &task.0;
    let Brief {
        instructions, text, ..
    } = brief();
    let instructions = instructions.unwrap();
    realm
        .write(&format!(
            "INSERT INTO errand (n, purpose, task, cap, subject, \
             instructions, text, at) VALUES ({n}, 'review', {task}, \
             {cap}, {task}, '{instructions}', '{text}', 0); INSERT \
             INTO step (kind, n, at) VALUES ('errand', {n}, 0);"
        ))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_errand_is_a_numbered_agent_outside_the_tree() {
    let (realm, _) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    seed(&realm, 0, &task, 1_000).await;

    let input = made_until(&realm, errand(0)).await;
    let copies =
        realm.tool(errand(0), &ReadCopies::default()).await.unwrap();

    assert_eq!(
        input,
        AgentInput {
            kind: AgentKind::Errand,
            task,
            place: Place {
                version: PlaceVersion(1),
                lead: None,
                hires: Vec::new(),
                roads: Vec::new(),
                tools_off: Vec::new(),
                folder: None,
                manual: false,
            },
            brief: Some(brief()),
            copies,
        }
    );
    let world = realm.ask(&ReadWorld).await.unwrap();
    assert!(
        world.agents.iter().all(|seen| seen.agent != errand(0)),
        "the world shows no errand: {world:?}"
    );
    assert!(
        realm
            .seen
            .fills
            .lock()
            .unwrap()
            .iter()
            .all(|(filled, _)| *filled != errand(0)),
        "an errand has no filesystem"
    );
}

async fn refused<T>(
    realm: &Realm,
    agent: AgentNumber,
    call: &str,
    act: T,
) where
    T: ToolCall + Debug,
    T::Answer: PartialEq + Debug,
    <T::Answer as Refusable>::Why: From<Standing>,
{
    let shown = format!("{act:?}");
    let answer = realm.tool(agent, &tool(call, act)).await.unwrap();
    assert_eq!(
        answer,
        T::Answer::refused(Standing::NotAMember.into()),
        "{shown} from agent {}",
        agent.0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_errand_is_refused_every_society_tool() {
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, MAYOR);
    seed(&realm, 0, &task, 1_000).await;
    made_until(&realm, errand(0)).await;
    let conversation = || ConversationId("c".to_owned());

    for agent in [errand(0), AgentNumber(77)] {
        refused(&realm, agent, "hire", hire(None)).await;
        refused(
            &realm,
            agent,
            "end",
            End {
                hire: to(THREE),
                why: None,
            },
        )
        .await;
        refused(&realm, agent, "tell", tell(to(MAYOR), "hi")).await;
        refused(
            &realm,
            agent,
            "ask",
            Ask {
                to: to(MAYOR),
                text: "how?".to_owned(),
                conversation: conversation(),
                paths: Vec::new(),
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "lay",
            LayRoad {
                a: to(MAYOR),
                b: to(THREE),
                why: None,
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "take_up",
            TakeUpRoad {
                a: to(MAYOR),
                b: to(THREE),
                why: None,
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "request",
            RequestRoad {
                to: to(MAYOR),
                why: None,
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "agree",
            AgreeRoad {
                mine: to(THREE),
                theirs: to(MAYOR),
                why: None,
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "tools",
            SetTools {
                hire: to(THREE),
                off: Vec::new(),
                on: Vec::new(),
                why: None,
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "ask_chief",
            AskChief {
                task: task.clone(),
                conversation: conversation(),
                message: None,
                questions: Vec::new(),
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "object",
            Object {
                task: task.clone(),
                conversation: conversation(),
                reason: "no".to_owned(),
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "worker",
            StartWorker {
                brief: "look".to_owned(),
                tools: Vec::new(),
                task: None,
                hamlet: None,
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "allowance",
            SetAllowance {
                hire: to(THREE),
                micros: None,
                why: None,
            },
        )
        .await;
        refused(&realm, agent, "budget", ReadBudget { task: None })
            .await;
        refused(
            &realm,
            agent,
            "nickname",
            SetNickname {
                hire: to(THREE),
                name: None,
                why: None,
            },
        )
        .await;
        refused(
            &realm,
            agent,
            "model",
            SetModel {
                of: ModelOf::Hire { hire: to(THREE) },
                model: None,
                effort: None,
                fast: None,
                why: None,
            },
        )
        .await;
    }
}

async fn outcome(
    realm: &Realm,
    n: u32,
) -> (Option<String>, Option<String>) {
    realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT ended, answer FROM errand WHERE n = ?1",
                [n],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_errand_done_twice_ends_once() {
    let (realm, _) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    seed(&realm, 0, &task, 1_000).await;
    made_until(&realm, errand(0)).await;
    let done = |ended| ErrandDone { ended };
    let pass = || Ended::Answered {
        text: "PASS".to_owned(),
    };

    realm.tool(errand(0), &done(pass())).await.unwrap();
    realm
        .tool(
            errand(0),
            &done(Ended::Failed {
                why: "too late".to_owned(),
            }),
        )
        .await
        .unwrap();

    assert_eq!(
        outcome(&realm, 0).await,
        (Some("answered".to_owned()), Some("PASS".to_owned()))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_errand_done_from_an_agent_that_is_no_errand_is_refused_as_a_wrong_caller()
 {
    let (realm, _) = hamlet_with(0).await;

    let by_a_hire = realm.tool(MAYOR, &answered()).await;

    assert_eq!(error_code(by_a_hire), "message.wrong_caller");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_errand_spends_under_its_cap() {
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, MAYOR);
    let other = standing(&realm, THREE);
    seed(&realm, 0, &task, 10).await;
    made_until(&realm, errand(0)).await;

    let under = may_spend(&realm, errand(0), &task).await;
    realm
        .tool(errand(0), &charge("c1", &task, 10, 1))
        .await
        .unwrap();
    let at_cap = may_spend(&realm, errand(0), &task).await;
    let elsewhere = may_spend(&realm, errand(0), &other).await;

    assert_eq!(under, Done::Done);
    let Done::Refused {
        why:
            SpendRefusal::Budget(Budget::OverCost {
                spent_micros: 10,
                budget_micros: 10,
                ..
            }),
    } = at_cap
    else {
        panic!("an errand at its cap spends no more: {at_cap:?}");
    };
    assert_eq!(
        elsewhere,
        Done::Refused {
            why: SpendRefusal::Budget(Budget::NoSuchTask)
        }
    );
}

async fn step_closed(realm: &Realm, n: u32) -> bool {
    realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT closed FROM step WHERE kind = 'errand' AND \
                 n = ?1",
                [n],
                |row| row.get(0),
            )
        })
        .await
}

fn answered() -> ErrandDone {
    ErrandDone {
        ended: Ended::Answered {
            text: "PASS".to_owned(),
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_errand_spends_no_more() {
    let (realm, _) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    seed(&realm, 0, &task, 1_000).await;
    made_until(&realm, errand(0)).await;

    realm.tool(errand(0), &answered()).await.unwrap();

    assert_eq!(
        may_spend(&realm, errand(0), &task).await,
        Done::Refused {
            why: SpendRefusal::Standing(Standing::YouLeft {
                how: HowEnded::Answered,
                why: None,
            })
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_errand_reads_its_copies_until_it_is_done() {
    let (realm, _) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    seed(&realm, 0, &task, 1_000).await;
    made_until(&realm, errand(0)).await;

    let living = realm.tool(errand(0), &ReadCopies::default()).await;
    realm.tool(errand(0), &answered()).await.unwrap();
    let ended = realm.tool(errand(0), &ReadCopies::default()).await;

    assert!(living.is_ok(), "{living:?}");
    assert_eq!(error_code(ended), "message.wrong_caller");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_errand_number_with_no_row_spends_nothing() {
    let (realm, _) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);

    assert_eq!(
        may_spend(&realm, errand(9), &task).await,
        Done::Refused {
            why: SpendRefusal::Budget(Budget::NoSuchTask)
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_errand_that_ended_before_it_was_made_is_driven_no_further()
 {
    let (realm, _) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    realm.seen.fail_once(Callee::Create, errand(0));
    seed(&realm, 0, &task, 1_000).await;
    until("the errand's create failed", || {
        realm.seen.has_failed(Callee::Create, errand(0))
    })
    .await;

    realm.tool(errand(0), &answered()).await.unwrap();

    assert!(step_closed(&realm, 0).await, "the errand's step closed");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_errand_reads_the_realms_model() {
    let (realm, _) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    seed(&realm, 0, &task, 10).await;
    made_until(&realm, errand(0)).await;
    super::chiefs::chief(
        &realm,
        EditSettings {
            set: vec![Setting::Model {
                model: "faux::faux-1".to_owned(),
            }],
        },
    )
    .await;

    let copies =
        realm.tool(errand(0), &ReadCopies::default()).await.unwrap();

    assert_eq!(
        copies.model.map(|part| part.value.model).as_deref(),
        Some("faux::faux-1")
    );
    assert!(copies.charter.is_some());
}
