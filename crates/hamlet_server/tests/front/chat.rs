//! A chief's chat through the door: lines, their replies and the
//! frames of the chat a client follows.

use hamlet_acceptance::{Client, chat};
use hamlet_protocol::client::{
    ClientMessage, Open, RequestId, Say, ServerMessage,
};
use serde_json::json;

use super::serve;

/// What the faux model answers the first line it is given.
const REPLY: &str = "The door carried this reply.";

#[tokio::test(flavor = "multi_thread")]
async fn a_client_following_an_agents_chat_gets_its_reply() {
    let (mut server, _home, _side) = serve(json!([REPLY])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    chief
        .send(ClientMessage::Say(Say {
            id: RequestId::new(),
            to: hand,
            text: "Say something.".into(),
            files: Vec::new(),
            images: Vec::new(),
        }))
        .await;
    chief.shows(&chat, REPLY).await;
    let frames = chief
        .messages()
        .iter()
        .map(|message| serde_json::to_string(message).unwrap())
        .collect::<Vec<_>>();
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    for frame in &frames {
        assert!(
            !frame.contains(&server.token),
            "a frame carried the token: {frame}"
        );
    }
    assert!(
        !server.log().contains(&server.token),
        "the server printed the token"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_that_breaks_the_schema_is_refused_alone() {
    let (mut server, _home, _side) = serve(json!([REPLY])).await;
    let mut chief = Client::join(&server).await;
    let broken = RequestId::new();
    chief
        .send_raw(
            &json!({ "type": "open", "id": broken }).to_string(),
        )
        .await;
    assert!(
        matches!(
            chief.answer(&broken).await,
            ServerMessage::Fault(_)
        ),
        "a message outside the schema is refused with a fault"
    );
    let hand = chief.hand();
    let opened = chief
        .ask(|id| {
            ClientMessage::Open(Open {
                id,
                chat: chat(hand),
                before: None,
            })
        })
        .await;
    assert!(
        matches!(opened, ServerMessage::Transcript(_)),
        "the connection serves the next message: {opened:?}"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
