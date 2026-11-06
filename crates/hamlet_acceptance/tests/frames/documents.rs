//! Recordings of reads, edits, sign-in, spend and gifts.

use base64::prelude::*;
use hamlet_acceptance::{Faux, Home, call, words};
use hamlet_testing::openai::{self, Endpoint, Response};
use serde_json::json;

use super::capture::{Capture, HAND};

/// The id of the scholar named `name`, once the state has it.
fn scholar(c: &Capture, name: &str) -> u64 {
    c.state()
        .scholars
        .iter()
        .find(|s| s.name == name)
        .unwrap()
        .id
        .0
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn reads_and_edits() {
    let mut c = Capture::start(Faux::new()).await;
    c.say(HAND, "/found docs ada keep the docs").await;
    c.until(|c| c.state().scholars.iter().any(|s| s.name == "ada"))
        .await;
    let ada = scholar(&c, "ada");
    for what in ["prompt", "history"] {
        assert!(
            !c.ask(json!({"type":"read","what":what,"agent":HAND}))
                .await[what]
                .is_null()
        );
    }
    assert!(!c.ask(json!({"type":"read","what":"papers","of":{"scholar":ada}})).await["papers"].is_null());
    for what in ["charter", "models"] {
        assert!(
            !c.ask(json!({"type":"read","what":what})).await[what]
                .is_null()
        );
    }
    assert_eq!(c.ask(json!({"type":"edit","doc":"limits","hamlet":"docs","set":{"section":"budget","key":"cost","value":2}})).await["kind"], "notice");
    c.until(|c| c.state().hamlets[0].limits.budget.cost == Some(2.0))
        .await;
    let mut limits =
        serde_json::to_value(&c.state().hamlets[0].limits).unwrap();
    limits["depth"] = 2.into();
    limits["budget"]["cost"] = 3.into();
    assert_eq!(c.ask(json!({"type":"edit","doc":"limits","hamlet":"docs","whole":limits})).await["kind"], "notice");
    assert_eq!(c.ask(json!({"type":"edit","doc":"limits","hamlet":"docs","set":{"section":"budget","key":"cost","value":-1}})).await["kind"], "error");
    assert_eq!(c.ask(json!({"type":"edit","doc":"settings","set":{"reflectEvery":0,"allowNet":true}})).await["kind"], "notice");
    assert_eq!(c.ask(json!({"type":"edit","doc":"charter","text":"Serve the chief with care."})).await["kind"], "notice");
    assert_eq!(
        c.ask(json!({"type":"read","what":"charter"})).await
            ["charter"],
        json!({"own":true,"text":"Serve the chief with care."})
    );
    assert_eq!(c.ask(json!({"type":"edit","doc":"character","scholar":ada,"text":"Calm and exact."})).await["kind"], "notice");
    assert_eq!(c.ask(json!({"type":"edit","doc":"memory","of":{"scholar":ada},"path":"tea.md","text":"---\ndescription: Tea\n---\nEarl grey.\n"})).await["kind"], "notice");
    let papers = c.ask(json!({"type":"read","what":"papers","of":{"scholar":ada}})).await;
    assert_eq!(papers["papers"]["character"], "Calm and exact.");
    c.finish("reads-and-edits").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn login_key() {
    let endpoint = Endpoint::start().await;
    let mut home = Home::new();
    let server = home
        .serve_providers(&[
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_MODEL", "local::org/large"),
            ("HAMLET_BASE_URL", &endpoint.url),
        ])
        .await;
    let mut c = Capture::join(home, server).await;
    c.until(|c| c.state().providers.iter().any(|p| p.id == "openai"))
        .await;
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
    let endpoint = Endpoint::start().await;
    endpoint.enqueue([
        Response {
            text: "First.".into(),
            ..Response::default()
        },
        Response {
            text: "This answer must never arrive.".into(),
            ..Response::default()
        },
    ]);
    let mut home = Home::new();
    let server = home
        .serve_providers(&[
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_MODEL", "local::org/large"),
            ("HAMLET_BASE_URL", &endpoint.url),
        ])
        .await;
    let mut c = Capture::join(home, server).await;
    c.open(HAND).await;
    let cap = openai::COST * 0.7;
    assert_eq!(c.ask(json!({"type":"edit","doc":"limits","set":{"section":"budget","key":"cost","value":cap}})).await["kind"], "notice");
    c.say(HAND, "One.").await;
    c.shows(HAND, "First.").await;
    c.idle().await;
    c.say(HAND, "Two.").await;
    c.until(|c| {
        c.items(HAND).iter().any(|i| {
            i.text.contains("budget")
                && i.text.contains("/reset-budget")
        })
    })
    .await;
    c.idle().await;
    assert!(
        !c.items(HAND)
            .iter()
            .any(|i| i.text == "This answer must never arrive.")
    );
    c.finish("budget-stop").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn give() {
    let mut c = Capture::start(Faux::new().responses([
        words("A folder received."),
        call(
            "bash",
            json!({"command":"mkdir -p skills/greet && printf -- '---\\nname: greet\\ndescription: Says hello\\n---\\nRun it.\\n' > skills/greet/SKILL.md"}),
        ),
        words("Written."),
    ]))
    .await;
    let begun = c.ask(json!({"type":"give","to":HAND,"files":[{"path":"folder/deep/a.md","size":1},{"path":"folder/b.md","size":1}]})).await;
    assert_eq!(begun["kind"], "notice");
    for (path, data) in
        [("folder/deep/a.md", b"A"), ("folder/b.md", b"B")]
    {
        let reply = c.ask(json!({"type":"give-part","upload":begun["upload"],"path":path,"offset":0,"data":BASE64_STANDARD.encode(data)})).await;
        assert_eq!(reply["kind"], "notice");
    }
    assert_eq!(
        std::fs::read(c.home.file("hand/inbox/folder/deep/a.md"))
            .unwrap(),
        b"A"
    );
    c.open(HAND).await;
    c.shows(HAND, "A folder received.").await;
    c.idle().await;
    c.say(HAND, "Write a skill.").await;
    c.shows(HAND, "Written.").await;
    c.idle().await;
    c.say(HAND, "/found docs ada keep the docs").await;
    c.until(|c| c.state().scholars.iter().any(|s| s.name == "ada"))
        .await;
    let ada = scholar(&c, "ada");
    assert_eq!(c.ask(json!({"type":"give","to":ada,"skill":{"from":{"agent":HAND},"name":"greet"}})).await["kind"], "notice");
    c.until(|c| {
        c.state().agents.iter().any(|a| {
            a.name == "ada@docs"
                && a.skills.iter().any(|s| s == "greet")
        })
    })
    .await;
    assert_eq!(c.ask(json!({"type":"give","to":ada,"skill":{"from":{"agent":HAND},"name":"nothing"}})).await["kind"], "error");
    c.open(HAND).await;
    c.finish("give").await;
}
