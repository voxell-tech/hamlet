//! Reading and editing what the profiles, the library and the agents
//! keep, through the door.

use hamlet_acceptance::{Client, chat};
use hamlet_protocol::client::{
    AgentId, Check, ClientMessage, Edit, EditSettingsSet,
    EditSettingsSetWorkspace, Keeper, MoveLodges, ProfileId,
    ReplyKind, RequestId, SettingsViewWorkspaceMoveState,
};
use hamlet_testing::{Models, Scratch, Server};
use serde_json::json;

use super::commands::found;
use super::worker::{TOKEN, Worker};
use super::{SERVER, look, serve};

const TEA: &str = "---\ndescription: Tea\n---\nEarl grey.\n";

const NO_SETTING: EditSettingsSet = EditSettingsSet {
    allow_net: None,
    character: None,
    effort: None,
    endpoints: None,
    model: None,
    newcomers: None,
    reflect_every: None,
    workspace: None,
};

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
        &[SERVER.to_owned()],
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

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_memory_edit_of_an_agent_is_in_its_papers() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let edit = |path: &str, text: &str| {
        let (path, text) = (path.to_owned(), text.to_owned());
        move |id| {
            ClientMessage::Edit(Edit::Memory {
                id,
                of: Keeper::Agent(hand),
                path,
                text,
            })
        }
    };
    let saved = chief.request(edit("units.md", "Metric.\n")).await;
    let outside =
        chief.request(edit("../units.md", "Imperial.")).await;
    let papers = chief.papers(Keeper::Agent(hand)).await;

    assert_eq!(
        (saved.kind, saved.text.as_str()),
        (ReplyKind::Notice, "The memory is saved.")
    );
    assert_eq!(
        (outside.kind, outside.text.as_str()),
        (ReplyKind::Error, "there is no file `../units.md` here")
    );
    let kept = papers
        .memory
        .iter()
        .find(|file| file.path == "units.md")
        .unwrap_or_else(|| panic!("{papers:?}"));
    assert_eq!(kept.text, "Metric.\n");
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profiles_character_and_memory_edits_are_in_its_papers() {
    let (mut server, _home, _side) = serve(json!([
        {"calls": [{"name": "create_profile", "arguments": {"name": "ada"}}]},
        "Ada is made.",
    ]))
    .await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let chat = chat(hand);
    chief.open(&chat).await;
    chief.say(hand, "Make ada.").await;
    chief.shows(&chat, "Ada is made.").await;
    let ada = ProfileId(2);
    let character = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Character {
                id,
                profile: ada,
                text: "Careful with numbers.".into(),
            })
        })
        .await;
    let memory = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Memory {
                id,
                of: Keeper::Profile(ada),
                path: "memory/units.md".into(),
                text: "---\ndescription: The units\n---\nMetric.\n"
                    .into(),
            })
        })
        .await;
    let missing = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Character {
                id,
                profile: ProfileId(9),
                text: "Nobody.".into(),
            })
        })
        .await;
    let papers = chief.papers(Keeper::Profile(ada)).await;

    assert_eq!(
        (character.kind, character.text.as_str()),
        (ReplyKind::Notice, "The character is saved.")
    );
    assert_eq!(
        (memory.kind, memory.text.as_str()),
        (ReplyKind::Notice, "The memory is saved.")
    );
    assert_eq!(
        (missing.kind, missing.text.as_str()),
        (ReplyKind::Error, "no profile has the address `profile#9`")
    );
    assert_eq!(papers.character, "Careful with numbers.");
    assert_eq!(papers.memory.len(), 1, "{papers:?}");
    assert_eq!(
        (
            papers.memory[0].path.as_str(),
            papers.memory[0].description.as_str()
        ),
        ("units.md", "The units")
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_settings_are_saved_and_a_blank_character_is_refused() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let saved = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Settings {
                id,
                set: EditSettingsSet {
                    reflect_every: Some(7),
                    allow_net: Some(true),
                    ..NO_SETTING
                },
            })
        })
        .await;
    let blank = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Settings {
                id,
                set: EditSettingsSet {
                    character: Some("  ".into()),
                    ..NO_SETTING
                },
            })
        })
        .await;
    let negative = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Settings {
                id,
                set: EditSettingsSet {
                    reflect_every: Some(-1),
                    ..NO_SETTING
                },
            })
        })
        .await;

    assert_eq!(
        (saved.kind, saved.text.as_str()),
        (ReplyKind::Notice, "The settings are saved.")
    );
    assert_eq!(
        (blank.kind, blank.text.as_str()),
        (
            ReplyKind::Error,
            "A blank profile needs a character to start from."
        )
    );
    assert_eq!(
        (negative.kind, negative.text.as_str()),
        (ReplyKind::Error, "Reflect after zero or more turns.")
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hamlets_limit_and_the_realms_are_saved() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    found(&server, &mut chief, "Docs").await;
    let docs = look(&server).await.hamlets[0].id.clone();
    let hamlet = chief
        .limit(Some("Docs"), "asking", "questions", json!(5))
        .await;
    let negative =
        chief.limit(Some("Docs"), "road", "most", json!(-1)).await;
    let unfit = chief
        .limit(Some("Docs"), "asking", "questions", json!("three"))
        .await;
    let model = chief
        .limit(Some("Docs"), "", "model", json!("faux::faux-b"))
        .await;
    let realm =
        chief.limit(None, "", "roadsBetween", json!(false)).await;
    let missing = chief
        .limit(Some("nowhere"), "asking", "questions", json!(3))
        .await;

    assert_eq!(
        (hamlet.kind, hamlet.text),
        (
            ReplyKind::Notice,
            format!("The limits of `{docs}` are saved.")
        )
    );
    assert_eq!(
        (negative.kind, negative.text.as_str()),
        (ReplyKind::Error, "the most roads cannot be less than zero")
    );
    assert_eq!(
        (unfit.kind, unfit.text.as_str()),
        (ReplyKind::Error, "Those limits do not fit their form.")
    );
    assert_eq!(
        (realm.kind, realm.text.as_str()),
        (ReplyKind::Notice, "The realm's limits are saved.")
    );
    assert_eq!(
        (model.kind, model.text),
        (
            ReplyKind::Notice,
            format!("The limits of `{docs}` are saved.")
        )
    );
    assert_eq!(missing.kind, ReplyKind::Error);
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_check_and_a_move_with_nothing_to_move_are_answered()
 {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let checked = chief
        .request(|id| ClientMessage::Check(Check::Connection { id }))
        .await;
    let moved = chief
        .request(|id| ClientMessage::MoveLodges(MoveLodges { id }))
        .await;

    assert_eq!(
        (checked.kind, checked.text.as_str()),
        (ReplyKind::Notice, "Every agent runs the faux model here.")
    );
    assert_eq!(
        (moved.kind, moved.text.as_str()),
        (
            ReplyKind::Error,
            "Choose a remote workspace in the settings first."
        )
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// The edit that keeps `tea.md` in the memory of `agent`.
fn keeping_tea(id: RequestId, agent: AgentId) -> ClientMessage {
    ClientMessage::Edit(Edit::Memory {
        id,
        of: Keeper::Agent(agent),
        path: "tea.md".into(),
        text: TEA.into(),
    })
}

/// The settings' edit that chooses the remote workspace at `url`.
fn choosing(id: RequestId, url: String) -> ClientMessage {
    ClientMessage::Edit(Edit::Settings {
        id,
        set: EditSettingsSet {
            workspace: Some(Some(EditSettingsSetWorkspace {
                token: TOKEN.into(),
                url,
            })),
            ..NO_SETTING
        },
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_copies_each_agents_files_to_the_chosen_workspace() {
    let worker = Worker::start();
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let kept = chief.request(|id| keeping_tea(id, hand)).await;
    let chosen =
        chief.request(|id| choosing(id, worker.url.clone())).await;
    let moved = chief
        .request(|id| ClientMessage::MoveLodges(MoveLodges { id }))
        .await;
    let shown = chief.realm().settings.workspace.moving;

    assert_eq!(kept.kind, ReplyKind::Notice, "{}", kept.text);
    assert_eq!(chosen.kind, ReplyKind::Notice, "{}", chosen.text);
    assert_eq!(
        (moved.kind, moved.text.as_str()),
        (
            ReplyKind::Notice,
            "Every lodge is copied to the chosen workspace; agents' \
             tools still run on this machine."
        )
    );
    let tea = worker
        .files
        .join(format!("realm/lodges/{}/memory/tea.md", hand.0));
    assert_eq!(std::fs::read_to_string(tea).unwrap(), TEA);
    let requests = worker.requests();
    assert!(
        requests
            .iter()
            .all(|request| request["workspace"] == "realm"),
        "{requests:#?}"
    );
    let shown = shown.expect("the move is shown in the settings");
    assert_eq!(
        (shown.state, shown.done, shown.of, shown.text),
        (SettingsViewWorkspaceMoveState::Moved, 1, 1, moved.text)
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_to_a_workspace_that_does_not_answer_fails() {
    let (mut server, _home, _side) = serve(json!([])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    let kept = chief.request(|id| keeping_tea(id, hand)).await;
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let chosen = chief.request(|id| choosing(id, url)).await;
    let moved = chief
        .request(|id| ClientMessage::MoveLodges(MoveLodges { id }))
        .await;
    let shown = chief.realm().settings.workspace.moving;

    assert_eq!(kept.kind, ReplyKind::Notice, "{}", kept.text);
    assert_eq!(chosen.kind, ReplyKind::Notice, "{}", chosen.text);
    assert_eq!(moved.kind, ReplyKind::Error);
    assert!(
        moved.text.starts_with(
            "Moving the lodges failed: The workspace result is \
             unknown. Inspect its files before repeating the \
             operation. Request: "
        ),
        "{}",
        moved.text
    );
    let shown = shown.expect("the move is shown in the settings");
    assert_eq!(
        (shown.state, shown.done, shown.of, shown.text),
        (SettingsViewWorkspaceMoveState::Failed, 1, 1, moved.text)
    );
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
