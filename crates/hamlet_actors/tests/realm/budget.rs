use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hamlet_actor::Action as _;
use hamlet_messages::address;
use hamlet_messages::realm::budget::{
    Budget, BudgetLimit, Charge, MaySpend, Payer, ResetBudget,
    SpendRefusal,
};
use hamlet_messages::realm::{
    Asked, Done, End, HamletId, HamletRefusal, HireAnswer, HowEnded,
    Named, SetLimits, Standing,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, CallId, RealmTask};
use hamlet_protocol::client::RequestId;
use tokio::task::JoinSet;

use super::{
    Forward, MAYOR, Realm, call, chief_hires, decoded, hamlet_with,
    hire, standing,
};

async fn limit(
    realm: &Realm,
    hamlet: Option<&HamletId>,
    budget: Vec<BudgetLimit>,
) {
    let set = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: SetLimits {
                hamlet: hamlet.cloned(),
                budget,
            },
        })
        .await
        .unwrap();
    assert_eq!(set, Done::Done);
}

fn charge(
    call: &str,
    task: &RealmTask,
    micros: u32,
    ms: u32,
) -> Charge {
    Charge {
        call: CallId(call.to_owned()),
        task: task.clone(),
        micros,
        took_ms: ms,
        tokens: None,
    }
}

async fn may_spend(
    realm: &Realm,
    agent: AgentNumber,
    task: &RealmTask,
) -> Done<SpendRefusal> {
    let may = MaySpend { task: task.clone() };
    realm.tool(agent, &may).await.unwrap()
}

async fn reset(realm: &Realm, payer: Payer) -> Done<HamletRefusal> {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act: ResetBudget { payer },
        })
        .await
        .unwrap()
}

fn over_cost(
    payer: &Payer,
    spent: u64,
    budget: u64,
) -> Done<SpendRefusal> {
    Done::Refused {
        why: SpendRefusal::Budget(Budget::OverCost {
            payer: payer.clone(),
            spent_micros: spent,
            budget_micros: budget,
        }),
    }
}

fn pays(hamlet: &HamletId) -> Payer {
    Payer::Hamlet {
        hamlet: hamlet.clone(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_charge_repeated_with_one_id_counts_once() {
    // charge one call id twice, the second time at another price:
    // one row, and the spend shows the first price once.
    let (realm, hamlet) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Cost { micros: 1 }],
    )
    .await;

    realm.tool(MAYOR, &charge("c1", &task, 6, 1)).await.unwrap();
    realm
        .tool(MAYOR, &charge("c1", &task, 20, 1))
        .await
        .unwrap();
    let spent = may_spend(&realm, MAYOR, &task).await;
    let rows = realm
        .rows(|db| {
            db.query_row(
                "SELECT COUNT(*), SUM(micros) FROM charge WHERE \
                 call = 'c1'",
                [],
                |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                },
            )
        })
        .await;

    assert_eq!(spent, over_cost(&pays(&hamlet), 6, 1));
    assert_eq!(rows, (1, 6));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn spending_past_the_budget_is_refused() {
    // set a small cost and time with Asked<SetLimits>. Charge past
    // the cost: OverCost; raise the cost and charge past the model
    // time: OverTime; ResetBudget: allowed again. A reset of a
    // hamlet the realm does not hold is refused.
    let (realm, hamlet) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    let payer = pays(&hamlet);
    let small = vec![
        BudgetLimit::Cost { micros: 10 },
        BudgetLimit::Time { ms: 100 },
    ];
    limit(&realm, Some(&hamlet), small).await;

    let fresh = may_spend(&realm, MAYOR, &task).await;
    realm
        .tool(MAYOR, &charge("c1", &task, 10, 1))
        .await
        .unwrap();
    let past_cost = may_spend(&realm, MAYOR, &task).await;
    let more = vec![BudgetLimit::Cost { micros: 1_000 }];
    limit(&realm, Some(&hamlet), more).await;
    realm
        .tool(MAYOR, &charge("c2", &task, 0, 99))
        .await
        .unwrap();
    let past_time = may_spend(&realm, MAYOR, &task).await;
    let reset_hamlet = reset(&realm, payer.clone()).await;
    let after_reset = may_spend(&realm, MAYOR, &task).await;
    let nowhere = HamletId("nowhere".to_owned());
    let reset_nowhere = reset(&realm, pays(&nowhere)).await;

    assert_eq!(fresh, Done::Done);
    assert_eq!(past_cost, over_cost(&payer, 10, 10));
    assert_eq!(
        past_time,
        Done::Refused {
            why: SpendRefusal::Budget(Budget::OverTime {
                payer,
                spent_ms: 100,
                budget_ms: 100,
            }),
        }
    );
    assert_eq!(reset_hamlet, Done::Done);
    assert_eq!(after_reset, Done::Done);
    assert_eq!(
        reset_nowhere,
        Done::Refused {
            why: HamletRefusal::NoSuchHamlet
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_agent_may_not_spend() {
    // end a hire; MaySpend as it: Standing(YouLeft). A charge it
    // makes after its end still counts toward its hamlet.
    let (realm, hamlet) = hamlet_with(0).await;
    let three = AgentNumber(3);
    let hired = chief_hires(&realm, &hamlet, hire(None)).await;
    assert_eq!(hired, HireAnswer::Hired { agent: three });
    realm.live(three).await;
    let task = standing(&realm, three);
    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Cost { micros: 1 }],
    )
    .await;
    let ended = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: End {
                hire: Named::Agent { agent: three },
                why: Some("done".to_owned()),
            },
        })
        .await
        .unwrap();
    assert_eq!(ended, Done::Done);

    let refused = may_spend(&realm, three, &task).await;
    realm.tool(three, &charge("c1", &task, 4, 1)).await.unwrap();
    let mayor =
        may_spend(&realm, MAYOR, &standing(&realm, MAYOR)).await;

    assert_eq!(
        refused,
        Done::Refused {
            why: SpendRefusal::Standing(Standing::YouLeft {
                how: HowEnded::ByTheChief,
                why: Some("done".to_owned()),
            }),
        }
    );
    assert_eq!(mayor, over_cost(&pays(&hamlet), 4, 1));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_the_realm_does_not_hold_charges_the_realm() {
    // a charge under a task the realm never opened is the realm's;
    // MaySpend under it is refused NoSuchTask.
    let (realm, _) = hamlet_with(0).await;
    let unknown = RealmTask("999".to_owned());
    let hand_task = standing(&realm, Hand::NUMBER);
    limit(&realm, None, vec![BudgetLimit::Cost { micros: 1 }]).await;

    realm
        .tool(MAYOR, &charge("c1", &unknown, 3, 1))
        .await
        .unwrap();
    let hand = may_spend(&realm, Hand::NUMBER, &hand_task).await;
    let no_task = may_spend(&realm, MAYOR, &unknown).await;

    assert_eq!(hand, over_cost(&Payer::Realm, 3, 1));
    assert_eq!(
        no_task,
        Done::Refused {
            why: SpendRefusal::Budget(Budget::NoSuchTask)
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn ten_charges_at_once_each_count() {
    // ten agents charge one small budget at once: each charge
    // counts, and the spend is their sum.
    let (realm, hamlet) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Cost { micros: 1 }],
    )
    .await;

    let mut charges = JoinSet::new();
    for n in 0..10 {
        let host = realm.host.clone();
        let forward =
            Forward {
                name: Charge::NAME.to_owned(),
                args: hamlet_actor::action::encode_positional(
                    &charge(&format!("c{n}"), &task, 7, 1),
                )
                .unwrap(),
            };
        charges.spawn(async move {
            let agent = address::Agent(AgentNumber(10 + n));
            decoded::<()>(&call(&host, &agent, &forward).await?)
        });
    }
    for charged in charges.join_all().await {
        charged.unwrap();
    }
    let spent = may_spend(&realm, MAYOR, &task).await;

    assert_eq!(spent, over_cost(&pays(&hamlet), 70, 1));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_charge_before_the_period_began_counts_nothing() {
    // a charge two days old, written with the realm stopped, is
    // outside today's period: only a charge made now counts.
    let (realm, hamlet) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    let payer = pays(&hamlet);
    let two_days_ago = SystemTime::now()
        .checked_sub(Duration::from_secs(48 * 3600))
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let payer_text = serde_json::to_string(&payer).unwrap();
    realm
        .write(&format!(
            "INSERT INTO charge (agent, call, payer, micros, \
             took_ms, at) VALUES (2, 'old', '{payer_text}', 100, 1, \
             {two_days_ago})"
        ))
        .await;
    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Cost { micros: 50 }],
    )
    .await;

    let fresh = may_spend(&realm, MAYOR, &task).await;
    realm
        .tool(MAYOR, &charge("c1", &task, 50, 1))
        .await
        .unwrap();
    let spent = may_spend(&realm, MAYOR, &task).await;

    assert_eq!(fresh, Done::Done);
    assert_eq!(spent, over_cost(&payer, 50, 50));
    realm.host.shutdown().await.unwrap();
}
