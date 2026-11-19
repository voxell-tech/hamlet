//! The realm a client is shown through the door.

use hamlet_acceptance::{Client, chat};
use hamlet_protocol::client::AgentKind;
use serde_json::json;

use super::serve;

/// What the faux Hand says once it founded the hamlet.
const FOUNDED: &str = "Docs is founded.";

#[tokio::test(flavor = "multi_thread")]
async fn hello_shows_the_hand_and_its_hamlet() {
    let (mut server, _home, _side) = serve(json!([
        {
            "calls": [{
                "name": "found_hamlet",
                "arguments": {
                    "name": "Docs",
                    "purpose": "Keep the docs.",
                },
            }],
        },
        FOUNDED,
    ]))
    .await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    chief.tell(hand, "Found a hamlet for the docs.").await;
    chief.shows(&chat, FOUNDED).await;

    let second = Client::join(&server).await;
    let realm = second.realm();
    let shown = |id| realm.agents.iter().find(|agent| agent.id == id);
    let the_hand = shown(hand).expect("the Hand is shown");
    assert_eq!(
        (the_hand.kind, the_hand.name.as_str(), &the_hand.hamlet),
        (AgentKind::Hand, "hand", &None)
    );
    let docs = second
        .hamlet("Docs")
        .expect("the hamlet the Hand founded is shown");
    let mayor = shown(docs.mayor).expect("its mayor is shown");
    assert_eq!(
        (
            mayor.kind,
            mayor.mayor,
            mayor.hamlet.as_deref(),
            mayor.parent
        ),
        (AgentKind::Hire, true, Some(docs.id.as_str()), Some(hand))
    );
    assert!(
        realm.deeds.iter().any(|deed| deed.name == "create-hamlet"),
        "the commands are offered"
    );
    second.close().await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
