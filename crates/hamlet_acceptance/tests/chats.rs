//! A chief's conversations with the Hand: lines and answers, numbered
//! sessions, failed and stopped turns, and reflection.

use std::time::Duration;

use hamlet_acceptance::*;
use hamlet_protocol::client::*;

fn kinds(client: &Client, chat: &ChatRef) -> Vec<(ItemKind, String)> {
    client
        .items(chat)
        .into_iter()
        .map(|item| (item.kind, item.text))
        .collect()
}

fn said(kind: ItemKind, text: &str) -> (ItemKind, String) {
    (kind, text.into())
}

#[tokio::test]
async fn the_hand_answers_and_a_second_connection_sees_the_chat() {
    let mut s = Session::start(
        Faux::new().responses([words("Hello, chief.")]),
    )
    .await;
    let hand = s.hand();
    let hand_chat = chat(hand);
    assert!(s.chief.open(&hand_chat).await.items.is_empty());
    s.chief.tell(hand, "Hi").await;
    s.chief.shows(&hand_chat, "Hello, chief.").await;
    s.chief
        .until("the turn over", |c| !c.is_working(&hand_chat))
        .await;
    let items = s.chief.items(&hand_chat);
    assert_eq!(items[0].kind, ItemKind::User);
    assert_eq!(items[0].chief.as_deref(), Some("nixon"));
    assert_eq!(items[1].kind, ItemKind::Assistant);
    s.chief.idle("hand").await;
    assert!(s.chief.agent("hand").unwrap().spent.tokens > 0);

    let mut other = s.join().await;
    let page = other.open(&hand_chat).await;
    assert_eq!(
        page.items
            .iter()
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>(),
        ["Hi", "Hello, chief."]
    );
    assert!(!page.working);
}

#[tokio::test]
async fn sessions_are_numbered_and_come_back() {
    // `/new` has the Hand reflect on the session it leaves.
    let mut s = Session::start(Faux::new().responses([
        words("One."),
        words("Kept it."),
        words("Two."),
    ]))
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "a").await;
    s.chief.shows(&now, "One.").await;
    assert_eq!(
        s.chief.command(hand, "/new").await,
        "Session 2 began."
    );
    let first = ChatRef {
        past: Some(1),
        ..chat(hand)
    };
    s.chief.shows(&now, "Kept it.").await;
    // The watch of the current session starts again after /new.
    let began = s.chief.open(&now).await;
    assert_eq!(began.number, Some(2));
    s.chief.tell(hand, "b").await;
    s.chief.shows(&now, "Two.").await;

    let history = s.chief.history(hand).await;
    let numbers = history
        .iter()
        .map(|p| (p.number, p.current, p.title.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(numbers, [(2, true, "b"), (1, false, "a")]);
    let page = s.chief.open(&first).await;
    assert_eq!(page.items[0].text, "a");

    let listed = s.chief.command(hand, "/resume").await;
    assert_eq!(listed, "- session 1\n- session 2");
    assert_eq!(
        s.chief.command(hand, "/resume 1").await,
        "Back in session 1."
    );
    s.chief.refused(hand, "/resume 9").await;
}

#[tokio::test]
async fn a_failed_turn_waits_for_a_chief_and_retry_carries_on() {
    let mut s = Session::start(Faux::new().responses([
        failure("the model fell over"),
        words("Recovered."),
        words(""),
    ]))
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Go").await;
    let fault = s
        .chief
        .wait_for("the fault", |w| {
            matches!(w.detail, WaitDetail::Fault {})
        })
        .await;
    assert_eq!(fault.about, "the model fell over");
    assert_eq!(fault.agent, Some(hand));
    assert_eq!(fault.answers, [Choice::Dismiss]);
    s.chief
        .until("the error shown", |c| {
            kinds(c, &now).contains(&said(
                ItemKind::Error,
                "the model fell over",
            ))
        })
        .await;

    assert_eq!(s.chief.command(hand, "/retry").await, "Carrying on.");
    s.chief.shows(&now, "Recovered.").await;

    // An empty final answer fails its turn as well.
    s.chief.tell(hand, "And again").await;
    s.chief
        .wait_for("the empty answer", |w| {
            w.about.contains("answered with nothing")
        })
        .await;

    // Both faults stay until dismissed.
    assert_eq!(s.chief.waits().len(), 2);
    s.chief.command(hand, "/dismiss #1").await;
    s.chief.command(hand, "/dismiss").await;
    s.chief.until("no waits", |c| c.waits().is_empty()).await;
}

#[tokio::test]
async fn stop_ends_a_running_turn() {
    let mut s =
        Session::start(Faux::new().responses([
            words("Slow.").stalled(3000),
            words("Quick."),
        ]))
        .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Take your time").await;
    s.chief.until("work", |c| c.is_working(&now)).await;
    assert_eq!(
        s.chief.command(hand, "/stop").await,
        "Stopped 1 running turns."
    );
    s.chief.until("rest", |c| !c.is_working(&now)).await;
    assert!(!s.chief.texts(&now).contains(&"Slow.".into()));
    assert_eq!(
        s.chief.command(hand, "/stop all").await,
        "Stopped 0 running turns."
    );
}

#[tokio::test]
async fn the_hand_reflects_every_few_answered_turns() {
    let mut s = Session::start_with(
        Faux::new().responses([
            words("A."),
            words("B."),
            words("Remembered."),
            words("C."),
        ]),
        &[("HAMLET_REFLECT_EVERY", "2")],
    )
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "one").await;
    s.chief.shows(&now, "A.").await;
    s.chief.tell(hand, "two").await;
    s.chief.shows(&now, "Remembered.").await;
    assert!(s.chief.texts(&now).contains(&"reflection".into()));
    // The count starts again after a reflection.
    s.chief.tell(hand, "three").await;
    s.chief.shows(&now, "C.").await;
    s.chief.settle(Duration::from_millis(300)).await;
    let reflections = s
        .chief
        .texts(&now)
        .iter()
        .filter(|t| *t == "reflection")
        .count();
    assert_eq!(reflections, 1);
}

#[tokio::test]
async fn rewind_takes_the_chat_back_to_before_a_line() {
    let mut s = Session::start(Faux::new().agent(
        1,
        [words("First."), words("Second."), words("Third.")],
    ))
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "One").await;
    s.chief.shows(&now, "First.").await;
    s.chief.tell(hand, "Two").await;
    s.chief.shows(&now, "Second.").await;
    s.chief.idle("hand").await;
    let items = s.chief.items(&now);
    let line = |text: &str| {
        items
            .iter()
            .find(|i| i.kind == ItemKind::User && i.text == text)
            .unwrap()
            .clone()
    };
    let two = line("Two");
    assert_eq!(two.rewindable, Some(true));

    // Only a chief's own line can be taken back.
    let answer = items
        .iter()
        .find(|i| i.kind == ItemKind::Assistant)
        .unwrap();
    let reply = s.chief.rewind(&now, &answer.id).await;
    assert_eq!(reply.kind, ReplyKind::Error, "{}", reply.text);
    let reply = s.chief.rewind(&now, "nowhere").await;
    assert_eq!(reply.kind, ReplyKind::Error, "{}", reply.text);

    let reply = s.chief.rewind(&now, &two.id).await;
    assert_eq!(
        (reply.kind, reply.text.as_str()),
        (ReplyKind::Rewound, "Two")
    );
    s.chief
        .until("the chat without its second line", |c| {
            let texts = c.texts(&now);
            texts.contains(&"First.".into())
                && !texts.contains(&"Two".into())
                && !texts.contains(&"Second.".into())
        })
        .await;

    // The line sent again carries on from there.
    s.chief.tell(hand, "Two, again").await;
    s.chief.shows(&now, "Third.").await;
    let texts = s.chief.texts(&now);
    assert_eq!(
        texts,
        ["One", "First.", "Two, again", "Third."],
        "{texts:?}"
    );
}
