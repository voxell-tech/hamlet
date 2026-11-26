//! Reading and editing what the profiles, the library and the agents
//! keep, through the door.

use hamlet_acceptance::{Client, chat};
use hamlet_protocol::client::ReplyKind;
use hamlet_testing::{Models, Scratch, Server};
use serde_json::json;

use super::SERVER;

#[tokio::test(flavor = "multi_thread")]
async fn the_product_entry_answers_from_a_faux_script() {
    let home = Scratch::new("front-home");
    let side = Scratch::new("front-side");
    std::fs::write(
        side.join("faux.json"),
        json!({
            "models": ["faux-1"],
            "price": { "input": 3, "output": 15 },
            "responses": [
                { "thinking": "Weighing it.", "text": "From the script." }
            ],
        })
        .to_string(),
    )
    .unwrap();
    let mut server = Server::start(
        &[SERVER.to_owned(), "--actors".to_owned()],
        &home,
        Models::Faux(&side.join("faux.json")),
        &[],
    )
    .await
    .unwrap_or_else(|failure| panic!("{failure}"));
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    let reply = chief.say(hand, "Hello.").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    chief.shows(&chat, "From the script.").await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
