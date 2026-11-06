//! The `hamlet_server` executable stopped at its fault points, killed
//! there, and started again on the same realm: what it kept is
//! carried on and nothing runs twice. Every server here starts pi, so
//! the target needs Node and the `harness` feature.

#![cfg(unix)]

mod remote;

use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_acceptance::{Client, Faux, call, chat, words};
use hamlet_protocol::client::ServerMessage;
use hamlet_testing::{Models, Scratch, Server};
use serde_json::json;

/// The `hamlet_server` these tests are built with.
const SERVER: &str = env!("CARGO_BIN_EXE_hamlet_server");

/// A realm on a scratch home, and a folder beside it for its faux
/// scripts and the markers of its fault points.
struct Realm {
    home: Scratch,
    side: Scratch,
    started: u32,
    env: Vec<(String, String)>,
}

impl Realm {
    /// A new realm, whose agents do not reflect between turns.
    fn new() -> Self {
        Self {
            home: Scratch::new("crash-home"),
            side: Scratch::new("crash-side"),
            started: 0,
            env: vec![("HAMLET_REFLECT_EVERY".into(), "0".into())],
        }
    }

    /// A new realm whose servers stop at `point` once.
    fn faulting(point: &str) -> Self {
        let realm = Self::new().with("HAMLET_FAULT", point);
        let side = realm.side.display().to_string();
        realm.with("HAMLET_FAULT_DIR", &side)
    }

    /// The same, with the environment variable `name` set too.
    fn with(mut self, name: &str, value: &str) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }

    /// Starts a server on the realm answering with `faux`, and
    /// connects the chief.
    async fn serve(&mut self, faux: Faux) -> (Server, Client) {
        self.started += 1;
        let file =
            self.side.join(format!("faux-{}.json", self.started));
        faux.write(&file);
        let env = self
            .env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        let server = Server::start(
            &[SERVER.to_owned()],
            &self.home,
            Models::Faux(&file),
            &env,
        )
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
        let chief = Client::join(&server).await;
        (server, chief)
    }

    /// Waits until the server stopped at its fault point.
    async fn paused(&self) {
        let marker = self.side.join("server-paused");
        for _ in 0..300 {
            if marker.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the server never stopped at its fault point");
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.join(relative)
    }
}

/// The lines of `file`.
fn lines(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

#[tokio::test]
async fn a_request_whose_handler_panics_is_faulted_and_the_next_answered()
 {
    let mut realm = Realm::faulting("request");
    let (mut server, mut chief) = realm.serve(Faux::new()).await;
    let hand = chief.hand();
    let first = chief.tell(hand, "/budget").await;
    match chief.answer(&first).await {
        ServerMessage::Fault(fault) => assert!(
            fault.text.contains("the handler panicked"),
            "{}",
            fault.text
        ),
        other => panic!("a panic got {other:?}, not a fault"),
    }
    assert!(realm.side.join("server-panicked").exists());
    chief.command(hand, "/budget").await;
    assert!(server.is_running());
}

#[tokio::test]
async fn a_job_cut_off_by_a_crash_runs_at_the_next_start() {
    let mut realm = Realm::faulting("jobs");
    let (mut server, mut chief) = realm.serve(Faux::new()).await;
    let hand = chief.hand();
    chief
        .tell(hand, "/scholar new sam: A careful reader.")
        .await;
    realm.paused().await;
    server.kill();
    // The scholar is kept and its repository is a job still to run:
    // only the Hand's own scholar has one.
    let scholars = realm.path("scholars");
    let planted = || std::fs::read_dir(&scholars).unwrap().count();
    assert_eq!(planted(), 1);

    // The next start runs it: sam's main gets its first commit.
    let (_server, mut chief) = realm.serve(Faux::new()).await;
    let mut versions = String::new();
    for _ in 0..100 {
        versions = chief.command(hand, "/versions scholar#2").await;
        if versions.contains("Latest commits of main") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("sam's main has no commit: {versions}");
}

#[tokio::test]
async fn a_turn_ended_before_a_crash_is_followed_at_the_next_start() {
    let mut realm = Realm::faulting("followed");
    let (mut server, mut chief) =
        realm.serve(Faux::new().responses([words("Three.")])).await;
    let hand = chief.hand();
    chief.command(hand, "/worker Count to three.").await;
    realm.paused().await;
    let worker = chief
        .realm()
        .agents
        .into_iter()
        .find(|agent| agent.name.starts_with("worker"))
        .expect("the worker is in the realm");
    assert!(worker.left.is_none());
    server.kill();

    // Its answer is kept; following it ends the worker.
    let (_server, mut chief) = realm.serve(Faux::new()).await;
    chief
        .until("the worker to end", |c| {
            c.agent(&worker.name).is_some_and(|a| a.left.is_some())
        })
        .await;
}

#[tokio::test]
async fn a_command_cut_off_after_it_ran_is_not_run_again() {
    let mut realm = Realm::faulting("ran");
    let command = call(
        "bash",
        json!({"command": "echo ran >> ran.txt", "description": "Count."}),
    );
    let (mut server, mut chief) =
        realm.serve(Faux::new().responses([command])).await;
    let hand = chief.hand();
    let hand_chat = chat(hand);
    chief.open(&hand_chat).await;
    chief.tell(hand, "Count once.").await;
    realm.paused().await;
    server.kill();
    let ran = realm.path("hand/ran.txt");
    assert_eq!(lines(&ran), ["ran"]);

    let (_server, mut chief) = realm
        .serve(Faux::new().responses([words("Counted.")]))
        .await;
    chief.open(&hand_chat).await;
    chief.shows(&hand_chat, "Counted.").await;
    let failure = chief
        .items(&hand_chat)
        .into_iter()
        .find_map(|item| item.tool?.failure)
        .expect("the call's failure is kept");
    assert!(failure.contains("interrupted"), "{failure}");
    assert_eq!(lines(&ran), ["ran"]);
}

/// The Hand makes the scholar `sam`, and the server is killed after
/// the change and before the call is answered; how many scholars
/// named `sam` there are once the Hand asked again, and how the call
/// failed, if it did.
async fn scholar_made_across_a_crash(
    mut realm: Realm,
) -> (usize, Option<String>) {
    let make = call(
        "create_scholar",
        json!({"name": "sam", "character": "A careful reader."}),
    );
    let (mut server, mut chief) =
        realm.serve(Faux::new().responses([make])).await;
    let hand = chief.hand();
    let hand_chat = chat(hand);
    chief.open(&hand_chat).await;
    chief.tell(hand, "Make sam.").await;
    realm.paused().await;
    server.kill();

    let (_server, mut chief) =
        realm.serve(Faux::new().responses([words("Made.")])).await;
    chief.open(&hand_chat).await;
    chief.shows(&hand_chat, "Made.").await;
    let made = chief
        .realm()
        .scholars
        .iter()
        .filter(|s| s.name == "sam")
        .count();
    let failure = chief
        .items(&hand_chat)
        .into_iter()
        .find_map(|item| item.tool)
        .expect("the call's result is kept")
        .failure;
    (made, failure)
}

#[tokio::test]
async fn a_society_call_cut_off_after_its_change_is_answered_the_same()
 {
    let (made, failure) =
        scholar_made_across_a_crash(Realm::faulting("carried")).await;
    assert_eq!((made, failure), (1, None));
}

#[tokio::test]
async fn control_a_change_and_its_answer_kept_apart_lose_the_answer()
{
    // The crash test above must catch a change kept apart from its
    // call's answer: asked again, the call makes a second scholar,
    // as a name no longer refuses it.
    let realm = Realm::faulting("carried")
        .with("HAMLET_FAULT_SPLIT_MARKER", "1");
    let (made, _) = scholar_made_across_a_crash(realm).await;
    assert_eq!(made, 2);
}
