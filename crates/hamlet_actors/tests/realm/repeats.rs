use std::fmt::Debug;

use hamlet_messages::agent::chat::Count;
use hamlet_messages::realm::budget::{
    BudgetLimit, Payer, ResetBudget,
};
use hamlet_messages::realm::choices::{
    ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::documents::EditCharter;
use hamlet_messages::realm::hamlets::{MoveHamlet, NameHamlet};
use hamlet_messages::realm::oversight::{AskChief, Object, Question};
use hamlet_messages::realm::workers::SetAllowance;
use hamlet_messages::realm::{
    Ask, Asked, LayRoad, Seat, SetLimits, SetTier, SetTools,
    TakeUpRoad, Tier, ToolName,
};
use hamlet_messages::workspace::ConversationId;
use hamlet_protocol::client::RequestId;
use rusqlite::Connection;
use rusqlite::types::ValueRef;

use super::asking::chief_task;
use super::{FOUR, MAYOR, Realm, THREE, hamlet_with, to, tool};

/// Every row of every table of the realm's own, as text.
fn every_row(db: &Connection) -> rusqlite::Result<String> {
    let tables = db
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' \
             AND name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY name",
        )?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut text = String::new();
    for table in tables {
        let mut select =
            db.prepare(&format!("SELECT * FROM \"{table}\""))?;
        let columns = select.column_count();
        let mut rows = select.query([])?;
        while let Some(row) = rows.next()? {
            text.push_str(&table);
            for column in 0..columns {
                let value: ValueRef<'_> = row.get_ref(column)?;
                text.push_str(&format!(" {value:?}"));
            }
            text.push('\n');
        }
    }
    Ok(text)
}

/// Runs `call` twice: the first changes rows, and the second answers
/// as the first and changes none.
async fn repeats<O: Debug + PartialEq>(
    realm: &Realm,
    name: &str,
    call: impl AsyncFn() -> anyhow::Result<O>,
) {
    let start = realm.stopped_rows(every_row).await;
    let first = call().await.unwrap();
    let before = realm.stopped_rows(every_row).await;
    assert!(
        start != before,
        "{name} changed nothing at first: {first:?}"
    );
    let again = call().await.unwrap();
    let after = realm.stopped_rows(every_row).await;
    assert_eq!(again, first, "{name} answers again as at first");
    let gone = before
        .lines()
        .filter(|line| !after.contains(line))
        .collect::<Vec<_>>();
    let came = after
        .lines()
        .filter(|line| !before.contains(line))
        .collect::<Vec<_>>();
    assert!(
        before == after,
        "{name} changed rows when repeated: gone {gone:#?} came \
         {came:#?}"
    );
}

fn asked<T>(id: &RequestId, act: T) -> Asked<T> {
    Asked {
        id: id.clone(),
        act,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn each_act_repeated_with_its_id_answers_as_at_first_and_changes_nothing()
 {
    let (realm, hamlet) = hamlet_with(2).await;
    let chiefs = chief_task(&realm, THREE).await;
    let fours = chief_task(&realm, FOUR).await;
    let id = RequestId::new;

    let (tier, name, moved, limits, reset) =
        (id(), id(), id(), id(), id());
    repeats(&realm, "set_tier", async || {
        realm
            .ask(&asked(
                &tier,
                SetTier {
                    hamlet: hamlet.clone(),
                    tier: Tier::Hundred,
                },
            ))
            .await
    })
    .await;
    repeats(&realm, "name_hamlet", async || {
        realm
            .ask(&asked(
                &name,
                NameHamlet {
                    hamlet: hamlet.clone(),
                    name: "Weir".to_owned(),
                },
            ))
            .await
    })
    .await;
    repeats(&realm, "move_hamlet", async || {
        realm
            .ask(&asked(
                &moved,
                MoveHamlet {
                    hamlet: hamlet.clone(),
                    x: 4.0,
                    z: -2.0,
                },
            ))
            .await
    })
    .await;
    repeats(&realm, "set_limits", async || {
        realm
            .ask(&asked(
                &limits,
                SetLimits {
                    hamlet: Some(hamlet.clone()),
                    budget: vec![BudgetLimit::Cost { micros: 5 }],
                    limits: Vec::new(),
                },
            ))
            .await
    })
    .await;
    repeats(&realm, "reset_budget", async || {
        realm
            .ask(&asked(
                &reset,
                ResetBudget {
                    payer: Payer::Hamlet {
                        hamlet: hamlet.clone(),
                    },
                },
            ))
            .await
    })
    .await;

    let (nickname, model, charter) = (id(), id(), id());
    repeats(&realm, "set_nickname", async || {
        realm
            .ask(&asked(
                &nickname,
                SetNickname {
                    hire: to(THREE),
                    name: Some("Wren".to_owned()),
                    why: None,
                },
            ))
            .await
    })
    .await;
    repeats(&realm, "set_model", async || {
        realm
            .ask(&asked(
                &model,
                SetModel {
                    of: ModelOf::Hire { hire: to(THREE) },
                    model: Some("faux/one".to_owned()),
                    effort: None,
                    fast: None,
                    why: None,
                },
            ))
            .await
    })
    .await;
    repeats(&realm, "edit_charter", async || {
        realm
            .ask(&asked(
                &charter,
                EditCharter {
                    text: Some("Keep the well clean.".to_owned()),
                },
            ))
            .await
    })
    .await;

    repeats(&realm, "set_allowance", async || {
        realm
            .tool(
                MAYOR,
                &tool(
                    "a1",
                    SetAllowance {
                        hire: to(THREE),
                        micros: Some(Count(10)),
                        why: None,
                    },
                ),
            )
            .await
    })
    .await;
    repeats(&realm, "set_tools", async || {
        realm
            .tool(
                MAYOR,
                &tool(
                    "s1",
                    SetTools {
                        hire: to(THREE),
                        off: vec![ToolName {
                            name: "bash".to_owned(),
                        }],
                        on: Vec::new(),
                        why: None,
                    },
                ),
            )
            .await
    })
    .await;
    repeats(&realm, "lay_road", async || {
        realm
            .tool(
                MAYOR,
                &tool(
                    "r1",
                    LayRoad {
                        a: to(THREE),
                        b: to(FOUR),
                        why: None,
                    },
                ),
            )
            .await
    })
    .await;
    repeats(&realm, "ask", async || {
        realm
            .tool(
                THREE,
                &tool(
                    "q1",
                    Ask {
                        to: to(FOUR),
                        text: "Is the well dry?".to_owned(),
                        conversation: ConversationId("c".to_owned()),
                        paths: Vec::new(),
                    },
                ),
            )
            .await
    })
    .await;
    repeats(&realm, "take_up_road", async || {
        realm
            .tool(
                MAYOR,
                &tool(
                    "r2",
                    TakeUpRoad {
                        a: to(THREE),
                        b: to(FOUR),
                        why: None,
                    },
                ),
            )
            .await
    })
    .await;
    repeats(&realm, "object", async || {
        realm
            .tool(
                THREE,
                &tool(
                    "o1",
                    Object {
                        task: chiefs.clone(),
                        conversation: ConversationId("c".to_owned()),
                        reason: "The well is someone else's."
                            .to_owned(),
                    },
                ),
            )
            .await
    })
    .await;
    repeats(&realm, "ask_chief", async || {
        realm
            .tool(
                FOUR,
                &tool(
                    "c1",
                    AskChief {
                        task: fours.clone(),
                        conversation: ConversationId("c".to_owned()),
                        message: None,
                        questions: vec![Question {
                            question: "Which well?".to_owned(),
                            description: None,
                            options: Vec::new(),
                            text_input: true,
                        }],
                    },
                ),
            )
            .await
    })
    .await;
    let seat = id();
    repeats(&realm, "seat", async || {
        realm
            .ask(&asked(
                &seat,
                Seat {
                    hire: to(THREE),
                    why: None,
                },
            ))
            .await
    })
    .await;
    realm.host.shutdown().await.unwrap();
}
