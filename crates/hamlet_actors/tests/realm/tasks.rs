use hamlet_messages::realm::Done;
use hamlet_messages::realm::budget::{
    Budget, OpenTask, SpendRefusal, TaskOpened, TaskTurn,
};
use hamlet_protocol::client::{Choice, RequestId};

use super::asking::{chief_task, given, objection, open_waits};
use super::{
    MAYOR, THREE, answer, error_code, hamlet_with, may_spend, paid,
    tell, to, tool, until,
};

#[tokio::test(flavor = "multi_thread")]
async fn an_open_task_repeated_with_its_request_opens_one_task() {
    // the mayor opens a retry's task under one request twice, then
    // a reflection's under the same request: the repeat answers the
    // first task and the third is refused; a new request opens a
    // second task.
    let (realm, _) = hamlet_with(0).await;
    let request = RequestId::new();
    let open = |request: &RequestId, turn| OpenTask {
        request: request.clone(),
        turn,
    };

    let first =
        realm.tool(MAYOR, &open(&request, TaskTurn::Retry)).await;
    let again =
        realm.tool(MAYOR, &open(&request, TaskTurn::Retry)).await;
    let other_turn = realm
        .tool(MAYOR, &open(&request, TaskTurn::Reflection))
        .await;
    let fresh = realm
        .tool(MAYOR, &open(&RequestId::new(), TaskTurn::Retry))
        .await
        .unwrap();
    let opened = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT COUNT(*) FROM task WHERE agent = 2 AND \
                 origin = 'retry'",
                [],
                |row| row.get::<_, i64>(0),
            )
        })
        .await;

    let first = first.unwrap();
    assert!(matches!(first, TaskOpened::Opened { .. }), "{first:?}");
    assert_eq!(again.unwrap(), first);
    assert_eq!(error_code(other_turn), "message.id_reused");
    assert_ne!(fresh, first);
    assert_eq!(opened, 2);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_a_held_letter_carries_is_spent_on_only_once_delivered()
 {
    // 3 objects to its chief's line, then tells the mayor on that
    // line's task: the letter is held, and the mayor may not spend on
    // its task. Once a chief lets the line go on, the letter reaches
    // the mayor, and the mayor may.
    let (realm, _) = hamlet_with(1).await;
    let task = chief_task(&realm, THREE).await;
    realm
        .tool(THREE, &tool("o1", objection(&task, "no")))
        .await
        .unwrap();
    let told = realm
        .tool(
            THREE,
            &paid("t1", &task, tell(to(MAYOR), "it is done")),
        )
        .await
        .unwrap();

    let while_held = may_spend(&realm, MAYOR, &task).await;
    let waits = open_waits(&realm, THREE).await;
    realm
        .ask(&answer(&waits[0].0.to_string(), Choice::Go, None))
        .await
        .unwrap();
    until("the letter reaches the mayor", || {
        given(&realm, MAYOR, 0)
            .contains(&"letter: it is done".to_owned())
    })
    .await;
    let once_delivered = may_spend(&realm, MAYOR, &task).await;

    assert_eq!(told, Done::Done);
    assert_eq!(
        while_held,
        Done::Refused {
            why: SpendRefusal::Budget(Budget::NoSuchTask)
        }
    );
    assert_eq!(once_delivered, Done::Done);
    realm.host.shutdown().await.unwrap();
}
