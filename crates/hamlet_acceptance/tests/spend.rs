//! Spend caps: a model call is not made once a budget, a task's cap
//! or an allowance is spent. The models are a loopback endpoint that
//! bills each answer, since the faux model is free.

use std::time::Duration;

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::openai::{self, Endpoint, Response};
use serde_json::json;

fn words_of(text: &str) -> Response {
    Response {
        text: text.into(),
        ..Response::default()
    }
}

fn bash(n: u32) -> Response {
    let arguments =
        json!({"command": format!("echo {n}")}).to_string();
    Response {
        text: String::new(),
        finish: "tool_calls".into(),
        calls: vec![json!({
            "index": 0, "id": format!("call_{n}"), "type": "function",
            "function": {"name": "bash", "arguments": arguments}
        })],
        ..Response::default()
    }
}

/// The last notice of a chat, once it is there.
async fn last_notice(
    client: &mut Client,
    chat: &ChatRef,
    containing: &str,
) -> String {
    client
        .until(&format!("a notice about {containing}"), |c| {
            c.items(chat).iter().any(|i| {
                i.kind == ItemKind::Notice
                    && i.text.contains(containing)
            })
        })
        .await;
    client
        .items(chat)
        .into_iter()
        .rev()
        .find(|i| i.kind == ItemKind::Notice)
        .unwrap()
        .text
}

#[tokio::test]
async fn the_realms_budget_stops_the_hand_until_it_starts_again() {
    let mut endpoint = Endpoint::start().await;
    endpoint
        .enqueue([words_of("First."), words_of("After the reset.")]);
    let mut s = Session::with_endpoint(&endpoint).await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    let cap = openai::COST * 0.7;
    s.chief.limit(None, "budget", "cost", json!(cap)).await;
    s.chief.tell(hand, "One.").await;
    s.chief.shows(&now, "First.").await;
    s.chief.idle("hand").await;
    assert!(s.chief.agent("hand").unwrap().spent.micros > 0);

    // The budget is spent, so the next line makes no model call.
    s.chief.tell(hand, "Two.").await;
    let notice = last_notice(&mut s.chief, &now, "budget").await;
    assert!(notice.contains("/reset-budget"), "{notice}");
    s.chief
        .wait_for("the aborted turn", |w| w.agent == Some(hand))
        .await;
    assert_eq!(endpoint.completions(1).await.len(), 1);

    s.chief.command(hand, "/reset-budget").await;
    s.chief.tell(hand, "Three.").await;
    s.chief.shows(&now, "After the reset.").await;
}

#[tokio::test]
async fn a_tasks_cap_stops_a_runaway_chain_of_calls() {
    let mut endpoint = Endpoint::start().await;
    endpoint.enqueue([bash(1), bash(2), bash(3), bash(4)]);
    let mut s = Session::with_endpoint(&endpoint).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief
        .limit(
            Some("docs"),
            "task",
            "cost",
            json!(openai::COST * 1.5),
        )
        .await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Keep going until stopped.").await;
    let notice = last_notice(&mut s.chief, &now, "task").await;
    assert!(notice.contains("may spend at most"), "{notice}");
    s.chief.settle(Duration::from_millis(500)).await;
    // Two calls were made: the second took the task past its cap.
    assert_eq!(endpoint.completions(2).await.len(), 2);
}

#[tokio::test]
async fn a_hires_allowance_stops_its_calls_and_a_lead_may_set_it() {
    let mut endpoint = Endpoint::start().await;
    endpoint.enqueue([words_of("Noted.")]);
    let mut s = Session::with_endpoint(&endpoint).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let cy = s.hire(ada, "cy", "dig").await;
    let reply = s.chief.command(ada, "/allowance docs#1 0.001").await;
    assert_eq!(reply, "`docs#1` may spend <$0.01 a period.");
    s.chief
        .until("its allowance shown", |c| {
            c.agent("docs#1")
                .is_some_and(|cy| cy.allowance == Some(0.001))
        })
        .await;
    let now = chat(cy);
    s.chief.open(&now).await;
    s.chief.tell(cy, "Say something.").await;
    s.chief.shows(&now, "Noted.").await;
    s.chief.tell(cy, "Say more.").await;
    let notice = last_notice(&mut s.chief, &now, "allowance").await;
    assert!(
        notice.contains("Its lead or a chief can raise it"),
        "{notice}"
    );
    assert_eq!(endpoint.completions(1).await.len(), 1);
    s.chief.command(ada, "/allowance docs#1 none").await;
}
