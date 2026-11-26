//! The Node host as the server watches it.

use std::time::{Duration, Instant};

use hamlet_acceptance::Client;
use serde_json::json;

use super::{PATIENCE, kill_node, serve};

#[tokio::test(flavor = "multi_thread")]
async fn a_node_host_killed_by_a_signal_is_logged_with_its_name() {
    let (mut server, home, _side) = serve(json!(["Up."])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Are you up?").await;
    kill_node(&home).await;
    let began = Instant::now();
    while !server.log().contains(
        "WARN hamlet_host::child: the Node host ended \
         status=signal: 9 (SIGKILL)",
    ) {
        assert!(began.elapsed() < PATIENCE, "{}", server.log());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_realm_starts_with_nothing_failing_on_the_node_link() {
    let (mut server, _home, _side) = serve(json!(["Up."])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Are you up?").await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    for failed in ["a step failed", "a delivery failed"] {
        assert!(!server.log().contains(failed), "{}", server.log());
    }
}
