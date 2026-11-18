use hamlet_messages::agent::chat::Count;
use hamlet_messages::realm::budget::{
    Budget, BudgetLimit, BudgetRead, Charge, OpenTask, Payer, Period,
    ReadBudget, ReadBudgetRefusal, ResetBudget, SpendRefusal,
    TaskOpened, TaskTurn,
};
use hamlet_messages::realm::hamlets::{ReadRecord, Record};
use hamlet_messages::realm::workers::{
    Ended, SetAllowance, StartWorker, Started, WorkerDone,
};
use hamlet_messages::realm::world::Spend;
use hamlet_messages::realm::{
    AgentRefusal, Asked, Done, End, HowEnded, LeadRefusal, Named,
    OnlyAHire, Rank, Standing,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, RealmTask};
use hamlet_protocol::client::RequestId;

use super::chiefs::chief;
use super::{
    FOUR, MAYOR, Realm, THREE, charge, hamlet_with, limit, may_spend,
    standing, to, tool, words,
};

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
) -> Done<OnlyAHire> {
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
) -> Done<LeadRefusal<OnlyAHire>> {
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

#[tokio::test(flavor = "multi_thread")]
async fn over_its_allowance_a_hire_is_refused() {
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
    for chief_ends_it in [false, true] {
        let (realm, _) = hamlet_with(1).await;
        let task = standing(&realm, THREE);
        assert_eq!(
            chief_sets(&realm, THREE, Some(10)).await,
            Done::Done
        );
        let started = realm
            .tool(THREE, &tool("w", start("count")))
            .await
            .unwrap();
        assert_eq!(started, Started::Started { worker: FOUR });

        realm.tool(FOUR, &charge("w1", &task, 10, 1)).await.unwrap();
        let living = may_spend(&realm, FOUR, &task).await;
        let starter = may_spend(&realm, THREE, &task).await;
        if chief_ends_it {
            let end = End {
                hire: to(FOUR),
                why: None,
            };
            assert_eq!(chief(&realm, end).await, Done::Done);
        } else {
            let done = WorkerDone {
                ended: Ended::Answered {
                    text: "3".to_owned(),
                },
            };
            realm.tool(FOUR, &done).await.unwrap();
        }
        let ended = may_spend(&realm, THREE, &task).await;

        assert_eq!(living, over_allowance(THREE, 10, 10));
        assert_eq!(starter, over_allowance(THREE, 10, 10));
        assert_eq!(
            ended,
            Done::Done,
            "chief_ends_it: {chief_ends_it}"
        );
        realm.host.shutdown().await.unwrap();
    }
}

fn start(brief: &str) -> StartWorker {
    StartWorker {
        brief: brief.to_owned(),
        tools: Vec::new(),
        task: None,
        hamlet: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_hires_spend_leaves_its_leads_allowance() {
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
        refused(OnlyAHire::NotAHire {
            name: "hand".to_owned()
        })
    );
    assert_eq!(words(nobody), "no agent has the address `hire#99`");
    assert_eq!(spent, over_allowance(THREE, 5, 5));
    assert!(
        matches!(
            &left,
            Done::Refused {
                why: OnlyAHire::Left { hire, left }
            } if *hire == FOUR && left.how == HowEnded::ByTheChief
        ),
        "{left:?}"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn read_budget_shows_what_may_spend_used() {
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
        .stopped_rows(|db| {
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
    let (realm, hamlet) = hamlet_with(2).await;
    let opened = async |agent: AgentNumber, turn: TaskTurn| {
        let answer = realm
            .tool(
                agent,
                &OpenTask {
                    request: RequestId::new(),
                    turn,
                },
            )
            .await;
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
        request: RequestId::new(),
        turn: TaskTurn::Retry,
    };
    let left = realm.tool(FOUR, &retry_four).await.unwrap();
    let origins = realm
        .stopped_rows(|db| {
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

#[tokio::test(flavor = "multi_thread")]
async fn setting_an_allowance_is_recorded() {
    let (realm, hamlet) = hamlet_with(1).await;

    let set =
        lead_sets(&realm, MAYOR, "a1", allowance(THREE, Some(5)))
            .await;

    assert_eq!(set, Done::Done);
    let read = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: ReadRecord {
                hamlet: Some(hamlet),
            },
        })
        .await
        .unwrap();
    let Record::Read { lines } = read else {
        panic!("the hamlet's record is read: {read:?}");
    };
    assert_eq!(
        lines.last().map(|line| line.text.as_str()),
        Some("- hire#2 set the allowance of hire#3: small")
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowance_counts_the_period_of_the_tasks_payer() {
    let (realm, hamlet) = hamlet_with(1).await;
    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Period {
            period: Period::Month,
        }],
    )
    .await;
    assert_eq!(chief_sets(&realm, THREE, Some(10)).await, Done::Done);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let two_days_ago = i64::try_from(now).unwrap() - 2 * 86_400_000;
    realm
        .write(&format!(
            "INSERT INTO charge (agent, call, payer, micros, \
             took_ms, at) VALUES ({}, 'old', '\"realm\"', 10, 1, \
             {two_days_ago})",
            THREE.0
        ))
        .await;
    let hands = standing(&realm, Hand::NUMBER);

    let may = may_spend(&realm, THREE, &hands).await;
    let read = realm
        .tool(THREE, &tool("r1", ReadBudget { task: Some(hands) }))
        .await
        .unwrap();

    assert_eq!(may, Done::Done);
    assert!(
        matches!(
            read,
            BudgetRead::Budget {
                period: Period::Day,
                own_micros: 0,
                ..
            }
        ),
        "{read:?}"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowance_past_the_largest_is_kept_at_the_largest() {
    let (realm, _) = hamlet_with(1).await;

    let chief = chief_sets(&realm, THREE, Some(u64::MAX)).await;
    let lead = lead_sets(
        &realm,
        MAYOR,
        "a1",
        allowance(THREE, Some(u64::MAX)),
    )
    .await;
    let read = realm
        .tool(THREE, &tool("r1", ReadBudget { task: None }))
        .await
        .unwrap();

    assert_eq!((chief, lead), (Done::Done, Done::Done));
    let largest = u64::try_from(i64::MAX).unwrap();
    assert!(
        matches!(
            read,
            BudgetRead::Budget {
                allowance_micros: Some(micros),
                ..
            } if micros == largest
        ),
        "{read:?}"
    );
    realm.host.shutdown().await.unwrap();
}
