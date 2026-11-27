//! The realm a client is shown through the door.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;

use hamlet_acceptance::{Client, chat};
use hamlet_protocol::client::{
    Agent, AgentId, AgentKind, Choice, ClientMessage, Doing,
    ItemKind, ReplyKind, RequestId, Say, ServerMessage, WaitDetail,
};
use hamlet_testing::Scratch;
use serde_json::json;

use super::{
    kill_node, look, said_once_hired, serve, serve_with, until,
};

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
    let (mut server, _home, _side) = serve(json!(["Here."])).await;
    let mut one = Client::join(&server).await;
    let mut two = Client::join(&server).await;
    let hand = one.hand();
    one.command(hand, "/create-hamlet Docs").await;
    let founded = until(&server, "the hamlet", |realm| {
        !realm.hamlets.is_empty()
    })
    .await;
    let (hamlet, mayor) =
        (founded.hamlets[0].id.clone(), founded.hamlets[0].mayor);
    said_once_hired(&mut one, mayor, "Are you hired?").await;
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
    }
    let now = look(&server).await.revision;
    for chief in [&mut one, &mut two] {
        chief
            .until("the realm as it stands", |chief| {
                chief.realm().revision >= now
            })
            .await;
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
    }
    one.close().await;
    two.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_is_served_before_the_node_host_is_up() {
    let slow = Scratch::new("front-slow-node");
    let node = slow.join("node");
    std::fs::write(&node, "#!/bin/sh\nsleep 3\nexec node \"$@\"\n")
        .unwrap();
    std::fs::set_permissions(&node, Permissions::from_mode(0o755))
        .unwrap();
    let (mut server, home, _side) = serve_with(
        json!(["Waited for."]),
        &[("HAMLET_NODE", node.to_str().unwrap())],
    )
    .await;
    let mut chief = Client::join(&server).await;
    assert!(
        !home.join("demo-node.pid").exists(),
        "the hello was answered only once the Node host was up"
    );
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    let reply = chief.say(hand, "Are you up?").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let answered = chief.items(&chat).into_iter().any(|item| {
        item.kind == ItemKind::Assistant && item.text == "Waited for."
    });
    assert!(
        answered,
        "the line was answered: {:?}",
        chief.texts(&chat)
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_realm_starts_the_hand_and_its_library() {
    let (mut server, _home, _side) = serve(json!(["Here."])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    chief.tell(hand, "Are you there?").await;
    chief.shows(&chat, "Here.").await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    let log = server.log();
    assert!(
        !log.contains("shelves were not read"),
        "the Hand could not read its shelves: {log}"
    );
}

/// The faux model's turn that asks the chief a question, which holds
/// its agent at work until the chief answers.
fn asks_the_chief() -> serde_json::Value {
    json!({
        "calls": [{
            "name": "ask_chief",
            "arguments": {"questions": [{"question": "Which grain?"}]},
        }],
    })
}

/// The agent `id` as `client` was last shown it.
fn shown(client: &Client, id: AgentId) -> Option<Agent> {
    client
        .realm()
        .agents
        .into_iter()
        .find(|agent| agent.id == id)
}

fn changed(client: &Client) -> bool {
    client
        .messages()
        .iter()
        .any(|message| matches!(message, ServerMessage::Change(_)))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_client_sees_the_hire_the_first_made() {
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
    let mut second = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Found a hamlet for the docs.").await;
    second
        .until("the mayor the first chief's Hand hired", |second| {
            second.hamlet("Docs").is_some_and(|docs| {
                shown(second, docs.mayor).is_some()
            })
        })
        .await;
    assert!(changed(&second), "the hire came as a change");
    second.close().await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// The Hand is followed at work on a line nobody watches, then
/// rests. A chat the door still followed would end when the Node host
/// is killed, and say so in the log before the host is back to answer
/// a line.
#[tokio::test(flavor = "multi_thread")]
async fn an_idle_realm_holds_no_agents_subscription() {
    let (mut server, home, _side) =
        serve(json!([asks_the_chief(), "Planted.", "Back."])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let line = chief.tell(hand, "Plant something.").await;
    chief
        .until("the Hand at work", |chief| {
            shown(chief, hand).is_some_and(|hand| {
                hand.busy && hand.activity.doing != Doing::Rest
            })
        })
        .await;
    let asked = chief
        .wait_for("the question", |wait| {
            matches!(wait.detail, WaitDetail::Question { .. })
        })
        .await;
    chief.choose(&asked.id, Choice::Dismiss, None).await;
    chief.reply(&line).await;
    chief
        .until("the Hand at rest", |chief| {
            shown(chief, hand).is_some_and(|hand| !hand.busy)
        })
        .await;
    kill_node(&home).await;
    let reply = chief.say(hand, "Are you back?").await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
    let log = server.log();
    assert!(
        !log.contains("chat ended"),
        "the door followed an idle agent's chat: {log}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_that_has_not_worked_is_shown_at_rest_in_its_words()
{
    let (mut server, _home, _side) = serve(json!([])).await;
    let chief = Client::join(&server).await;
    let hand = shown(&chief, chief.hand()).unwrap();
    assert_eq!(
        (hand.activity.doing, hand.activity.doing_words.as_str()),
        (Doing::Rest, "At rest")
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_sees_activity_while_the_agent_works() {
    let (mut server, _home, _side) =
        serve(json!([asks_the_chief(), "Planted."])).await;
    let mut chief = Client::join(&server).await;
    let mut watcher = Client::join(&server).await;
    let hand = chief.hand();
    let line = chief.tell(hand, "Plant something.").await;
    watcher
        .until("the Hand at work", |watcher| {
            shown(watcher, hand).is_some_and(|hand| {
                hand.busy && hand.activity.doing != Doing::Rest
            })
        })
        .await;
    let working = shown(&watcher, hand).unwrap();
    assert_ne!(working.activity.doing_words, "At rest");
    let asked = chief
        .wait_for("the question", |wait| {
            matches!(wait.detail, WaitDetail::Question { .. })
        })
        .await;
    chief.choose(&asked.id, Choice::Dismiss, None).await;
    chief.reply(&line).await;
    watcher
        .until("the Hand at rest", |watcher| {
            shown(watcher, hand).is_some_and(|hand| {
                !hand.busy && hand.activity.doing == Doing::Rest
            })
        })
        .await;
    watcher.close().await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_is_shown_a_wait_as_it_opens() {
    let (mut server, _home, _side) =
        serve(json!([asks_the_chief(), "Planted."])).await;
    let mut chief = Client::join(&server).await;
    let mut watcher = Client::join(&server).await;
    let hand = chief.hand();
    let line = chief.tell(hand, "Plant something.").await;
    let asked = watcher
        .wait_for("the question", |wait| {
            matches!(wait.detail, WaitDetail::Question { .. })
        })
        .await;
    assert!(changed(&watcher), "the wait came as a change");
    chief.choose(&asked.id, Choice::Dismiss, None).await;
    chief.reply(&line).await;
    watcher
        .until("the wait closed", |watcher| {
            watcher.waits().is_empty()
        })
        .await;
    watcher.close().await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_is_named_by_its_address_and_its_profile() {
    let (mut server, _home, _side) = serve(json!(["Here."])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let asked = chief.messages().len();
    let founded = chief.command(hand, "/create-hamlet Docs").await;
    assert!(
        founded.ends_with("under its mayor `hire#2 (Mayor)`."),
        "{founded}"
    );
    assert!(
        chief.messages()[asked..].iter().any(|message| matches!(
            message,
            ServerMessage::Change(_)
        )),
        "the asker is shown the hamlet before the reply"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
