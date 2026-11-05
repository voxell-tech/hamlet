//! Recordings of a chief's chats: with the Hand, and a hire in a
//! manual hamlet.

use hamlet_acceptance::{Faux, Home, call, calls, failure, words};
use serde_json::{Value, json};

use super::capture::{Capture, HAND};

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn hello() {
    Capture::start(Faux::new()).await.finish("hello").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn turn() {
    let greeting = "Hello chief. The Hand is here and ready to work \
                    with you. "
        .repeat(6)
        .trim()
        .to_owned();
    let faux = Faux::new().tokens_per_second(100.0).responses([
        call("bash", json!({"command":"echo hello"}))
            .thinking("I will greet the chief."),
        words(&greeting),
    ]);
    let mut c = Capture::start(faux).await;
    c.open(HAND).await;
    c.say(HAND, "Hello, Hand.").await;
    c.shows(HAND, &greeting).await;
    c.idle().await;
    let items = c.items(HAND);
    assert_eq!(c.open(HAND).await["items"], json!(items));
    assert!(c.has(|m| {
        m["ops"].as_array().is_some_and(|ops| {
            ops.iter().any(|o| o["op"] == "append")
        })
    }));
    c.finish("turn").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn tool() {
    let mut c = Capture::start(Faux::new().responses([
        calls([
            ("bash", json!({"command":"echo success"})),
            ("bash", json!({"command":"echo broken; exit 7"})),
        ]),
        words("One command succeeded and one failed."),
    ]))
    .await;
    c.open(HAND).await;
    c.say(HAND, "Run both commands.").await;
    c.shows(HAND, "One command succeeded and one failed.").await;
    for exit in [0, 7] {
        assert!(c.has(|m| m["ops"].as_array().is_some_and(|ops| {
            ops.iter()
                .any(|o| o["op"] == "close" && o["exit"] == exit)
        })));
    }
    c.finish("tool").await;
}

fn wait(c: &Capture, field: &str, value: &str) -> Option<Value> {
    serde_json::to_value(c.state()).unwrap()["waits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["detail"][field] == value)
        .cloned()
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn approval() {
    let home = Home::new();
    let folder = home.side().join("work");
    std::fs::create_dir(&folder).unwrap();
    let result = folder.join("result.txt");
    let command = format!(
        "printf 'heading\\napproved\\n' > {}",
        result.display()
    );
    let mut c = Capture::start_in(
        home,
        Faux::new().agent(
            2,
            [
                calls([
                    ("bash", json!({"command": command})),
                    (
                        "write",
                        json!({
                            "path": result,
                            "content": "heading\nreviewed\n",
                        }),
                    ),
                ]),
                words("The approved command finished."),
            ],
        ),
    )
    .await;
    c.say(HAND, "/found docs ada keep the docs").await;
    c.until(|c| c.state().agents.iter().any(|a| a.id.0 == 2))
        .await;
    c.open(2).await;
    c.say(2, &format!("/folder {}", folder.display())).await;
    c.send(json!({"type":"say","to":2,"text":"Run the command and write in manual mode."})).await;
    let mut preview = Value::Null;
    for danger in ["command", "edit"] {
        c.until(|c| wait(c, "danger", danger).is_some()).await;
        let held = wait(&c, "danger", danger).unwrap();
        if danger == "edit" {
            preview = held["change"].clone();
            assert_eq!(
                preview["lines"],
                json!([{"mark":" ","text":"heading"},{"mark":"-","text":"approved"},{"mark":"+","text":"reviewed"}])
            );
        }
        assert_eq!(c.ask(json!({"type":"answer","wait":held["id"],"choice":"allow"})).await["kind"], "notice");
    }
    c.shows(2, "The approved command finished.").await;
    assert!(c.has(|m| m["ops"].as_array().is_some_and(|ops| {
        ops.iter()
            .any(|o| o["op"] == "close" && o["change"] == preview)
    })));
    c.say(2, "/mode auto").await;
    c.finish("approval").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn fault() {
    let mut c = Capture::start(
        Faux::new().responses([failure("Faux model unavailable")]),
    )
    .await;
    c.open(HAND).await;
    c.send(json!({"type":"say","to":HAND,"text":"Try the model."}))
        .await;
    c.until(|c| wait(c, "kind", "fault").is_some()).await;
    let held = wait(&c, "kind", "fault").unwrap();
    c.idle().await;
    assert_eq!(c.ask(json!({"type":"answer","wait":held["id"],"choice":"dismiss"})).await["kind"], "notice");
    c.finish("fault").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn stop() {
    let mut c = Capture::start(Faux::new().responses([
        call(
            "bash",
            json!({"command":"echo started > started; sleep 30"}),
        ),
        words("This answer must never arrive."),
    ]))
    .await;
    c.open(HAND).await;
    c.send(json!({"type":"say","to":HAND,"text":"Run until I stop you."}))
        .await;
    let started = c.home.file("hand/started");
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_secs(20);
    while !started.exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20))
            .await;
    }
    assert_eq!(
        c.ask(json!({"type":"stop","agent":HAND})).await["kind"],
        "notice"
    );
    c.idle().await;
    assert!(
        !c.items(HAND)
            .iter()
            .any(|i| i.text == "This answer must never arrive.")
    );
    c.finish("stop").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn rewind() {
    let mut c = Capture::start(Faux::new().responses([
        words("First answer."),
        words("Second answer."),
    ]))
    .await;
    c.open(HAND).await;
    c.send(json!({"type":"say","to":HAND,"text":"Keep this line."}))
        .await;
    c.shows(HAND, "First answer.").await;
    c.idle().await;
    c.send(
        json!({"type":"say","to":HAND,"text":"Take back this line."}),
    )
    .await;
    c.shows(HAND, "Second answer.").await;
    c.idle().await;
    let target = c
        .items(HAND)
        .into_iter()
        .find(|i| i.text == "Take back this line.")
        .unwrap()
        .id;
    let reply = c.ask(json!({"type":"rewind","chat":{"with":HAND},"item":target})).await;
    assert_eq!(reply["kind"], "rewound");
    assert_eq!(reply["text"], "Take back this line.");
    c.until(|c| {
        c.items(HAND)
            .iter()
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>()
            == ["Keep this line.", "First answer."]
    })
    .await;
    c.finish("rewind").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn new_and_resume() {
    let mut c = Capture::start(Faux::new().responses([
        words("The first conversation."),
        words("A note for later."),
    ]))
    .await;
    c.open(HAND).await;
    c.say(HAND, "Remember our first conversation.").await;
    c.shows(HAND, "The first conversation.").await;
    c.idle().await;
    c.say(HAND, "/new").await;
    c.idle().await;
    c.open(HAND).await;
    let history = c
        .ask(json!({"type":"read","what":"history","agent":HAND}))
        .await;
    assert!(
        history["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["number"] == 1 && h["current"] == false)
    );
    c.say(HAND, "/resume 1").await;
    c.open(HAND).await;
    c.finish("new-and-resume").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn paging() {
    let mut c =
        Capture::start(Faux::new().responses(
            (1..=60).map(|i| words(&format!("Answer {i}."))),
        ))
        .await;
    for i in 1..=60 {
        c.say(HAND, &format!("Line {i}.")).await;
        c.idle().await;
    }
    let latest = c.open(HAND).await;
    assert_eq!(latest["items"].as_array().unwrap().len(), 100);
    assert_eq!(latest["earlier"], latest["items"][0]["id"]);
    let older = c.ask(json!({"type":"open","chat":{"with":HAND},"before":latest["earlier"]})).await;
    assert!(older["earlier"].is_null());
    assert_eq!(older["items"].as_array().unwrap().len(), 20);
    let texts = older["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(latest["items"].as_array().unwrap())
        .map(|i| i["text"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        (1..=60)
            .flat_map(|i| [
                format!("Line {i}."),
                format!("Answer {i}.")
            ])
            .collect::<Vec<_>>()
    );
    c.finish("paging").await;
}
