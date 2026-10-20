use base64::prelude::*;
use hamlet_acceptance::{
    Home, Script, call, calls, failure, openai, words,
};
use serde_json::{Value, json};

use super::capture::{Capture, MAYOR};

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn hello() {
    Capture::start(Script::new()).await.finish("hello").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn turn() {
    let greeting = "Hello chief. The mayor is here and ready to \
                    work with you. "
        .repeat(6)
        .trim()
        .to_owned();
    let script = Script::new()
        .price(0.01)
        .tokens_per_second(100.0)
        .responses([
            call("bash", json!({"command":"echo hello"}))
                .thinking("I will greet the chief."),
            words(&greeting),
        ]);
    let mut c = Capture::start(script).await;
    c.open().await;
    c.say(MAYOR, "Hello, mayor.").await;
    c.shows(&greeting).await;
    c.idle().await;
    let items = c.items();
    assert_eq!(c.open().await["items"], json!(items));
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
    let mut c = Capture::start(Script::new().responses([
        calls([
            ("bash", json!({"command":"echo success"})),
            ("bash", json!({"command":"echo broken; exit 7"})),
        ]),
        words("One command succeeded and one failed."),
    ]))
    .await;
    c.open().await;
    c.say(MAYOR, "Run both commands.").await;
    c.shows("One command succeeded and one failed.").await;
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
    let mut c = Capture::start(Script::new().responses([
        calls([
            ("bash", json!({"command":"printf 'heading\\napproved\\n' > result.txt"})),
            ("write", json!({"path":"result.txt","content":"heading\nreviewed\n"})),
        ]),
        words("The approved command finished."),
    ])).await;
    c.open().await;
    let folder = c.home.side().join("work");
    std::fs::create_dir(&folder).unwrap();
    c.say(MAYOR, &format!("/folder {}", folder.display())).await;
    c.send(json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"Run the command and write in manual mode."})).await;
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
    c.shows("The approved command finished.").await;
    assert!(c.has(|m| m["ops"].as_array().is_some_and(|ops| {
        ops.iter()
            .any(|o| o["op"] == "close" && o["change"] == preview)
    })));
    c.say(MAYOR, "/mode auto").await;
    c.finish("approval").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn fault() {
    let mut c = Capture::start(
        Script::new().responses([failure("Faux model unavailable")]),
    )
    .await;
    c.open().await;
    c.send(json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"Try the model."})).await;
    c.until(|c| wait(c, "kind", "fault").is_some()).await;
    let held = wait(&c, "kind", "fault").unwrap();
    c.idle().await;
    assert!(c.state().agents[0].faulted);
    assert_eq!(c.ask(json!({"type":"answer","wait":held["id"],"choice":"dismiss"})).await["kind"], "notice");
    c.finish("fault").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn stop() {
    let mut c = Capture::start(Script::new().responses([
        call(
            "bash",
            json!({"command":"echo started > NOTES.md; sleep 30"}),
        ),
        words("This answer must never arrive."),
    ]))
    .await;
    c.open().await;
    c.send(json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"Run until I stop you."})).await;
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_secs(20);
    loop {
        let papers = c.ask(json!({"type":"read","what":"papers","agent":{"hamlet":"home","agent":"mayor"}})).await;
        if papers["papers"]["notes"] == "started" {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20))
            .await;
    }
    assert_eq!(c.ask(json!({"type":"stop","agent":{"hamlet":"home","agent":"mayor"}})).await["kind"], "notice");
    c.until(|c| {
        c.items().iter().any(|i| i.text.starts_with("Stopped:"))
    })
    .await;
    c.idle().await;
    assert!(
        !c.items()
            .iter()
            .any(|i| i.text == "This answer must never arrive.")
    );
    c.finish("stop").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn rewind() {
    let mut c = Capture::start(Script::new().responses([
        words("First answer."),
        words("Second answer."),
    ]))
    .await;
    c.open().await;
    c.say(MAYOR, "Keep this line.").await;
    c.shows("First answer.").await;
    c.say(MAYOR, "Take back this line.").await;
    c.shows("Second answer.").await;
    c.idle().await;
    let target = c
        .items()
        .into_iter()
        .find(|i| i.text == "Take back this line.")
        .unwrap()
        .id;
    let reply = c.ask(json!({"type":"rewind","chat":{"with":{"hamlet":"home","agent":"mayor"}},"item":target})).await;
    assert_eq!(reply["kind"], "rewound");
    assert_eq!(reply["text"], "Take back this line.");
    c.until(|c| {
        c.items()
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
    let mut c = Capture::start(
        Script::new().responses([words("The first conversation.")]),
    )
    .await;
    c.open().await;
    c.say(MAYOR, "Remember our first conversation.").await;
    c.shows("The first conversation.").await;
    c.idle().await;
    c.say(MAYOR, "/new").await;
    let history = c.ask(json!({"type":"read","what":"history","agent":{"hamlet":"home","agent":"mayor"}})).await;
    assert!(history["history"].as_array().unwrap().iter().any(|h| h
        ["number"]
        == 1
        && h["current"] == false
        && h["messages"] == 2));
    c.say(MAYOR, "/resume 1").await;
    c.shows("Went back to an earlier session.").await;
    c.finish("new-and-resume").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn paging() {
    let mut c =
        Capture::start(Script::new().responses(
            (1..=23).map(|i| words(&format!("Answer {i}."))),
        ))
        .await;
    for i in 1..=23 {
        c.say(MAYOR, &format!("Line {i}.")).await;
    }
    let latest = c.open().await;
    assert_eq!(latest["items"].as_array().unwrap().len(), 40);
    assert_eq!(latest["earlier"], latest["items"][0]["id"]);
    let older = c.ask(json!({"type":"open","chat":{"with":{"hamlet":"home","agent":"mayor"}},"before":latest["earlier"]})).await;
    assert!(older["earlier"].is_null());
    assert_eq!(older["items"].as_array().unwrap().len(), 6);
    let texts = older["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(latest["items"].as_array().unwrap())
        .map(|i| i["text"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        (1..=23)
            .flat_map(|i| [
                format!("Line {i}."),
                format!("Answer {i}.")
            ])
            .collect::<Vec<_>>()
    );
    c.finish("paging").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn reads_and_edits() {
    let mut c = Capture::start(Script::new()).await;
    for what in ["prompt", "papers"] {
        assert!(!c.ask(json!({"type":"read","what":what,"agent":{"hamlet":"home","agent":"mayor"}})).await[what].is_null());
    }
    for what in ["charter", "models"] {
        assert!(
            !c.ask(json!({"type":"read","what":what})).await[what]
                .is_null()
        );
    }
    assert_eq!(c.ask(json!({"type":"edit","doc":"limits","hamlet":"home","set":{"section":"budget","key":"cost","value":2}})).await["kind"], "notice");
    let mut limits =
        serde_json::to_value(&c.state().hamlets[0].limits).unwrap();
    limits["mostAgents"] = 4.into();
    limits["budget"]["cost"] = 3.into();
    assert_eq!(c.ask(json!({"type":"edit","doc":"limits","hamlet":"home","whole":limits})).await["kind"], "notice");
    assert_eq!(c.ask(json!({"type":"edit","doc":"limits","hamlet":"home","set":{"section":"budget","key":"cost","value":-1}})).await["kind"], "error");
    assert_eq!(c.ask(json!({"type":"edit","doc":"settings","set":{"reflectEvery":0,"allowNet":true}})).await["kind"], "notice");
    assert_eq!(c.ask(json!({"type":"edit","doc":"charter","text":"Serve the chief with care."})).await["kind"], "notice");
    assert_eq!(
        c.ask(json!({"type":"read","what":"charter"})).await
            ["charter"],
        json!({"own":true,"text":"Serve the chief with care."})
    );
    c.finish("reads-and-edits").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn login_key() {
    let endpoint = openai::Endpoint::start().await;
    let mut home = Home::new();
    let server = home
        .serve_providers(&[
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_MODEL", "local::org/large"),
            ("HAMLET_BASE_URL", &endpoint.url),
        ])
        .await;
    let mut c = Capture::join(home, server).await;
    let prompt = c.ask(json!({"type":"login","start":{"provider":"openai","method":"key"}})).await;
    assert_eq!(
        prompt["step"],
        json!({"prompt":"Enter OpenAI API key","secret":true})
    );
    assert_eq!(c.ask(json!({"type":"login","reply":{"login":prompt["login"],"value":"sk-fake-frames-not-a-real-key"}})).await["kind"], "notice");
    c.until(|c| {
        c.has(|m| {
            m["type"] == "login" && m["step"]["done"] == "openai"
        })
    })
    .await;
    c.until(|c| {
        c.has(|m| {
            m["changes"].as_array().is_some_and(|updates| {
                updates.iter().any(|u| {
                    u["providers"].as_array().is_some_and(|ps| {
                        ps.iter().any(|p| {
                            p["id"] == "openai"
                                && p["configured"] == true
                        })
                    })
                })
            })
        })
    })
    .await;
    c.finish("login-key").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn budget_stop() {
    let mut c =
        Capture::start(Script::new().price(1_000_000.0).responses([
            call("read", json!({"path":"NOTES.md"})),
            words("This answer must never arrive."),
        ]))
        .await;
    c.open().await;
    assert_eq!(c.ask(json!({"type":"edit","doc":"limits","hamlet":"home","set":{"section":"budget","key":"cost","value":0.01}})).await["kind"], "notice");
    c.say(MAYOR, "Read your notes until the budget stops you.")
        .await;
    c.until(|c| {
        c.items().iter().any(|i| {
            i.text.contains("budget")
                && i.text.contains("$0.01")
                && i.text.contains("chief")
        })
    })
    .await;
    c.idle().await;
    assert!(
        !c.items()
            .iter()
            .any(|i| i.text == "This answer must never arrive.")
    );
    c.finish("budget-stop").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn give() {
    let mut c = Capture::start(
        Script::new().responses([words("A folder received.")]),
    )
    .await;
    let begun = c.ask(json!({"type":"give","to":{"hamlet":"home","agent":"mayor"},"files":[{"path":"folder/deep/a.md","size":1},{"path":"folder/b.md","size":1}]})).await;
    assert_eq!(begun["kind"], "notice");
    for (path, data) in
        [("folder/deep/a.md", b"A"), ("folder/b.md", b"B")]
    {
        let reply = c.ask(json!({"type":"give-part","upload":begun["upload"],"path":path,"offset":0,"data":BASE64_STANDARD.encode(data)})).await;
        assert_eq!(reply["kind"], "notice");
    }
    assert_eq!(
        std::fs::read(
            c.home.file("houses/home/0/inbox/folder/deep/a.md")
        )
        .unwrap(),
        b"A"
    );
    c.open().await;
    c.shows("A folder received.").await;
    c.idle().await;
    assert_eq!(c.ask(json!({"type":"give","to":{"hamlet":"home","agent":"mayor"},"skill":{"starter":"web-search"}})).await["kind"], "notice");
    c.until(|c| {
        c.state().agents[0]
            .skills
            .as_ref()
            .is_some_and(|s| s.iter().any(|s| s == "web-search"))
    })
    .await;
    assert_eq!(c.ask(json!({"type":"give","to":{"hamlet":"home","agent":"mayor"},"skill":{"starter":"nothing"}})).await["kind"], "error");
    c.open().await;
    c.finish("give").await;
}
