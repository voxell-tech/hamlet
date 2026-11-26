//! A chief's slash commands through the door.

use hamlet_acceptance::Client;
use hamlet_protocol::Effort;
use hamlet_protocol::client::{
    AgentId, AgentKind, Answer, AttachedImage, Choice, ClientMessage,
    Edit, Keeper, ReplyKind, RequestId, Say, ScholarId,
    ServerMessage, State,
};
use hamlet_testing::{Scratch, Server};
use serde_json::json;

use super::ended::{ended, hire_blank};
use super::{look, said_once_hired, serve, until};

/// Founds the hamlet `name` from the Hand and gives its mayor.
pub(super) async fn found(
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

/// A hamlet's mayor holds no wait: a command said to it answers the
/// oldest of all, and one said to the Hand the oldest of the Hand's.
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
async fn an_answer_sent_after_a_stop_finds_its_wait_closed() {
    for round in 1..=3 {
        let (mut server, _home, _side) = serve(json!([
            {
                "calls": [{
                    "name": "bash",
                    "arguments": { "command": "curl https://one.example" },
                }],
            },
            "Fetched.",
        ]))
        .await;
        let mut chief = Client::join(&server).await;
        let hand = chief.hand();
        let line = chief.tell(hand, "Fetch the page.").await;
        let wait = oldest(
            &until(&server, "a wait", |realm| {
                !realm.waits.is_empty()
            })
            .await,
        )
        .unwrap();
        let stop = chief.tell(hand, "/stop").await;
        let answer = RequestId::new();
        chief
            .send(ClientMessage::Answer(Answer {
                choice: Choice::Allow,
                id: answer.clone(),
                picks: None,
                text: None,
                wait: wait.clone(),
            }))
            .await;
        chief.reply(&stop).await;
        let answered = chief.reply(&answer).await;
        assert_eq!(
            (answered.kind, answered.text),
            (
                ReplyKind::Error,
                "nothing waits here with that id".to_owned()
            ),
            "round {round}"
        );
        chief.reply(&line).await;
        chief.close().await;
        assert_eq!(server.stop().await, Some(0), "{}", server.log());
    }
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

    for hire in ["blank", "scholar#2", "scholar#2@v1"] {
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
            said_once_hired(&mut chief, agent, "Who is there?").await;
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

#[tokio::test(flavor = "multi_thread")]
async fn a_chief_names_a_living_worker_by_its_address() {
    let busy = json!({
        "calls": [{
            "name": "bash",
            "arguments": { "command": "sleep 1" },
        }],
    });
    let mut responses = vec![json!({
        "calls": [{
            "name": "start_worker",
            "arguments": { "task": "Count the shelves and answer." },
        }],
    })];
    responses.extend(std::iter::repeat_n(busy, 60));
    let (mut server, _home, _side) =
        serve(serde_json::Value::Array(responses)).await;
    let mut chief = Client::join(&server).await;
    let mayor = found(&server, &mut chief, "Docs").await;
    chief.tell(mayor, "Start a counter.").await;
    let worker = until(&server, "the worker", |realm| {
        realm
            .agents
            .iter()
            .any(|agent| agent.kind == AgentKind::Worker)
    })
    .await
    .agents
    .into_iter()
    .find(|agent| agent.kind == AgentKind::Worker)
    .unwrap()
    .id;
    let address = format!("worker#{}", worker.0);

    assert_eq!(
        chief
            .command(mayor, &format!("/nickname {address} Zed"))
            .await,
        format!("`{address}` is called Zed now.")
    );
    assert_eq!(
        chief
            .command(mayor, &format!("/model {address} faux::faux-1"))
            .await,
        format!("`{address}` runs faux::faux-1.")
    );
    assert_eq!(
        chief.refused(mayor, &format!("/end {address}")).await,
        format!("`{address} (Zed)` is not a hire")
    );
    assert!(
        look(&server)
            .await
            .agents
            .iter()
            .any(|agent| agent.id == worker && agent.left.is_none()),
        "the worker goes on"
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chief_stopping_or_telling_an_ended_hire_reads_that_no_agent_has_its_address()
 {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let mayor = found(&server, &mut chief, "Docs").await;
    let docs = look(&server).await.hamlets[0].id.clone();
    let address = hire_blank(&mut chief, &docs).await;
    chief.command(mayor, &format!("/end {address}")).await;
    ended(&server, &address).await;

    let none = format!("no agent has the address `{address}`");
    assert_eq!(
        chief.refused(mayor, &format!("/stop {address}")).await,
        none
    );
    assert_eq!(
        chief
            .refused(mayor, &format!("/to {address} Read again."))
            .await,
        none
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_house_names_a_missing_profile_before_numbers_it_cannot_read()
 {
    let (mut server, _home, _side) = serve(json!([
        {"calls": [{"name": "create_scholar", "arguments": {"name": "ada"}}]},
        "Ada is made.",
    ]))
    .await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = hamlet_acceptance::chat(hand);
    chief.open(&chat).await;
    chief.say(hand, "Make ada.").await;
    chief.shows(&chat, "Ada is made.").await;

    let missing =
        chief.refused(hand, "/move-house scholar#99 four 5").await;
    let unread =
        chief.refused(hand, "/move-house scholar#2 four 5").await;
    let short = chief.refused(hand, "/move-house scholar#99 4").await;

    assert_eq!(missing, "no scholar has the address `scholar#99`");
    assert_eq!(
        unread,
        "Use /move-house <scholar#id> <x> <z> [degrees]"
    );
    assert_eq!(short, "no scholar has the address `scholar#99`");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_house_refuses_numbers_that_are_not_finite_after_the_profile_and_the_harbor()
 {
    let (mut server, _home, _side) = serve(json!([
        {"calls": [{"name": "create_scholar", "arguments": {"name": "ada"}}]},
        "Ada is made.",
    ]))
    .await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = hamlet_acceptance::chat(hand);
    chief.open(&chat).await;
    chief.say(hand, "Make ada.").await;
    chief.shows(&chat, "Ada is made.").await;
    let at_harbor = "the scholar `scholar#2 (ada)` waits at the \
                     harbor; welcome it into the city first";

    let mut read = Vec::new();
    for line in [
        "/move-house scholar#2 1e999 5",
        "/move-house scholar#2 5 -inf",
        "/move-house scholar#2 NaN 5",
        "/move-house scholar#2 1 2 inf",
        "/move-house scholar#99 inf 5",
    ] {
        read.push(chief.refused(hand, line).await);
    }
    chief.command(hand, "/turn-away scholar#2").await;
    read.push(
        chief.refused(hand, "/move-house scholar#2 inf 5").await,
    );

    assert_eq!(
        read,
        [
            "`inf, 5` is not an address",
            "`5, -inf` is not an address",
            "`NaN, 5` is not an address",
            "`1, 2` is not an address",
            "no scholar has the address `scholar#99`",
            at_harbor,
        ]
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// A server whose Hand made the profile `scholar#2 (ada)`, its faux
/// model answering with `more` after, and its chief.
async fn with_ada(
    more: &[serde_json::Value],
) -> (Server, Scratch, Scratch, Client) {
    let mut responses = vec![
        json!({"calls": [{"name": "create_scholar", "arguments": {"name": "ada"}}]}),
        json!("Ada is made."),
    ];
    responses.extend_from_slice(more);
    let (server, home, side) = serve(json!(responses)).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = hamlet_acceptance::chat(hand);
    chief.open(&chat).await;
    chief.say(hand, "Make ada.").await;
    chief.shows(&chat, "Ada is made.").await;
    (server, home, side, chief)
}

/// Writes `text` to ada's memory file `notes.md`.
async fn note(chief: &mut Client, text: &str) {
    let text = text.to_owned();
    let saved = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Memory {
                id,
                of: Keeper::Scholar(ScholarId(2)),
                path: "notes.md".into(),
                text,
            })
        })
        .await;
    assert_eq!(
        (saved.kind, saved.text.as_str()),
        (ReplyKind::Notice, "The memory is saved.")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tag_names_a_version_and_versions_lists_it() {
    let (mut server, _home, _side, mut chief) = with_ada(&[]).await;
    let hand = chief.hand();
    let none = chief.command(hand, "/versions scholar#2").await;
    let tagged =
        chief.command(hand, "/tag scholar#2 v1 the first").await;
    let taken = chief.refused(hand, "/tag scholar#2 v1").await;
    let listed = chief.command(hand, "/versions scholar#2").await;
    let missing = chief.refused(hand, "/versions scholar#9").await;

    assert!(
        none.starts_with(
            "No versions yet; /tag names one.\nLatest commits of \
             main:\n- "
        ),
        "{none}"
    );
    assert_eq!(tagged, "Tagged scholar#2 (ada) as v1.");
    assert_eq!(
        taken,
        "the version `v1` exists already; choose another"
    );
    assert!(listed.starts_with("Versions:\n- v1 ("), "{listed}");
    assert!(
        listed.contains("): the first\nLatest commits of main:\n- "),
        "{listed}"
    );
    assert_eq!(missing, "no scholar has the address `scholar#9`");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn revert_undoes_a_commit_of_a_profiles_main() {
    let (mut server, _home, _side, mut chief) = with_ada(&[]).await;
    let hand = chief.hand();
    note(&mut chief, "Metric.").await;
    chief.command(hand, "/tag scholar#2 noted").await;

    let reverted = chief
        .command(hand, "/revert scholar#2 noted not wanted")
        .await;
    let papers = chief.papers(Keeper::Scholar(ScholarId(2))).await;

    assert!(
        reverted.starts_with("Reverted ")
            && reverted.ends_with(" on scholar#2 (ada)'s main."),
        "{reverted}"
    );
    assert!(papers.memory.is_empty(), "{papers:?}");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_takes_a_file_out_of_a_profiles_history() {
    let (mut server, _home, _side, mut chief) = with_ada(&[]).await;
    let hand = chief.hand();
    note(&mut chief, "A secret.").await;

    let purged = chief
        .command(hand, "/purge scholar#2 memory/notes.md")
        .await;
    let outside = chief.refused(hand, "/purge scholar#2 ../x").await;
    let papers = chief.papers(Keeper::Scholar(ScholarId(2))).await;

    assert!(
        purged.starts_with(
            "Purged memory/notes.md from scholar#2 (ada): "
        ) && purged.ends_with(" branches rewritten."),
        "{purged}"
    );
    assert_eq!(outside, "there is no file `../x` here");
    assert!(papers.memory.is_empty(), "{papers:?}");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn scholars_lists_the_citys_profiles() {
    let (mut server, _home, _side, mut chief) = with_ada(&[]).await;
    let hand = chief.hand();
    let listed = chief.command(hand, "/scholars").await;
    assert_eq!(listed, "- scholar#2 (ada)");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hamlet_is_founded_from_a_profile_version() {
    let (mut server, _home, _side, mut chief) = with_ada(&[]).await;
    let hand = chief.hand();
    chief.command(hand, "/tag scholar#2 v1").await;
    let missing = chief
        .refused(hand, "/create-hamlet scholar#2@v9 Nowhere")
        .await;
    chief
        .command(hand, "/create-hamlet scholar#2@v1 Docs")
        .await;

    assert_eq!(missing, "`scholar#2 (ada)` has no version `v9`");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_with_no_change_lists_them() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let mayor = found(&server, &mut chief, "Docs").await;
    let every = chief.command(mayor, "/tools mayor").await;
    chief.command(mayor, "/tools mayor -bash").await;
    let off = chief.command(mayor, "/tools mayor").await;

    assert_eq!(every, "`mayor` has every tool.");
    assert_eq!(off, "`mayor` has these off: bash");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn history_shows_the_hamlets_decisions() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let mayor = found(&server, &mut chief, "Docs").await;
    let hand = chief.hand();
    let decided = chief.command(mayor, "/history").await;
    let nowhere = chief.refused(hand, "/history").await;

    assert!(decided.starts_with("- "), "{decided}");
    assert!(
        decided.contains("founded the hamlet under"),
        "{decided}"
    );
    assert_eq!(
        nowhere,
        "Say this to a hamlet's mayor or one of its hires."
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_with_no_number_lists_the_sessions() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = hamlet_acceptance::chat(hand);
    chief.open(&chat).await;
    chief.say(hand, "First.").await;
    chief.shows(&chat, "Heard: First.").await;
    chief.command(hand, "/new").await;
    chief.say(hand, "Second.").await;
    chief.shows(&chat, "Heard: Second.").await;

    let listed = chief.command(hand, "/resume").await;

    assert_eq!(listed, "- session 1\n- session 2");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn dream_names_one_profile_or_hire() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let bare = chief.refused(hand, "/dream").await;
    let missing = chief.refused(hand, "/dream scholar#9").await;

    assert_eq!(bare, "Use /dream <scholar#id|address>");
    assert_eq!(missing, "no scholar has the address `scholar#9`");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
