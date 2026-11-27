//! A letter's lane, which the realm lists once it keeps the letter,
//! before the letter is delivered.

use hamlet_acceptance::{Client, Faux, chat};
use hamlet_protocol::client::{
    AgentId, ChatRef, ChatRefKind, Choice, ClientMessage, Open,
    ReplyKind, ServerMessage, State,
};
use hamlet_testing::{Models, Scratch, Server, call, words};
use serde_json::json;

use super::commands::found;
use super::ended::hire_blank;
use super::{SERVER, look};

fn lane(kind: ChatRefKind, from: AgentId, to: AgentId) -> ChatRef {
    ChatRef {
        from: Some(from),
        kind: Some(kind),
        ..chat(to)
    }
}

async fn opened(chief: &mut Client, lane: &ChatRef) -> ServerMessage {
    let chat = lane.clone();
    chief
        .ask(|id| {
            ClientMessage::Open(Open {
                id,
                chat,
                before: None,
            })
        })
        .await
}

fn lists(realm: &State, to: AgentId, from: AgentId) -> bool {
    realm.agents.iter().any(|agent| {
        agent.id == to
            && agent
                .conversations
                .iter()
                .any(|lane| lane.with == from)
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lane_opened_before_its_letter_is_delivered_shows_the_letter()
 {
    opened_before_delivery("tell", "text", ChatRefKind::Letters)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lane_opened_before_its_question_is_delivered_shows_it() {
    opened_before_delivery("ask", "question", ChatRefKind::Questions)
        .await;
}

/// A letter or question, sent with `tool` whose words are in `field`,
/// over the parcel cap waits for a chief with its row kept, so the
/// realm lists its lane of `kind` before it is delivered.
async fn opened_before_delivery(
    tool: &str,
    field: &str,
    kind: ChatRefKind,
) {
    let make = "mkdir big; for i in $(seq 1001); do : > big/$i; done";
    let faux = Faux::new()
        .agent(
            2,
            [
                call("bash", json!({"command": make})),
                call(
                    tool,
                    json!({"to": "hire#3", field: "Everything.",
                           "paths": ["big"]}),
                ),
                words("Sent."),
            ],
        )
        .agent(3, [words("Noted.")]);
    let (home, side) =
        (Scratch::new("front-home"), Scratch::new("front-side"));
    faux.write(&side.join("faux.json"));
    let mut server = Server::start(
        &[SERVER.to_owned()],
        &home,
        Models::Faux(&side.join("faux.json")),
        &[("HAMLET_REFLECT_EVERY", "0")],
    )
    .await
    .unwrap_or_else(|failure| panic!("{failure}"));
    let mut chief = Client::join(&server).await;
    let mayor = found(&server, &mut chief, "Docs").await;
    let docs = look(&server).await.hamlets[0].id.clone();
    let address = hire_blank(&mut chief, &docs).await;
    chief.until(&address, |c| c.agent(&address).is_some()).await;
    let hire = chief.id(&address);
    let lane = lane(kind, mayor, hire);

    // A lane the realm does not list is refused.
    match opened(&mut chief, &lane).await {
        ServerMessage::Reply(reply) => {
            assert_eq!(reply.kind, ReplyKind::Error, "{reply:?}");
        }
        other => panic!("an unlisted lane opened: {other:?}"),
    }

    chief.tell(mayor, "Send everything.").await;
    let send =
        chief.wait_for("the send", |w| w.agent == Some(mayor)).await;
    assert!(lists(&chief.realm(), hire, mayor), "the lane is listed");
    let ServerMessage::Transcript(page) =
        opened(&mut chief, &lane).await
    else {
        panic!("the listed lane was refused");
    };
    assert!(page.items.is_empty(), "{page:?}");

    chief.choose(&send.id, Choice::Allow, None).await;
    chief
        .until("the letter on the same watch", |c| {
            c.texts(&lane)
                .iter()
                .any(|t| t.starts_with("Everything."))
        })
        .await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
