//! What waits for a chief: approvals at the gate, lasting grants,
//! questions and objections.

use std::fs;
use std::time::Duration;

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

fn bash(command: &str) -> hamlet_testing::Step {
    call("bash", json!({"command": command}))
}

#[tokio::test]
async fn the_network_waits_for_a_chief_and_always_becomes_a_grant() {
    let mut s = Session::start(Faux::new().responses([
        bash("echo curl one > one"),
        bash("echo curl two > two"),
        bash("echo curl three > three"),
        words("Done."),
        bash("echo curl four > four"),
        bash("echo curl five > five"),
        words("Done again."),
    ]))
    .await;
    let hand = s.hand();
    let lodge = s.home.file("hand");
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Try the network.").await;

    // Allowed once: it runs, and the next one asks again.
    let first = s
        .chief
        .wait_for("the first ask", |w| w.subject.contains("one"))
        .await;
    assert_eq!(first.agent, Some(hand));
    let WaitDetail::Approval { danger, always, .. } = &first.detail
    else {
        panic!("an approval: {first:?}");
    };
    assert_eq!(*danger, WaitDanger::Network);
    assert_eq!(*always, Some(Lasting::Grant));
    assert_eq!(
        first.answers,
        [Choice::Allow, Choice::Always, Choice::Deny]
    );
    assert!(!lodge.join("one").exists());
    s.chief.choose(&first.id, Choice::Allow, None).await;
    let second = s
        .chief
        .wait_for("the second ask", |w| w.subject.contains("two"))
        .await;
    assert!(lodge.join("one").exists());

    // Refused: the command does not run, and the rest of the turn's
    // network calls are refused without asking again.
    s.chief
        .choose(&second.id, Choice::Deny, Some("not now"))
        .await;
    s.chief.shows(&now, "Done.").await;
    assert!(
        !lodge.join("two").exists() && !lodge.join("three").exists()
    );
    assert!(s.chief.waits().is_empty());

    // Allowed for good: no later command asks.
    s.chief.tell(hand, "Once more.").await;
    let fourth = s
        .chief
        .wait_for("the fourth ask", |w| w.subject.contains("four"))
        .await;
    s.chief.choose(&fourth.id, Choice::Always, None).await;
    s.chief.shows(&now, "Done again.").await;
    assert!(
        lodge.join("four").exists() && lodge.join("five").exists()
    );
    assert!(s.chief.waits().is_empty());
    let grants = s.chief.realm().grants;
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].agent, Some(hand));
    assert_eq!(grants[0].permission, Permission::Network);
    assert_eq!(grants[0].source, GrantSource::Grant);
    assert_eq!(
        s.chief.command(hand, "/grants").await,
        "- hand may use network"
    );

    // Taking the grant back makes the agent ask again.
    assert_eq!(
        s.chief.command(hand, "/revoke hand network").await,
        "hand may no longer use network."
    );
    s.chief
        .until("grant gone", |c| c.realm().grants.is_empty())
        .await;
    assert!(s.join().await.realm().grants.is_empty());
    assert_eq!(
        s.chief.command(hand, "/grants").await,
        "No agent holds a lasting grant."
    );
}

#[tokio::test]
async fn the_settings_can_allow_the_network_to_every_agent() {
    let mut s =
        Session::start(Faux::new().responses([
            bash("echo curl open > open"),
            words("Done."),
        ]))
        .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Edit(Edit::Settings {
                id,
                set: EditSettingsSet {
                    allow_net: Some(true),
                    character: None,
                    effort: None,
                    newcomers: None,
                    endpoints: None,
                    model: None,
                    reflect_every: None,
                    workspace: None,
                },
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    s.chief.tell(hand, "Go.").await;
    s.chief.shows(&now, "Done.").await;
    assert!(s.chief.waits().is_empty());
    assert!(s.home.file("hand/open").exists());
}

#[tokio::test]
async fn a_manual_hamlet_asks_before_it_edits_or_runs_in_its_folder()
{
    let folder = hamlet_testing::Scratch::new("folder");
    let edited = folder.join("notes.txt");
    let ran_file = folder.join("ran");
    let home = Home::new();
    let lodge_file = home.file("hamlets/docs/0/lodge-file");
    let mut s = Session::start_in(
        home,
        Faux::new().agent(
            2,
            [
                call(
                    "write",
                    json!({"path": edited, "content": "hello\n"}),
                ),
                bash(&format!("echo ran > {}", ran_file.display())),
                call(
                    "write",
                    json!({"path": lodge_file, "content": "own\n"}),
                ),
                words("Done."),
            ],
        ),
        QUIET,
    )
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    assert_eq!(ada, AgentId(2));
    s.chief
        .command(ada, &format!("/folder {}", folder.display()))
        .await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Write the notes.").await;

    let edit = s.chief.wait_for("the edit", |_| true).await;
    let WaitDetail::Approval { danger, .. } = &edit.detail else {
        panic!("an approval");
    };
    assert_eq!(danger, &WaitDanger::Edit);
    let change = edit.change.clone().expect("the edit is shown");
    assert!(change.created && change.added == 1, "{change:?}");
    assert!(!edited.exists());
    s.chief.command(ada, "/allow").await;
    let command = s
        .chief
        .wait_for("the command", |w| {
            matches!(
                &w.detail,
                WaitDetail::Approval {
                    danger: WaitDanger::Command,
                    ..
                }
            )
        })
        .await;
    assert_eq!(fs::read_to_string(&edited).unwrap(), "hello\n");
    assert!(command.subject.contains("echo ran"));
    s.chief
        .command(ada, &format!("/allow #{}", command.id))
        .await;

    // The hire's own lodge is free.
    s.chief.shows(&now, "Done.").await;
    assert_eq!(fs::read_to_string(&ran_file).unwrap(), "ran\n");
    assert!(lodge_file.exists());
    assert!(s.chief.waits().is_empty());
}

#[tokio::test]
async fn an_auto_hamlet_works_in_its_folder_without_asking() {
    let folder = hamlet_testing::Scratch::new("folder");
    let mut s = Session::start(Faux::new().agent(
        2,
        [
            call(
                "write",
                json!({"path": folder.join("a.txt"), "content": "a"}),
            ),
            words("Done."),
        ],
    ))
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief
        .command(ada, &format!("/folder {}", folder.display()))
        .await;
    s.chief.command(ada, "/mode auto").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Write.").await;
    s.chief.shows(&now, "Done.").await;
    assert_eq!(
        fs::read_to_string(folder.join("a.txt")).unwrap(),
        "a"
    );
}

#[tokio::test]
async fn ask_chief_waits_for_picks_or_words_and_the_answer_reaches_the_agent()
 {
    let mut s = Session::start(Faux::new().responses([
        call(
            "ask_chief",
            json!({"message": "Before planting", "questions": [
                {"question": "Which grain?", "description": "A crop",
                 "options": [{"title": "Wheat", "description": "Winter"}, "Oats"],
                 "text_input": false},
                {"question": "How much?"}
            ]}),
        ),
        call("ask_chief", json!({"questions": [{"question": "Sure?"}]})),
        words("Planted."),
    ]))
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Plant something.").await;
    let asked = s
        .chief
        .wait_for("the questions", |w| {
            matches!(w.detail, WaitDetail::Question { .. })
        })
        .await;
    let WaitDetail::Question { questions } = &asked.detail else {
        panic!("questions");
    };
    assert_eq!(questions.message, "Before planting");
    assert_eq!(questions.questions.len(), 2);
    assert_eq!(questions.questions[0].options[1].title, "Oats");
    assert!(!questions.questions[0].text_input);
    assert_eq!(asked.answers, [Choice::Reply, Choice::Dismiss]);

    // An answer must pick for every question, from the offers.
    let wrong = [
        vec![Pick::Offer { index: 1 }],
        vec![
            Pick::Offer { index: 9 },
            Pick::Words { text: "two".into() },
        ],
        vec![
            Pick::Words {
                text: "Wheat".into(),
            },
            Pick::Words { text: "two".into() },
        ],
    ];
    for picks in wrong {
        let reply = s
            .chief
            .request(|id| {
                ClientMessage::Answer(Answer {
                    choice: Choice::Reply,
                    id,
                    picks: Some(picks),
                    text: None,
                    wait: asked.id.clone(),
                })
            })
            .await;
        assert_eq!(reply.kind, ReplyKind::Error, "{}", reply.text);
    }
    assert_eq!(s.chief.waits().len(), 1);
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Answer(Answer {
                choice: Choice::Reply,
                id,
                picks: Some(vec![
                    Pick::Offer { index: 1 },
                    Pick::Words {
                        text: "two acres".into(),
                    },
                ]),
                text: None,
                wait: asked.id.clone(),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);

    // The second question is answered in words by command.
    let sure = s
        .chief
        .wait_for("the second question", |w| w.id != asked.id)
        .await;
    s.chief
        .command(hand, &format!("/reply #{} yes, plant", sure.id))
        .await;
    s.chief.shows(&now, "Planted.").await;
    let answers = s
        .chief
        .items(&now)
        .into_iter()
        .filter(|i| i.kind == ItemKind::Answered)
        .count();
    assert!(answers >= 1);
    assert!(s.chief.waits().is_empty());
}

#[tokio::test]
async fn an_objection_holds_the_turn_until_the_chief_rules() {
    let mut s = Session::start(Faux::new().responses([
        calls([
            (
                "object",
                json!({"reason": "The file is better left alone."}),
            ),
            ("bash", json!({"command": "echo done > ruled"})),
        ]),
        words("Went ahead."),
    ]))
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Make the file.").await;
    let objection = s
        .chief
        .wait_for("the objection", |w| {
            matches!(w.detail, WaitDetail::Objection {})
        })
        .await;
    assert_eq!(objection.answers, [Choice::Go, Choice::Withdraw]);
    assert!(objection.about.contains("better left alone"));
    s.chief.settle(Duration::from_millis(500)).await;
    assert!(
        !s.home.file("hand/ruled").exists(),
        "the other call is held"
    );
    s.chief.command(hand, "/go carry on").await;
    s.chief.shows(&now, "Went ahead.").await;
    assert!(s.home.file("hand/ruled").exists());
}

#[tokio::test]
async fn withdrawing_an_objection_stops_the_turns_calls() {
    let mut s = Session::start(Faux::new().responses([calls([
        ("object", json!({"reason": "Leave the file."})),
        ("bash", json!({"command": "echo done > ruled"})),
    ])]))
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Make the file.").await;
    s.chief
        .wait_for("the objection", |w| {
            matches!(w.detail, WaitDetail::Objection {})
        })
        .await;
    s.chief.command(hand, "/withdraw").await;
    s.chief
        .shows(
            &now,
            "The chief withdrew the instruction: Make the file.",
        )
        .await;
    s.chief
        .until("the turn over", |c| !c.is_working(&now))
        .await;
    s.chief.settle(Duration::from_millis(500)).await;
    assert!(!s.home.file("hand/ruled").exists());
    assert!(s.chief.waits().is_empty(), "{:?}", s.chief.waits());
}
