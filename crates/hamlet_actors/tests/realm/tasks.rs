use hamlet_messages::realm::budget::{
    OpenTask, TaskOpened, TaskTurn,
};
use hamlet_protocol::client::RequestId;

use super::{MAYOR, error_code, hamlet_with};

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
