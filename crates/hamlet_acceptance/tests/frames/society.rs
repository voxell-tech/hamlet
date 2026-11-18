//! Recordings of the society: agents, notifications and requests,
//! channels, leaving and merging, hamlets, the library, and what
//! waits for a chief.

use hamlet_acceptance::{Faux, call, calls, words};
use hamlet_protocol::client::ChannelKind;
use serde_json::json;

use super::capture::{Capture, HAND};

/// A realm with the hamlet `docs` under `ada` (2), and its agents
/// `cy` (3, `agent#3`) and `dee` (4, `agent#4`), once all are there
/// and at rest.
async fn docs(faux: Faux) -> Capture {
    let mut c = Capture::start(faux).await;
    c.found("docs", "ada", "keep the docs").await;
    c.until(|c| c.state().agents.iter().any(|a| a.id.0 == 2))
        .await;
    c.add_agent(2, "cy", "dig").await;
    c.add_agent(2, "dee", "haul").await;
    c.until(|c| c.state().agents.len() == 4).await;
    c.idle().await;
    c
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn society() {
    let mut c = docs(
        Faux::new()
            .agent(
                2,
                [
                    call(
                        "notify",
                        json!({"to":"agent#3","text":"Hello cy"}),
                    ),
                    words("Told."),
                    call(
                        "request",
                        json!({"to":"agent#3","text":"Which colour?"}),
                    ),
                    words("Asked cy."),
                    words("cy said blue."),
                ],
            )
            .agent(3, [words("Noted."), words("Blue.")]),
    )
    .await;
    c.open(2).await;
    c.open(3).await;
    c.say(2, "Tell cy hello.").await;
    c.shows(2, "Told.").await;
    c.shows(3, "Noted.").await;
    c.say(2, "Ask cy a question.").await;
    c.shows(2, "Asked cy.").await;
    c.shows(3, "Blue.").await;
    c.shows(2, "cy said blue.").await;
    c.finish("society").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn leaving() {
    let mut c = docs(Faux::new()).await;
    c.say(2, "/end agent#3 Finished").await;
    c.until(|c| {
        c.state()
            .agents
            .iter()
            .any(|a| a.address == "agent#3" && a.left.is_some())
    })
    .await;
    c.say(2, "/restore agent#3").await;
    c.say(2, "/merge agent#3 into agent#4 Together").await;
    c.until(|c| {
        c.has(|m| {
            m["changes"].as_array().is_some_and(|updates| {
                updates.iter().any(|u| {
                    u["agent"]["name"] == "agent#3"
                        && u["agent"]["left"]["how"] == "merged"
                })
            })
        })
    })
    .await;
    c.say(2, "/restore agent#3").await;
    c.open(2).await;
    c.finish("leaving").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn realm() {
    let mut c = docs(
        Faux::new()
            .agent(
                2,
                [
                    call(
                        "agree_channel",
                        json!({"mine":"agent#3","theirs":"agent#6","reason":"Trade"}),
                    ),
                    words("Channel proposed."),
                ],
            )
            .agent(
                5,
                [
                    call(
                        "agree_channel",
                        json!({"mine":"agent#6","theirs":"agent#3","reason":"Trade"}),
                    ),
                    words("Channel agreed."),
                ],
            ),
    )
    .await;
    c.found("mill", "bo", "grind flour").await;
    c.until(|c| c.state().agents.iter().any(|a| a.id.0 == 5))
        .await;
    c.add_agent(5, "dee", "haul").await;
    c.say(
        HAND,
        &format!("/move-hamlet {} 60 80", c.hamlet_id("mill")),
    )
    .await;
    c.say(2, "Propose a channel.").await;
    c.idle().await;
    c.say(5, "Agree the channel.").await;
    c.until(|c| {
        c.state()
            .hamlets
            .iter()
            .flat_map(|h| h.channels.iter())
            .any(|r| r.kind == ChannelKind::Agreed)
    })
    .await;
    c.say(
        HAND,
        &format!("/remove-hamlet {} Finished", c.hamlet_id("mill")),
    )
    .await;
    c.until(|c| c.state().closed.iter().any(|h| h.name == "mill"))
        .await;
    let mill = c.state().closed[0].id.clone();
    c.say(HAND, &format!("/erase-hamlet {mill}")).await;
    c.until(|c| c.state().closed.is_empty()).await;
    c.finish("realm").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn library() {
    let mut c = Capture::start(
        Faux::new()
            .responses([
                call(
                    "bash",
                    json!({"command":"mkdir -p skills/map-iron && printf -- '---\\nname: map-iron\\ndescription: Maps iron deposits.\\n---\\nRead the map.\\n' > skills/map-iron/SKILL.md"}),
                ),
                call(
                    "submit_skill",
                    json!({"skill":"map-iron","reason":"Useful maps"}),
                ),
                words("Shared."),
            ])
            .kind("errand", [words("ACCEPT Useful maps")]),
    )
    .await;
    c.open(HAND).await;
    c.say(HAND, "Share the map.").await;
    c.until(|c| {
        c.state().library.iter().any(|s| s.name == "map-iron")
    })
    .await;
    c.shows(HAND, "Shared.").await;
    c.say(HAND, "/unshelve map-iron Retire").await;
    c.until(|c| {
        !c.state().library.iter().any(|s| s.name == "map-iron")
    })
    .await;
    c.finish("library").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn question() {
    let mut c = Capture::start(Faux::new().responses([
        call(
            "request_chief",
            json!({"message":"Before planting","questions":[
                {"question":"Which grain?","options":[
                    {"title":"Wheat","description":"Winter"},"Oats"]}
            ]}),
        ),
        words("Planted."),
    ]))
    .await;
    c.open(HAND).await;
    c.send(json!({"type":"say","to":HAND,"text":"Plant something."}))
        .await;
    c.until(|c| !c.state().waits.is_empty()).await;
    let wait = serde_json::to_value(&c.state().waits[0]).unwrap();
    assert_eq!(wait["detail"]["kind"], "question");
    assert_eq!(c.ask(json!({"type":"answer","wait":wait["id"],"choice":"reply","picks":[{"kind":"offer","index":1}]})).await["kind"], "notice");
    c.shows(HAND, "Planted.").await;
    c.finish("question").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn objection() {
    let mut c = Capture::start(Faux::new().responses([
        calls([
            ("object", json!({"reason":"Leave the file."})),
            ("bash", json!({"command":"echo done > ruled"})),
        ]),
        words("Went ahead."),
    ]))
    .await;
    c.open(HAND).await;
    c.send(json!({"type":"say","to":HAND,"text":"Make the file."}))
        .await;
    c.until(|c| !c.state().waits.is_empty()).await;
    let wait = serde_json::to_value(&c.state().waits[0]).unwrap();
    assert_eq!(wait["detail"]["kind"], "objection");
    assert_eq!(
        c.ask(
            json!({"type":"answer","wait":wait["id"],"choice":"go"})
        )
        .await["kind"],
        "notice"
    );
    c.shows(HAND, "Went ahead.").await;
    c.finish("objection").await;
}
