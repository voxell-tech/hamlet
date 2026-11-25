//! An ended hire restored and erased through the door.

use hamlet_acceptance::Client;
use hamlet_testing::Server;
use serde_json::json;

use super::commands::found;
use super::{look, serve, until};

async fn ended(server: &Server, address: &str) {
    until(server, "the hire's end", |realm| {
        realm.agents.iter().any(|agent| {
            agent.name == address && agent.left.is_some()
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chief_restores_ends_and_erases_a_hire_and_then_finds_no_agent()
 {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let mayor = found(&server, &mut chief, "Docs").await;
    let docs = look(&server).await.hamlets[0].id.clone();
    let hired =
        chief.command(hand, &format!("/hire blank in {docs}")).await;
    let address = hired
        .strip_suffix(" is hired.")
        .unwrap_or_else(|| panic!("{hired}"))
        .trim_matches('`')
        .to_owned();
    chief.command(mayor, &format!("/end {address}")).await;
    ended(&server, &address).await;

    let restored =
        chief.command(mayor, &format!("/restore {address}")).await;
    until(&server, "the hire back", |realm| {
        realm.agents.iter().any(|agent| {
            agent.name == address && agent.left.is_none()
        })
    })
    .await;
    chief.command(mayor, &format!("/end {address}")).await;
    ended(&server, &address).await;
    let erased =
        chief.command(mayor, &format!("/erase {address}")).await;
    let again =
        chief.refused(mayor, &format!("/restore {address}")).await;

    assert_eq!(restored, format!("`{address}` is back."));
    assert_eq!(erased, format!("`{address}` was erased for good."));
    assert_eq!(
        again,
        format!("no agent has the address `{address}`")
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
