//! The chief's commands that act on more than one actor, each sent
//! again under its request id.

use hamlet_acceptance::Client;
use hamlet_protocol::client::{
    AgentId, ClientMessage, ReplyKind, RequestId, Say, State,
};
use hamlet_testing::Server;
use serde_json::json;

use super::commands::found;
use super::ended::{ended, hire_blank};
use super::{look, serve, until};

/// Sends `line` from a connection of its own and gives its answer.
async fn sent(server: &Server, line: &Say) -> (ReplyKind, String) {
    let mut sender = Client::join(server).await;
    sender.send(ClientMessage::Say(line.clone())).await;
    let reply = sender.reply(&line.id).await;
    sender.close().await;
    (reply.kind, reply.text)
}

fn line(to: AgentId, text: &str) -> Say {
    Say {
        id: RequestId::new(),
        to,
        text: text.to_owned(),
        files: Vec::new(),
        images: Vec::new(),
    }
}

async fn hire_of(
    chief: &mut Client,
    profile: &str,
    docs: &str,
) -> String {
    let hand = chief.hand();
    let hired = chief
        .command(hand, &format!("/hire {profile} in {docs}"))
        .await;
    hired
        .strip_suffix(" is hired.")
        .unwrap_or_else(|| panic!("{hired}"))
        .trim_matches('`')
        .to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn each_command_sent_again_under_its_id_answers_as_it_did() {
    let (mut server, _home, _side) = serve(json!([
        {"calls": [{"name": "create_scholar", "arguments": {"name": "ada"}}]},
        "Ada is made.",
        {"calls": [{"name": "create_scholar", "arguments": {"name": "bob"}}]},
        "Bob is made.",
    ]))
    .await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = hamlet_acceptance::chat(hand);
    chief.open(&chat).await;
    chief.say(hand, "Make ada.").await;
    chief.shows(&chat, "Ada is made.").await;
    chief.say(hand, "Make bob.").await;
    chief.shows(&chat, "Bob is made.").await;
    let mayor = found(&server, &mut chief, "Docs").await;
    found(&server, &mut chief, "Yard").await;
    let realm = look(&server).await;
    let id_of = |name: &str| {
        realm
            .hamlets
            .iter()
            .find(|hamlet| hamlet.name == name)
            .unwrap()
            .id
            .clone()
    };
    let (docs, yard) = (id_of("Docs"), id_of("Yard"));
    let one = hire_blank(&mut chief, &docs).await;
    let two = hire_blank(&mut chief, &docs).await;
    let three = hire_of(&mut chief, "scholar#2", &docs).await;

    let mut differ = Vec::new();
    // Each first answer is waited on until a client is shown what it
    // did, so the second meets the door's view after it.
    let mut twice =
        async |text: String, shown: &dyn Fn(&State) -> bool| {
            let said = line(mayor, &text);
            let first = sent(&server, &said).await;
            until(&server, &text, shown).await;
            let again = sent(&server, &said).await;
            if first != again {
                differ.push(format!(
                    "{text}: {first:?}, then {again:?}"
                ));
            }
        };
    twice(format!("/merge {one} into {two}"), &|realm| {
        realm
            .agents
            .iter()
            .any(|agent| agent.name == one && agent.left.is_some())
    })
    .await;
    twice(format!("/restore {one}"), &|realm| {
        realm
            .agents
            .iter()
            .any(|agent| agent.name == one && agent.left.is_none())
    })
    .await;
    chief.command(mayor, &format!("/end {three}")).await;
    ended(&server, &three).await;
    twice(format!("/erase {three}"), &|realm| {
        realm.agents.iter().all(|agent| agent.name != three)
    })
    .await;
    twice(format!("/remove-hamlet {yard}"), &|realm| {
        realm.hamlets.iter().all(|hamlet| hamlet.id != yard)
    })
    .await;
    twice(format!("/restore-hamlet {yard}"), &|realm| {
        realm.hamlets.iter().any(|hamlet| hamlet.id == yard)
    })
    .await;
    chief
        .command(mayor, &format!("/remove-hamlet {yard}"))
        .await;
    twice(format!("/erase-hamlet {yard}"), &|_| true).await;
    twice("/fork scholar#2 ada2".to_owned(), &|_| true).await;
    twice("/remove-scholar scholar#3".to_owned(), &|_| true).await;
    twice("/restore-scholar scholar#3".to_owned(), &|_| true).await;
    chief.command(mayor, "/remove-scholar scholar#3").await;
    twice("/erase-scholar scholar#3".to_owned(), &|_| true).await;
    twice("/turn-away scholar#2".to_owned(), &|_| true).await;
    twice("/welcome scholar#2".to_owned(), &|_| true).await;
    twice("/move-house scholar#2 30 40".to_owned(), &|_| true).await;

    assert_eq!(differ, Vec::<String>::new());
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
