//! Structured questions through a real server and its Pi process.

use hamlet_acceptance::*;
use hamlet_protocol::v0::*;
use serde_json::json;

#[tokio::test]
async fn structured_questions_survive_restart_and_only_valid_answers_settle()
 {
    let secret = "test-asking-private-token-872";
    let env =
        [("HAMLET_REFLECT_EVERY", "0"), ("HAMLET_API_KEY", secret)];
    let mut home = Home::new();
    let server = home.serve(&Script::new().responses([
        call("ask_chief", json!({"message": "Before planting", "questions": [
            {"question": "Which grain?", "description": secret,
             "options": [{"title": "Wheat", "description": "Winter crop"}, "Oats"], "text_input": false},
            {"question": "How much?"}
        ]})),
    ]), &env).await;
    let mut chief = Client::join(&server).await;
    let chat = chat(mayor());
    chief.open(&chat).await;
    chief.tell(mayor(), "Plant something.").await;
    let wait = chief
        .wait_for("structured questions", |w| {
            matches!(w.detail, WaitDetail::Question { .. })
        })
        .await;
    let Wait {
        about,
        detail:
            WaitDetail::Question {
                questions: Some(questions),
            },
        ..
    } = &wait
    else {
        panic!("typed questions")
    };
    assert_eq!(questions.questions.len(), 2);
    assert_eq!(questions.questions[0].options[1].title, "Oats");
    assert!(!questions.questions[0].text_input);
    assert_eq!(questions.questions[0].description, "[REDACTED]");
    assert!(about.contains("a) Wheat - Winter crop"));
    assert!(
        !serde_json::to_string(chief.messages())
            .unwrap()
            .contains(secret)
    );
    let again = Client::join(&server).await;
    assert_eq!(again.waits(), std::slice::from_ref(&wait));
    again.close().await;
    server.kill().await;
    let server = home
        .serve(
            &Script::new().responses([words("Oats, two acres.")]),
            &env,
        )
        .await;
    let mut chief = Client::join(&server).await;
    assert_eq!(chief.waits(), std::slice::from_ref(&wait));
    chief.open(&chat).await;
    for picks in [
        vec![Pick::Offer { index: 1 }],
        vec![
            Pick::Offer { index: 2 },
            Pick::Words {
                text: "two acres".into(),
            },
        ],
        vec![
            Pick::Words {
                text: "Wheat".into(),
            },
            Pick::Words {
                text: "two acres".into(),
            },
        ],
        vec![
            Pick::Offer { index: 1 },
            Pick::Words { text: "  ".into() },
        ],
    ] {
        let reply = chief
            .request(|id| {
                ClientMessage::Answer(Answer {
                    id,
                    wait: wait.id.clone(),
                    choice: Choice::Reply,
                    text: None,
                    picks: Some(picks),
                })
            })
            .await;
        assert_eq!(reply.kind, ReplyKind::Error, "{reply:?}");
        assert_eq!(chief.waits(), std::slice::from_ref(&wait));
    }
    let reply = chief
        .choose(&wait.id, Choice::Reply, Some("b and two acres"))
        .await;
    assert_eq!(reply.kind, ReplyKind::Error);
    let picks = vec![
        Pick::Offer { index: 1 },
        Pick::Words {
            text: format!("two acres {secret}"),
        },
    ];
    for (choice, text) in [
        (Choice::Dismiss, None),
        (Choice::Reply, Some("also text".into())),
    ] {
        let reply = chief
            .request(|id| {
                ClientMessage::Answer(Answer {
                    id,
                    wait: wait.id.clone(),
                    choice,
                    text,
                    picks: Some(picks.clone()),
                })
            })
            .await;
        assert_eq!(reply.kind, ReplyKind::Error);
        assert_eq!(chief.waits(), std::slice::from_ref(&wait));
    }
    let reply = chief
        .request(|id| {
            ClientMessage::Answer(Answer {
                id,
                wait: wait.id.clone(),
                choice: Choice::Reply,
                text: None,
                picks: Some(picks),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    chief.shows(&chat, "Oats, two acres.").await;
    let live = chief.items(&chat);
    let answers = live
        .iter()
        .filter(|i| i.kind == ItemKind::Answered)
        .collect::<Vec<_>>();
    assert_eq!(answers.len(), 1);
    assert_eq!(
        answers[0].text,
        "> Which grain?\nOats\n\n> How much?\ntwo acres [REDACTED]"
    );
    assert_eq!(chief.open(&chat).await.items, live);
    assert!(chief.waits().is_empty());
    assert!(
        !serde_json::to_string(chief.messages())
            .unwrap()
            .contains(secret)
    );
    assert_eq!(server.stop().await, Some(0));
    let mut stores = vec![home.file("realm.sqlite")];
    for runtime in std::fs::read_dir(home.file("harnesses")).unwrap()
    {
        let path = runtime.unwrap().path().join("pi.sqlite");
        if path.exists() {
            stores.push(path);
        }
    }
    assert!(stores.len() >= 2, "both realm and Pi stores scanned");
    for path in stores {
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes
                .windows(secret.len())
                .any(|b| b == secret.as_bytes()),
            "secret in {}",
            path.display()
        );
    }
}

#[tokio::test]
async fn question_limit_counts_calls_not_questions_and_invalid_calls_do_not_spend_it()
 {
    let mut s = Session::start(Script::new().responses([
        call("ask_chief", json!({"questions": [{"question": "Bad", "options": ["same", " same "]}]})),
        call("ask_chief", json!({"questions": [{"question": "First?"}, {"question": "Second?"}]})),
        call("ask_chief", json!({"question": "One too many?"})),
        words("Done asking."),
    ])).await;
    s.chief.limit("home", "asking", "questions", json!(1)).await;
    let chat = chat(mayor());
    s.chief.open(&chat).await;
    s.chief.tell(mayor(), "Plan.").await;
    let wait = s
        .chief
        .wait_for("the valid pair", |w| {
            matches!(w.detail, WaitDetail::Question { .. })
        })
        .await;
    let WaitDetail::Question {
        questions: Some(questions),
    } = &wait.detail
    else {
        panic!("questions")
    };
    assert_eq!(questions.questions[0].question, "First?");
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Answer(Answer {
                id,
                wait: wait.id.clone(),
                choice: Choice::Reply,
                text: None,
                picks: Some(vec![
                    Pick::Words { text: "one".into() },
                    Pick::Words { text: "two".into() },
                ]),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief.shows(&chat, "Done asking.").await;
    assert!(s.chief.waits().is_empty());
    assert_eq!(
        s.chief
            .items(&chat)
            .iter()
            .filter(|i| i.kind == ItemKind::Answered)
            .count(),
        1
    );
}
