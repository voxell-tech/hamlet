use hamlet_messages::agent::chat::Count;
use hamlet_messages::realm::budget::{
    Budget, BudgetLimit, BudgetRead, Charge, OpenTask, Payer, Period,
    ReadBudget, ReadBudgetRefusal, ResetBudget, SpendRefusal,
    TaskOpened, TaskTurn,
};
use hamlet_messages::realm::workers::{
    AllowanceRefusal, SetAllowance,
};
use hamlet_messages::realm::world::Spend;
use hamlet_messages::realm::{
    AgentRefusal, Asked, Done, End, HowEnded, LeadRefusal, Named,
    Rank, Standing,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, RealmTask};
use hamlet_protocol::client::RequestId;

use super::{
    FOUR, MAYOR, Realm, THREE, charge, hamlet_with, limit, may_spend,
    standing, to, tool, words,
};

const WORKER: AgentNumber = AgentNumber(50);

fn allowance(hire: AgentNumber, micros: Option<u64>) -> SetAllowance {
    SetAllowance {
        hire: to(hire),
        micros: micros.map(Count),
        why: Some("small".to_owned()),
    }
}

async fn chief_sets(
    realm: &Realm,
    hire: AgentNumber,
    micros: Option<u64>,
) -> Done<AllowanceRefusal> {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act: allowance(hire, micros),
        })
        .await
        .unwrap()
}

async fn lead_sets(
    realm: &Realm,
    lead: AgentNumber,
    call: &str,
    act: SetAllowance,
) -> Done<LeadRefusal<AllowanceRefusal>> {
    realm.tool(lead, &tool(call, act)).await.unwrap()
}

async fn end(realm: &Realm, hire: AgentNumber) {
    let ended = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: End {
                hire: to(hire),
                why: None,
            },
        })
        .await
        .unwrap();
    assert_eq!(ended, Done::Done);
}

fn over_allowance(
    hire: AgentNumber,
    spent: u64,
    allowance: u64,
) -> Done<SpendRefusal> {
    Done::Refused {
        why: SpendRefusal::Budget(Budget::OverAllowance {
            hire,
            name: format!("hire#{}", hire.0),
            spent_micros: spent,
            allowance_micros: allowance,
        }),
    }
}

/// Keeps one worker row of `starter`'s, as a start commits it.
async fn worker(realm: &Realm, starter: AgentNumber, ended: &str) {
    realm
        .write(&format!(
            "DELETE FROM worker; INSERT INTO worker (agent, \
             starter, brief, tools, task, ended, at) VALUES ({}, \
             {}, 'b', '[]', 1, {ended}, 0)",
            WORKER.0, starter.0
        ))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn over_its_allowance_a_hire_is_refused() {
    // the mayor's allowance bounds the hire under it: their charges
    // together spend it, and the hire is refused naming the mayor.
    // A reset of the hamlet's budget leaves it spent; taking the
    // allowance away lets the hire spend again.
    let (realm, hamlet) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    let mayors = standing(&realm, MAYOR);

    let set = chief_sets(&realm, MAYOR, Some(10)).await;
    let own = chief_sets(&realm, THREE, Some(100)).await;
    realm.tool(THREE, &charge("c1", &task, 6, 1)).await.unwrap();
    let under = may_spend(&realm, THREE, &task).await;
    realm
        .tool(MAYOR, &charge("c2", &mayors, 4, 1))
        .await
        .unwrap();
    let spent = may_spend(&realm, THREE, &task).await;
    let reset = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: ResetBudget {
                payer: Payer::Hamlet { hamlet },
            },
        })
        .await
        .unwrap();
    let after_reset = may_spend(&realm, THREE, &task).await;
    let taken = chief_sets(&realm, MAYOR, None).await;
    let after_taken = may_spend(&realm, THREE, &task).await;

    assert_eq!((set, own), (Done::Done, Done::Done));
    assert_eq!(under, Done::Done);
    assert_eq!(spent, over_allowance(MAYOR, 10, 10));
    assert_eq!(
        words(spent),
        "`hire#2`'s allowance of <$0.01 is spent. Its lead or a \
         chief can raise it."
    );
    assert_eq!(reset, Done::Done);
    assert_eq!(after_reset, over_allowance(MAYOR, 10, 10));
    assert_eq!(taken, Done::Done);
    assert_eq!(after_taken, Done::Done);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_living_worker_spends_its_starters_allowance() {
    // a worker is bounded as its starter: its charge counts toward
    // the starter's allowance while it lives, and drops out once it
    // has ended.
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    assert_eq!(chief_sets(&realm, THREE, Some(10)).await, Done::Done);
    worker(&realm, THREE, "NULL").await;

    realm
        .tool(WORKER, &charge("w1", &task, 10, 1))
        .await
        .unwrap();
    let living = may_spend(&realm, WORKER, &task).await;
    let starter = may_spend(&realm, THREE, &task).await;
    worker(&realm, THREE, "'answered'").await;
    let ended = may_spend(&realm, THREE, &task).await;

    assert_eq!(living, over_allowance(THREE, 10, 10));
    assert_eq!(starter, over_allowance(THREE, 10, 10));
    assert_eq!(ended, Done::Done);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_hires_spend_leaves_its_leads_allowance() {
    // a hire's charges count toward its lead's allowance until the
    // hire is ended.
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    let mayors = standing(&realm, MAYOR);
    assert_eq!(chief_sets(&realm, MAYOR, Some(10)).await, Done::Done);

    realm
        .tool(THREE, &charge("c1", &task, 10, 1))
        .await
        .unwrap();
    let spent = may_spend(&realm, MAYOR, &mayors).await;
    end(&realm, THREE).await;
    let after = may_spend(&realm, MAYOR, &mayors).await;

    assert_eq!(spent, over_allowance(MAYOR, 10, 10));
    assert_eq!(after, Done::Done);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lead_sets_its_hires_allowance() {
    // the mayor sets its hire's allowance, which then bounds the
    // hire. A hire may not set its lead's nor its own; the Hand has
    // none; an unknown or ended hire is refused.
    let (realm, _) = hamlet_with(2).await;
    let task = standing(&realm, THREE);

    let set =
        lead_sets(&realm, MAYOR, "a1", allowance(THREE, Some(5)))
            .await;
    let leads =
        lead_sets(&realm, THREE, "a2", allowance(MAYOR, Some(5)))
            .await;
    let own =
        lead_sets(&realm, THREE, "a3", allowance(THREE, Some(50)))
            .await;
    let hand = SetAllowance {
        hire: Named::Hand,
        ..allowance(THREE, Some(5))
    };
    let hand = lead_sets(&realm, MAYOR, "a4", hand).await;
    let nobody = allowance(AgentNumber(99), Some(5));
    let nobody = lead_sets(&realm, MAYOR, "a5", nobody).await;
    realm.tool(THREE, &charge("c1", &task, 5, 1)).await.unwrap();
    let spent = may_spend(&realm, THREE, &task).await;
    end(&realm, FOUR).await;
    let left = chief_sets(&realm, FOUR, Some(5)).await;

    assert_eq!(set, Done::Done);
    let not_a_lead = |hire| Done::Refused {
        why: LeadRefusal::Rank(Rank::NotALead { hire }),
    };
    assert_eq!(leads, not_a_lead(MAYOR));
    assert_eq!(own, not_a_lead(THREE));
    let refused = |why| Done::Refused {
        why: LeadRefusal::Act(why),
    };
    assert_eq!(
        hand,
        refused(AllowanceRefusal::NotAHire {
            name: "hand".to_owned()
        })
    );
    assert_eq!(nobody, refused(AllowanceRefusal::NoSuchAgent));
    assert_eq!(spent, over_allowance(THREE, 5, 5));
    assert!(
        matches!(
            &left,
            Done::Refused {
                why: AllowanceRefusal::Left { hire, left }
            } if *hire == FOUR && left.how == HowEnded::ByTheChief
        ),
        "{left:?}"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn read_budget_shows_what_may_spend_used() {
    // past its allowance, a hire reads the numbers the refusal
    // named; past its hamlet's budget, the hamlet's. A charge keeps
    // its tokens.
    let (realm, hamlet) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    let payer = Payer::Hamlet {
        hamlet: hamlet.clone(),
    };
    assert_eq!(chief_sets(&realm, THREE, Some(8)).await, Done::Done);
    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Cost { micros: 100 }],
    )
    .await;
    let tokens = Charge {
        tokens: Some(120),
        ..charge("c1", &task, 8, 1)
    };
    realm.tool(THREE, &tokens).await.unwrap();
    let read = async |call: &str, task: Option<RealmTask>| {
        let read = ReadBudget { task };
        realm.tool(THREE, &tool(call, read)).await.unwrap()
    };

    let over = may_spend(&realm, THREE, &task).await;
    let read_over = read("r1", None).await;
    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Cost { micros: 5 }],
    )
    .await;
    let over_cost = may_spend(&realm, THREE, &task).await;
    let read_cost = read("r2", Some(task.clone())).await;
    let unknown = read("r3", Some(RealmTask("999".to_owned()))).await;
    let kept = realm
        .rows(|db| {
            db.query_row(
                "SELECT tokens FROM charge WHERE call = 'c1'",
                [],
                |row| row.get::<_, u32>(0),
            )
        })
        .await;

    assert_eq!(over, over_allowance(THREE, 8, 8));
    let budget = |budget_micros| BudgetRead::Budget {
        spend: Spend {
            payer: payer.clone(),
            spent_micros: 8,
            budget_micros,
            spent_ms: 1,
            budget_ms: None,
        },
        period: Period::Day,
        own_micros: 8,
        allowance_micros: Some(8),
    };
    assert_eq!(read_over, budget(Some(100)));
    assert_eq!(
        over_cost,
        Done::Refused {
            why: SpendRefusal::Budget(Budget::OverCost {
                payer: payer.clone(),
                spent_micros: 8,
                budget_micros: 5,
            }),
        }
    );
    assert_eq!(read_cost, budget(Some(5)));
    assert_eq!(
        unknown,
        BudgetRead::Refused {
            why: AgentRefusal::Act(ReadBudgetRefusal::NoSuchTask)
        }
    );
    assert_eq!(kept, 120);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retry_opens_a_task_of_its_own() {
    // a retry and a reflection each open a task of the caller's own,
    // paid where it works: a hire's by its hamlet, the Hand's by the
    // realm. An ended hire opens none.
    let (realm, hamlet) = hamlet_with(2).await;
    let opened = async |agent: AgentNumber, turn: TaskTurn| {
        let answer = realm.tool(agent, &OpenTask { turn }).await;
        match answer.unwrap() {
            TaskOpened::Opened { task } => task,
            refused @ TaskOpened::Refused { .. } => {
                panic!("{refused:?}")
            }
        }
    };
    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Cost { micros: 1 }],
    )
    .await;

    let retry = opened(THREE, TaskTurn::Retry).await;
    let again = opened(THREE, TaskTurn::Retry).await;
    let reflection = opened(Hand::NUMBER, TaskTurn::Reflection).await;
    realm
        .tool(THREE, &charge("c1", &retry, 1, 1))
        .await
        .unwrap();
    let hire_pays =
        may_spend(&realm, THREE, &standing(&realm, THREE)).await;
    let hand_may = may_spend(&realm, Hand::NUMBER, &reflection).await;
    end(&realm, FOUR).await;
    let retry_four = OpenTask {
        turn: TaskTurn::Retry,
    };
    let left = realm.tool(FOUR, &retry_four).await.unwrap();
    let origins = realm
        .rows(|db| {
            db.prepare(
                "SELECT origin FROM task WHERE id IN (?1, ?2, ?3) \
                 ORDER BY id",
            )?
            .query_map([&retry.0, &again.0, &reflection.0], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
        })
        .await;

    assert_ne!(retry, standing(&realm, THREE));
    assert_ne!(retry, again);
    assert_eq!(
        hire_pays,
        Done::Refused {
            why: SpendRefusal::Budget(Budget::OverCost {
                payer: Payer::Hamlet { hamlet },
                spent_micros: 1,
                budget_micros: 1,
            }),
        }
    );
    assert_eq!(hand_may, Done::Done);
    assert_eq!(
        left,
        TaskOpened::Refused {
            why: Standing::YouLeft {
                how: HowEnded::ByTheChief,
                why: None,
            }
        }
    );
    assert_eq!(origins, ["retry", "retry", "reflection"]);
    realm.host.shutdown().await.unwrap();
}
