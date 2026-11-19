//! The realm a client is shown through the door.

use std::time::Duration;

use hamlet_acceptance::{Client, chat};
use hamlet_protocol::client::{
    AgentKind, ClientMessage, RequestId, Say, ServerMessage,
};
use serde_json::json;

use super::{look, serve, until};

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

/// Two chiefs rename one hamlet over and over at once: each is shown
/// the realm only ever moving forward, and last as it is.
#[tokio::test(flavor = "multi_thread")]
async fn each_client_is_shown_the_realm_moving_forward() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut one = Client::join(&server).await;
    let mut two = Client::join(&server).await;
    let hand = one.hand();
    one.command(hand, "/create-hamlet Docs").await;
    let hamlet = until(&server, "the hamlet", |realm| {
        !realm.hamlets.is_empty()
    })
    .await
    .hamlets[0]
        .id
        .clone();
    let mut sent = (Vec::new(), Vec::new());
    for n in 0..20 {
        for (chief, ids, tag) in [
            (&mut one, &mut sent.0, "one"),
            (&mut two, &mut sent.1, "two"),
        ] {
            let line = Say {
                id: RequestId::new(),
                to: hand,
                text: format!("/name-hamlet {hamlet} {tag}{n}"),
                files: Vec::new(),
                images: Vec::new(),
            };
            ids.push(line.id.clone());
            chief.send(ClientMessage::Say(line)).await;
        }
    }
    for (chief, ids) in [(&mut one, &sent.0), (&mut two, &sent.1)] {
        for id in ids {
            chief.reply(id).await;
        }
        chief.settle(Duration::from_secs(1)).await;
    }
    let now = look(&server).await.revision;
    for chief in [&one, &two] {
        let revisions = chief
            .messages()
            .iter()
            .filter_map(|message| match message {
                ServerMessage::State(state) => Some(state.revision),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            revisions.is_sorted(),
            "the realm went back: {revisions:?}"
        );
        assert_eq!(chief.realm().revision, now);
    }
    one.close().await;
    two.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
