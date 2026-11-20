use hamlet_messages::realm::budget::{
    OpenTask, TaskOpened, TaskTurn,
};
use hamlet_messages::realm::client::SayAnswer;
use hamlet_messages::realm::{Asked, Found, FoundAnswer};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;
use rusqlite::OptionalExtension;

use super::{
    MAYOR, Realm, error_code, found, paid, purpose_of, say, standing,
    tool,
};

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
    // record holds `hired`, then `founded`; the mayor's row is begun.
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
    let (decisions, row) = realm
        .stopped_rows(|db| {
            let decisions = db
                .prepare(
                    "SELECT kind, hamlet, by FROM decision ORDER BY \
                     id",
                )?
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let row = db
                .query_row(
                    "SELECT n FROM step WHERE kind = 'hire' AND n = \
                     2",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            Ok((decisions, row))
        })
        .await;
    let kept = |kind: &str| {
        (kind.to_owned(), Some(hamlet.0.clone()), "chief".to_owned())
    };
    assert_eq!(decisions, [kept("hired"), kept("founded")]);
    assert_eq!(row, Some(2), "the mayor's row is begun");
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
        .stopped_rows(|db| {
            Ok((count(db, "hamlet")?, count(db, "agent")?))
        })
        .await;
    assert_eq!((hamlets, agents), (1, 2));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hands_found_repeated_from_another_turn_is_a_reused_id() {
    // the Hand founds under one call id in a turn on its own task,
    // then again on a retry's: the second is refused, and one hamlet
    // is founded.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let own = standing(&realm, Hand::NUMBER);
    let opened = realm
        .tool(
            Hand::NUMBER,
            &OpenTask {
                request: RequestId::new(),
                turn: TaskTurn::Retry,
            },
        )
        .await
        .unwrap();
    let TaskOpened::Opened { task: retry } = opened else {
        panic!("the Hand opens a retry's task: {opened:?}");
    };
    let mill = Found {
        purpose: Some("Grind the grain".to_owned()),
        ..found("Mill")
    };

    let first = realm
        .tool(Hand::NUMBER, &paid("f1", &own, mill.clone()))
        .await;
    let other =
        realm.tool(Hand::NUMBER, &paid("f1", &retry, mill)).await;

    let first = first.unwrap();
    assert!(
        matches!(first, FoundAnswer::Founded { .. }),
        "{first:?}"
    );
    assert_eq!(error_code(other), "message.id_reused");
    let hamlets = realm.stopped_rows(|db| count(db, "hamlet")).await;
    assert_eq!(hamlets, 1);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_found_from_an_agent_that_is_not_the_hand_is_refused() {
    // tool ByHand<Tool<Found>> as agent 2: `message.wrong_caller`.
    let realm = Realm::start().await;

    let called =
        realm.tool(AgentNumber(2), &tool("t1", found("Mill"))).await;

    assert_eq!(error_code(called), "message.wrong_caller");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_founding_purpose_is_kept_trimmed_and_a_blank_one_names_the_hamlet()
 {
    // found Mill with a blank purpose and Pond with " Dig \n": Mill's
    // mayor is "Mayor of Mill." and keeps it past the chief's first
    // line, and Pond's keeps "Dig".
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    for (name, purpose) in [("Mill", "  "), ("Pond", " Dig \n")] {
        let founded = realm
            .ask(&Asked {
                id: RequestId::new(),
                act: Found {
                    purpose: Some(purpose.to_owned()),
                    ..found(name)
                },
            })
            .await
            .unwrap();
        assert!(matches!(founded, FoundAnswer::Founded { .. }));
    }
    realm.live(MAYOR).await;
    realm.live(AgentNumber(3)).await;

    let said = realm
        .ask(&say(&RequestId::new(), MAYOR, "build the mill"))
        .await
        .unwrap();

    assert!(matches!(said, SayAnswer::Taken { .. }), "{said:?}");
    assert_eq!(
        (
            purpose_of(&realm, MAYOR).await,
            purpose_of(&realm, AgentNumber(3)).await
        ),
        (Some("Mayor of Mill.".to_owned()), Some("Dig".to_owned()))
    );
    realm.host.shutdown().await.unwrap();
}
