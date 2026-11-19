//! A chief's slash commands through the door.

use hamlet_acceptance::Client;
use hamlet_protocol::Effort;
use hamlet_protocol::client::{
    AgentId, AttachedImage, Choice, ClientMessage, Edit, ReplyKind,
    RequestId, Say, ServerMessage, State,
};
use hamlet_testing::Server;
use serde_json::json;

use super::{answered, look, serve, until};

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

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_names_the_wait_it_answered() {
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
    chief.tell(hand, "Fetch both pages.").await;
    let first = oldest(
        &until(&server, "a wait", |realm| !realm.waits.is_empty())
            .await,
    )
    .unwrap();
    let answered = chief.choose(&first, Choice::Deny, None).await;
    let second = oldest(
        &until(&server, "the next wait", |realm| {
            realm.waits.iter().all(|wait| wait.id != first)
                && !realm.waits.is_empty()
        })
        .await,
    )
    .unwrap();
    let commanded =
        chief.command(hand, &format!("/deny #{second}")).await;
    let mut said = [answered.text, commanded];
    said.sort();
    assert_eq!(
        said,
        [
            "Answered /deny: hand wants to run `curl https://one.example`",
            "Answered /deny: hand wants to run `curl https://two.example`",
        ]
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// A piped `hamlet chat` sends its lines without waiting for each
/// reply: they are applied in the order sent.
#[tokio::test(flavor = "multi_thread")]
async fn one_connections_commands_apply_in_order() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    found(&server, &mut chief, "Docs").await;
    let hamlet = look(&server).await.hamlets[0].id.clone();
    let mut sent = Vec::new();
    for n in 0..20 {
        let line = Say {
            id: RequestId::new(),
            to: hand,
            text: format!("/name-hamlet {hamlet} n{n}"),
            files: Vec::new(),
            images: Vec::new(),
        };
        sent.push(line.id.clone());
        chief.send(ClientMessage::Say(line)).await;
    }
    for id in &sent {
        chief.reply(id).await;
    }
    let order = chief
        .messages()
        .iter()
        .filter_map(|message| match message {
            ServerMessage::Reply(reply) => {
                sent.iter().position(|id| *id == reply.to)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    chief.close().await;
    let named = look(&server).await.hamlets[0].name.clone();
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    assert_eq!(order, (0..20).collect::<Vec<_>>());
    assert_eq!(named, "n19");
}

/// A chief hires a blank agent and a profile pinned to one of its
/// versions; each goes live and answers a line. A version the profile
/// lacks is refused in today's words.
#[tokio::test(flavor = "multi_thread")]
async fn a_hire_goes_live_and_answers_a_line() {
    let (mut server, _home, _side) = serve(json!([
        {
            "calls": [{
                "name": "create_scholar",
                "arguments": { "name": "ada" },
            }],
        },
        "Ada is made.",
        {
            "calls": [{
                "name": "tag",
                "arguments": { "scholar": "scholar#2", "tag": "v1" },
            }],
        },
        "Ada is tagged.",
        "Here.",
        "Here.",
        "Here.",
    ]))
    .await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = hamlet_acceptance::chat(hand);
    chief.open(&chat).await;
    chief.say(hand, "Make a scholar named ada.").await;
    chief.shows(&chat, "Ada is made.").await;
    let tagged = chief.tell(hand, "Tag ada as v1.").await;
    until(&server, "the tag's wait", |realm| !realm.waits.is_empty())
        .await;
    chief.command(hand, "/allow").await;
    chief.reply(&tagged).await;
    chief.shows(&chat, "Ada is tagged.").await;
    found(&server, &mut chief, "Docs").await;
    let docs = look(&server).await.hamlets[0].id.clone();

    for hire in ["blank", "scholar#2@v1"] {
        let hired = chief
            .command(hand, &format!("/hire {hire} in {docs}"))
            .await;
        let name = hired
            .strip_suffix(" is hired.")
            .unwrap_or_else(|| panic!("{hire}: {hired}"))
            .trim_matches('`')
            .to_owned();
        let agent = until(&server, "the hire", |realm| {
            realm.agents.iter().any(|agent| agent.name == name)
        })
        .await
        .agents
        .into_iter()
        .find(|agent| agent.name == name)
        .unwrap()
        .id;
        let watched = hamlet_acceptance::chat(agent);
        chief.open(&watched).await;
        let reply =
            answered(&mut chief, agent, "Who is there?").await;
        assert_eq!(
            reply.kind,
            ReplyKind::Notice,
            "{hire}: {}",
            reply.text
        );
        chief.shows(&watched, "Here.").await;
    }
    assert_eq!(
        chief
            .refused(hand, &format!("/hire scholar#2@v9 in {docs}"))
            .await,
        "`scholar#2 (ada)` has no version `v9`"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
