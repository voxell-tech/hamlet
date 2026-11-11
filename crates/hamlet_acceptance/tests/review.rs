//! A lead's review of a chief's line to a hire under it, and a
//! chief's workers.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::words;
use serde_json::json;

#[tokio::test]
async fn a_lead_reviews_a_chiefs_line_to_its_hire_and_may_object() {
    let mut s = Session::start(
        Faux::new()
            .agent(
                3,
                [
                    words("Plain done."),
                    words("Odd done.").stalled(3000),
                ],
            )
            .kind(
                "errand",
                [
                    words("PASS"),
                    words("OBJECT: the odd line breaks the plan."),
                ],
            ),
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let bob = s.hire(ada, "bob", "write tests").await;
    assert_eq!(bob.0, 3);
    let reply = s
        .chief
        .limit(Some("docs"), "asking", "review", json!("always"))
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let now = chat(bob);
    s.chief.open(&now).await;

    // A line the lead passes is simply done.
    s.chief.tell(bob, "Plain line.").await;
    s.chief.shows(&now, "Plain done.").await;
    assert!(s.chief.waits().is_empty());

    // A line it objects to waits for the chief, while the work it
    // reviewed is still going on.
    s.chief.tell(bob, "Odd line.").await;
    let objection = s
        .chief
        .wait_for("the lead's objection", |w| {
            matches!(w.detail, WaitDetail::Objection {})
        })
        .await;
    assert_eq!(objection.agent, Some(bob));
    assert!(
        objection.about.contains(
            "hire#2 (ada) objects: the odd line breaks the plan."
        ),
        "{}",
        objection.about
    );
    assert_eq!(objection.answers, [Choice::Go, Choice::Withdraw]);
    s.chief.command(bob, "/go carry on").await;
    s.chief.shows(&now, "Odd done.").await;
}

#[tokio::test]
async fn a_lead_does_not_review_unless_the_limits_say_so() {
    let mut s = Session::start(
        Faux::new()
            .agent(3, [words("Done.")])
            .kind("errand", [words("OBJECT: never asked.")]),
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let bob = s.hire(ada, "bob", "write tests").await;
    let now = chat(bob);
    s.chief.open(&now).await;
    s.chief.tell(bob, "Any line.").await;
    s.chief.shows(&now, "Done.").await;
    s.chief.settle(std::time::Duration::from_millis(500)).await;
    assert!(s.chief.waits().is_empty());
}

#[tokio::test]
async fn a_chief_starts_a_worker_and_reads_its_answer() {
    let mut s = Session::start(
        Faux::new().kind("worker", [words("Counted: 3 files.")]),
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief.refused(ada, "/worker").await;
    let said = s.chief.command(ada, "/worker count the files").await;
    assert!(said.starts_with("Started worker#3;"), "{said}");
    let worker = s.chief.id("worker#3");
    let now = chat(worker);
    s.chief.open(&now).await;
    s.chief.shows(&now, "Counted: 3 files.").await;
    let texts = s.chief.texts(&now);
    assert!(
        texts.iter().any(|t| t.contains("count the files")),
        "{texts:?}"
    );
    s.chief
        .until("the worker has answered", |c| {
            c.agent("worker#3").is_some_and(|w| {
                w.left
                    .as_ref()
                    .is_some_and(|l| l.how == LeftHow::Answered)
            })
        })
        .await;
    let worker = s.chief.agent("worker#3").unwrap();
    assert_eq!(worker.kind, AgentKind::Worker);
    assert_eq!(worker.parent, None);
}
