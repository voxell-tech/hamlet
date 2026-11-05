//! Recordings of the society: hires, letters and questions, roads,
//! leaving and merging, hamlets, the library, and what waits for a
//! chief.

use hamlet_acceptance::{Faux, call, calls, words};
use hamlet_protocol::client::RoadKind;
use serde_json::json;

use super::capture::{Capture, HAND};

/// A realm with the hamlet `docs` under `ada` (2), and its hires
/// `cy` (3) and `dee` (4), once all are there and at rest.
async fn docs(faux: Faux) -> Capture {
    let mut c = Capture::start(faux).await;
    c.say(HAND, "/found docs ada keep the docs").await;
    c.until(|c| c.state().agents.iter().any(|a| a.id.0 == 2))
        .await;
    c.say(2, "/hire cy dig").await;
    c.say(2, "/hire dee haul").await;
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
                        "tell",
                        json!({"to":"cy","text":"Hello cy"}),
                    ),
                    words("Told."),
                    call(
                        "ask",
                        json!({"to":"cy","question":"Which colour?"}),
                    ),
                    words("cy said blue."),
                ],
            )
            .agent(3, [words("Noted."), words("Blue.")]),
    )
    .await;
    c.open(2).await;
    c.say(2, "Tell cy hello.").await;
    c.shows(2, "Told.").await;
    c.until(|c| {
        c.state().agents.iter().any(|a| {
            a.name == "cy@docs"
                && !a.busy
                && !a.conversations.is_empty()
        })
    })
    .await;
    let letters = c.ask(json!({"type":"open","chat":{"with":3,"from":2,"kind":"letters"}})).await;
    assert!(
        letters["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["text"] == "Noted.")
    );
    c.say(2, "Ask cy a question.").await;
    c.shows(2, "cy said blue.").await;
    let questions = c.ask(json!({"type":"open","chat":{"with":3,"from":2,"kind":"questions"}})).await;
    assert!(
        questions["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["text"] == "Blue.")
    );
    c.open(2).await;
    c.finish("society").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn leaving() {
    let mut c = docs(Faux::new()).await;
    c.say(2, "/end cy Finished").await;
    c.until(|c| {
        c.state()
            .agents
            .iter()
            .any(|a| a.name == "cy@docs" && a.left.is_some())
    })
    .await;
    c.say(2, "/restore cy").await;
    c.say(2, "/merge cy into dee Together").await;
    c.until(|c| {
        c.has(|m| {
            m["changes"].as_array().is_some_and(|updates| {
                updates.iter().any(|u| {
                    u["agent"]["name"] == "cy@docs"
                        && u["agent"]["left"]["how"] == "merged"
                })
            })
        })
    })
    .await;
    c.say(2, "/restore cy").await;
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
                        "agree_road",
                        json!({"mine":"cy","theirs":"dee@mill","reason":"Trade"}),
                    ),
                    words("Road proposed."),
                ],
            )
            .agent(
                5,
                [
                    call(
                        "agree_road",
                        json!({"mine":"dee","theirs":"cy@docs","reason":"Trade"}),
                    ),
                    words("Road agreed."),
                ],
            ),
    )
    .await;
    c.say(HAND, "/found mill bo grind flour").await;
    c.until(|c| c.state().agents.iter().any(|a| a.id.0 == 5))
        .await;
    c.say(5, "/hire dee haul").await;
    c.say(HAND, "/move-hamlet mill 60 80").await;
    c.say(2, "Propose a road.").await;
    c.idle().await;
    c.say(5, "Agree the road.").await;
    c.until(|c| {
        c.state()
            .hamlets
            .iter()
            .flat_map(|h| h.roads.iter())
            .any(|r| r.kind == RoadKind::Agreed)
    })
    .await;
    c.say(HAND, "/remove-hamlet mill Finished").await;
    c.until(|c| c.state().closed.iter().any(|h| h.id == "mill"))
        .await;
    c.say(HAND, "/erase-hamlet mill").await;
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
            "ask_chief",
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
