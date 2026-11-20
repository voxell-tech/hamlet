use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hamlet_actor::Action as _;
use hamlet_messages::address;
use hamlet_messages::realm::budget::{
    Budget, BudgetLimit, Charge, Payer, ResetBudget, SpendRefusal,
};
use hamlet_messages::realm::{
    Asked, Done, End, HamletId, HamletRefusal, HireAnswer, HowEnded,
    Named, Standing,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, RealmTask};
use hamlet_protocol::client::RequestId;
use tokio::task::JoinSet;

use super::{
    Forward, MAYOR, Realm, call, charge, chief_hires, decoded,
    error_code, hamlet_with, hire, limit, may_spend, shown, standing,
};

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
        .stopped_rows(|db| {
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
            why: HamletRefusal::NoSuchHamlet { id: nowhere }
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cap_taken_away_bounds_no_more_spending() {
    let (realm, hamlet) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    let payer = pays(&hamlet);
    let small = vec![
        BudgetLimit::Cost { micros: 10 },
        BudgetLimit::Time { ms: 100 },
    ];
    limit(&realm, Some(&hamlet), small).await;
    realm
        .tool(MAYOR, &charge("c1", &task, 10, 100))
        .await
        .unwrap();

    let both = may_spend(&realm, MAYOR, &task).await;
    limit(&realm, Some(&hamlet), vec![BudgetLimit::NoCost]).await;
    let time_only = may_spend(&realm, MAYOR, &task).await;
    limit(&realm, Some(&hamlet), vec![BudgetLimit::NoTime]).await;
    let neither = may_spend(&realm, MAYOR, &task).await;

    assert_eq!(both, over_cost(&payer, 10, 10));
    assert_eq!(
        time_only,
        Done::Refused {
            why: SpendRefusal::Budget(Budget::OverTime {
                payer,
                spent_ms: 100,
                budget_ms: 100,
            }),
        }
    );
    assert_eq!(neither, Done::Done);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_agent_may_not_spend_or_charge() {
    // end a hire; MaySpend as it: Standing(YouLeft). A charge it
    // makes after its end is refused and counts nothing.
    let (realm, hamlet) = hamlet_with(0).await;
    let three = AgentNumber(3);
    let hired = chief_hires(&realm, &hamlet, hire(None)).await;
    assert_eq!(
        hired,
        HireAnswer::Hired {
            agent: shown(three)
        }
    );
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
    let charged = realm.tool(three, &charge("c1", &task, 4, 1)).await;
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
    assert_eq!(error_code(charged), "message.wrong_caller");
    assert_eq!(mayor, Done::Done);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_the_caller_may_not_name_charges_its_own_payer() {
    // the mayor charges under a task the realm never opened and
    // under the Hand's: both go to the mayor's hamlet, not the
    // realm; MaySpend under either is refused NoSuchTask.
    let (realm, hamlet) = hamlet_with(0).await;
    let unknown = RealmTask("999".to_owned());
    let hand_task = standing(&realm, Hand::NUMBER);
    limit(&realm, None, vec![BudgetLimit::Cost { micros: 1 }]).await;

    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Cost { micros: 1 }],
    )
    .await;
    realm
        .tool(MAYOR, &charge("c1", &unknown, 3, 1))
        .await
        .unwrap();
    realm
        .tool(MAYOR, &charge("c2", &hand_task, 2, 1))
        .await
        .unwrap();
    let hand = may_spend(&realm, Hand::NUMBER, &hand_task).await;
    let own =
        may_spend(&realm, MAYOR, &standing(&realm, MAYOR)).await;
    let refused = [
        may_spend(&realm, MAYOR, &unknown).await,
        may_spend(&realm, MAYOR, &hand_task).await,
    ];

    assert_eq!(hand, Done::Done);
    assert_eq!(own, over_cost(&pays(&hamlet), 5, 1));
    let no_task = Done::Refused {
        why: SpendRefusal::Budget(Budget::NoSuchTask),
    };
    assert_eq!(refused, [no_task.clone(), no_task]);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn ten_charges_at_once_each_count() {
    // ten hires charge one small budget at once: each charge counts,
    // and the spend is their sum.
    let (realm, hamlet) = hamlet_with(10).await;
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
        let forward = Forward {
            name: Charge::NAME.to_owned(),
            args: hamlet_actor::action::encode_positional(&charge(
                &format!("c{n}"),
                &standing(&realm, AgentNumber(3 + n)),
                7,
                1,
            ))
            .unwrap(),
        };
        charges.spawn(async move {
            let agent = address::Agent(AgentNumber(3 + n));
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

#[tokio::test(flavor = "multi_thread")]
async fn a_charge_in_the_millisecond_of_a_reset_counts() {
    // reset the hamlet's budget, charge past it, and move the
    // charge to the reset's millisecond: the charge still counts.
    let (realm, hamlet) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    let payer = pays(&hamlet);
    limit(
        &realm,
        Some(&hamlet),
        vec![BudgetLimit::Cost { micros: 5 }],
    )
    .await;

    reset(&realm, payer.clone()).await;
    realm.tool(MAYOR, &charge("c1", &task, 5, 1)).await.unwrap();
    realm
        .write(
            "UPDATE charge SET at = (SELECT MAX(at) FROM \
             budget_reset) WHERE call = 'c1'",
        )
        .await;
    let spent = may_spend(&realm, MAYOR, &task).await;

    assert_eq!(spent, over_cost(&payer, 5, 5));
    realm.host.shutdown().await.unwrap();
}
