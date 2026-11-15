use hamlet_messages::realm::{Asked, Found, FoundAnswer, Tier, Tool};
use hamlet_messages::workspace::{AgentNumber, CallId};
use hamlet_protocol::client::RequestId;
use rusqlite::OptionalExtension;

use super::{Realm, error_code};

fn found(name: &str) -> Found {
    Found {
        name: name.to_owned(),
        mayor: None,
        tier: Tier::Fifty,
        purpose: None,
    }
}

fn count(
    db: &rusqlite::Connection,
    table: &str,
) -> rusqlite::Result<i64> {
    db.query_row(
        &format!("SELECT count(*) FROM {table}"),
        [],
        |row| row.get(0),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn founding_makes_the_mayor_agent_2() {
    // ask Asked<Found>; the answer is Founded { mayor: 2 }; the
    // record holds `founded`; the mayor's row is begun.
    let realm = Realm::start().await;

    let answer = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Mill"),
        })
        .await
        .unwrap();

    let FoundAnswer::Founded { hamlet, mayor } = answer else {
        panic!("the chief founds a hamlet: {answer:?}");
    };
    assert_eq!(mayor, AgentNumber(2));
    let (decision, row) = realm
        .rows(|db| {
            let decision = db
                .query_row(
                    "SELECT kind, hamlet, by FROM decision",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;
            let row = db
                .query_row(
                    "SELECT closed FROM step WHERE kind = 'hire' \
                     AND n = 2",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            Ok((decision, row))
        })
        .await;
    assert_eq!(
        decision,
        Some((
            "founded".to_owned(),
            Some(hamlet.0),
            "chief".to_owned()
        ))
    );
    assert_eq!(row, Some(0), "the mayor's row is begun and open");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_found_founds_once() {
    // ask the same Asked<Found> twice: one hamlet, the same answer;
    // the same id with another name fails with `message.id_reused`.
    let realm = Realm::start().await;
    let asked = Asked {
        id: RequestId::new(),
        act: found("Mill"),
    };

    let first = realm.ask(&asked).await.unwrap();
    let again = realm.ask(&asked).await.unwrap();
    let other = realm
        .ask(&Asked {
            id: asked.id.clone(),
            act: found("Weir"),
        })
        .await;

    assert_eq!(first, again);
    assert_eq!(error_code(other), "message.id_reused");
    let (hamlets, agents) = realm
        .rows(|db| Ok((count(db, "hamlet")?, count(db, "agent")?)))
        .await;
    assert_eq!((hamlets, agents), (1, 2));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_found_from_an_agent_that_is_not_the_hand_is_refused() {
    // tool ByHand<Tool<Found>> as agent 2: `message.wrong_caller`.
    let realm = Realm::start().await;

    // The Hand's form has the wire name of the agent's `Tool<Found>`.
    let called = realm
        .tool(
            AgentNumber(2),
            &Tool {
                call: CallId("t1".to_owned()),
                act: found("Mill"),
            },
        )
        .await;

    assert_eq!(error_code(called), "message.wrong_caller");
    realm.host.shutdown().await.unwrap();
}
