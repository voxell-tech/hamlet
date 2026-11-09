//! What a server keeps across a stop and a crash: the realm, the
//! chats, and the waits with the calls held on them.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

fn names(state: &State) -> Vec<(String, AgentId, bool)> {
    state
        .agents
        .iter()
        .map(|a| (a.name.clone(), a.id, a.left.is_some()))
        .collect()
}

#[tokio::test]
async fn a_wait_and_its_held_call_survive_a_crash() {
    let mut home = Home::new();
    let held = Faux::new().agent(
        2,
        [
            call("bash", json!({"command": "echo curl x >> out"})),
            words("Finished."),
        ],
    );
    let mut server = home.serve(&held, QUIET).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.command(hand, "/found docs keep the docs").await;
    chief
        .until("the mayor", |c| c.agent("docs#0").is_some())
        .await;
    let ada = chief.id("docs#0");
    let now = chat(ada);
    chief.open(&now).await;
    chief.tell(ada, "Fetch it.").await;
    let wait = chief.wait_for("the approval", |_| true).await;
    let before = chief.realm();
    server.kill();

    let server = home
        .serve(&Faux::new().agent(2, [words("Finished.")]), QUIET)
        .await;
    let mut chief = Client::join(&server).await;
    let after = chief.realm();
    assert_eq!(names(&after), names(&before));
    assert_eq!(after.waits, std::slice::from_ref(&wait));
    chief.open(&now).await;
    assert!(chief.texts(&now).contains(&"Fetch it.".to_string()));
    assert!(!home.file("hamlets/docs/0/out").exists());
    chief.choose(&wait.id, Choice::Allow, None).await;
    chief.shows(&now, "Finished.").await;
    // The command ran once, after the answer.
    assert_eq!(
        std::fs::read_to_string(home.file("hamlets/docs/0/out"))
            .unwrap(),
        "curl x\n"
    );
    assert!(chief.waits().is_empty());
}

#[tokio::test]
async fn the_realm_comes_back_as_it_was_after_a_stop() {
    let mut s = Session::start(Faux::new().responses([
        words("Hello."),
        call(
            "found_hamlet",
            json!({"hamlet": "bog", "scholar": "scholar#2", "purpose": "drain"}),
        ),
        words("Founded."),
    ]))
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Hi.").await;
    s.chief.shows(&now, "Hello.").await;
    s.chief.command(hand, "/scholar new bo: A digger.").await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.hire(ada, "cy", "dig").await;
    s.chief.command(ada, "/tier 100").await;
    s.chief.command(ada, "/end docs#1 done").await;
    s.chief
        .limit(Some("docs"), "task", "messages", json!(5))
        .await;
    s.chief.tell(hand, "Found one.").await;
    s.chief.shows(&now, "Founded.").await;
    s.chief.idle("hand").await;
    let before = s.chief.realm();
    let items = s.chief.items(&now);

    assert_eq!(s.server.stop().await, Some(0));
    let server = s.home.serve(&Faux::new(), QUIET).await;
    let mut chief = Client::join(&server).await;
    let after = chief.realm();
    assert_eq!(names(&after), names(&before));
    assert_eq!(after.scholars, before.scholars);
    assert_eq!(after.hand, before.hand);
    assert_eq!(after.limits, before.limits);
    assert_eq!(
        after
            .hamlets
            .iter()
            .map(|h| (&h.id, h.tier, h.mayor))
            .collect::<Vec<_>>(),
        before
            .hamlets
            .iter()
            .map(|h| (&h.id, h.tier, h.mayor))
            .collect::<Vec<_>>()
    );
    assert_eq!(chief.hamlet("docs").unwrap().limits.task.messages, 5);
    let page = chief.open(&now).await;
    assert_eq!(page.items, items);
    // Ids go on from where they were: nothing is made twice.
    let newest =
        before.scholars.iter().map(|s| s.id.0).max().unwrap();
    chief.command(chief.hand(), "/scholar new dee").await;
    chief.until("dee", |c| c.scholar("dee").is_some()).await;
    assert!(chief.scholar("dee").unwrap().id.0 > newest);
    assert_eq!(chief.history(hand).await.len(), 1);
}
