use hamlet_messages::realm::NeedsChief;
use hamlet_messages::realm::client::ReadWorld;

use super::{THREE, hamlet_with, needs};

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_wait_that_needs_nothing_is_refused() {
    let (realm, _) = hamlet_with(1).await;

    let asked = realm
        .call_as_workspace(&NeedsChief {
            needs: Vec::new(),
            ..needs(THREE, "c1")
        })
        .await;
    let seen = realm.ask(&ReadWorld).await.unwrap();

    assert!(asked.is_err(), "{asked:?}");
    assert_eq!(seen.waits, []);
    realm.host.shutdown().await.unwrap();
}
