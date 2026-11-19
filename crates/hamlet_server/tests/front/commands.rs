//! A chief's slash commands through the door.

use std::time::{Duration, Instant};

use hamlet_acceptance::Client;
use hamlet_protocol::Effort;
use hamlet_protocol::client::{
    AgentId, AttachedImage, ClientMessage, Edit, ReplyKind, State,
};
use hamlet_testing::Server;
use serde_json::json;

use super::serve;

/// How long a test waits for the realm to show a change; each
/// change takes well under a second here.
const PATIENCE: Duration = Duration::from_secs(20);

/// The realm as a client that says hello now is shown it.
async fn look(server: &Server) -> State {
    let client = Client::join(server).await;
    let realm = client.realm();
    client.close().await;
    realm
}

/// The realm once a hello shows it as `holds` says, read again until
/// then.
async fn until(
    server: &Server,
    what: &str,
    holds: impl Fn(&State) -> bool,
) -> State {
    let began = Instant::now();
    loop {
        let realm = look(server).await;
        if holds(&realm) {
            return realm;
        }
        assert!(began.elapsed() < PATIENCE, "waited for {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Founds the hamlet `name` from the Hand and gives its mayor.
async fn found(
    server: &Server,
    chief: &mut Client,
    name: &str,
) -> AgentId {
    let hand = chief.hand();
    chief.command(hand, &format!("/create-hamlet {name}")).await;
    let realm = until(server, "the hamlet", |realm| {
        realm.hamlets.iter().any(|hamlet| hamlet.name == name)
    })
    .await;
    realm
        .hamlets
        .iter()
        .find(|hamlet| hamlet.name == name)
        .unwrap()
        .mayor
}

#[tokio::test(flavor = "multi_thread")]
async fn effort_alone_keeps_the_model_and_fast() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let mayor = found(&server, &mut chief, "Docs").await;
    chief.command(mayor, "/model mayor faux::faux-b").await;
    chief.command(mayor, "/fast mayor on").await;
    chief.command(mayor, "/effort mayor high").await;

    let realm = look(&server).await;
    let shown = realm
        .agents
        .iter()
        .find(|agent| agent.id == mayor)
        .expect("the mayor is shown");
    assert_eq!(
        (shown.own_model.as_deref(), shown.fast, shown.effort),
        (Some("faux::faux-b"), true, Some(Effort::High))
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_with_an_attachment_is_refused() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let image = "a".repeat(64).parse().unwrap();
    let reply = chief
        .say_with(
            hand,
            "/create-hamlet Docs",
            vec![AttachedImage::last(image)],
        )
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Error, "a command takes no images or files")
    );
    assert!(
        look(&server).await.hamlets.is_empty(),
        "the refused command did nothing"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// The number of the oldest open wait.
fn oldest(realm: &State) -> Option<String> {
    realm
        .waits
        .iter()
        .min_by_key(|wait| wait.id.parse::<u64>().unwrap())
        .map(|wait| wait.id.clone())
}

/// A hamlet's mayor is still being hired here, so it holds no wait:
/// a command said to it answers the oldest of all, and one said to
/// the Hand the oldest of the Hand's.
#[tokio::test(flavor = "multi_thread")]
async fn the_oldest_wait_is_answered_when_none_is_named() {
    let (mut server, _home, _side) = serve(json!([
        {
            "calls": [
                {
                    "name": "bash",
                    "arguments": { "command": "curl https://one.example" },
                },
                {
                    "name": "bash",
                    "arguments": { "command": "curl https://two.example" },
                },
            ],
        },
        "Both pages are fetched.",
    ]))
    .await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let mayor = found(&server, &mut chief, "Docs").await;
    chief.tell(hand, "Fetch both pages.").await;

    let first = oldest(
        &until(&server, "a wait", |realm| !realm.waits.is_empty())
            .await,
    )
    .unwrap();
    let denied = chief.command(mayor, "/deny").await;
    assert!(
        denied.starts_with("Answered /deny: hand "),
        "the reply names the wait answered: {denied}"
    );
    let second = oldest(
        &until(&server, "the oldest wait answered", |realm| {
            realm.waits.iter().all(|wait| wait.id != first)
                && !realm.waits.is_empty()
        })
        .await,
    )
    .unwrap();
    chief.command(hand, "/deny").await;
    until(&server, "the Hand's wait answered", |realm| {
        realm.waits.iter().all(|wait| wait.id != second)
    })
    .await;
    assert_eq!(
        chief.say(hand, "/deny").await.text,
        "Nothing waits for you."
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_charter_edit_is_saved() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let reply = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Charter {
                id,
                text: Some("Keep every hamlet tidy.".into()),
            })
        })
        .await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Notice, "The charter is saved.")
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
