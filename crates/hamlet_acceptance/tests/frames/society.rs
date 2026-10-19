use hamlet_acceptance::{Script, call, words};
use serde_json::json;

use super::capture::{Capture, MAYOR};

fn script() -> Script {
    Script::new().agent("home:0", [
        call("create_agent", json!({"name":"scout","purpose":"Scout","reason":"Explore"})),
        call("create_agent", json!({"name":"smith","purpose":"Smith","reason":"Build","roads":["scout"]})),
        call("create_agent", json!({"name":"poet","purpose":"Poet","reason":"Write"})),
        words("The society is ready"),
    ])
}

async fn build(script: Script) -> Capture {
    let mut c = Capture::start(script).await;
    c.open().await;
    c.say(MAYOR, "Build our society").await;
    c.until(|c| c.state().agents.len() == 4).await;
    c.idle().await;
    c
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn society() {
    let mut c = build(script().agent("home:0", [
        call("tell_agent", json!({"name":"smith","message":"Make a hinge"})),
        words("Sent"), words("Letter received"),
        call("ask_agent", json!({"name":"scout","message":"Where is the iron?"})),
        words("Iron found"),
    ]).agent("home:2", [words("Hinge ready")]).agent("home:1", [words("In the northern hill")])).await;
    c.say(MAYOR, "Tell smith").await;
    c.shows("Letter received").await;
    c.idle().await;
    assert!(
        c.items()
            .iter()
            .any(|i| i.author.as_deref() == Some("smith@home")
                && i.text.contains("Hinge ready"))
    );
    let letters = c.ask(json!({"type":"open","chat":{"with":{"hamlet":"home","agent":"smith"},"from":{"hamlet":"home","number":0},"kind":"letters"}})).await;
    assert!(
        letters["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["text"] == "Hinge ready")
    );
    c.say(MAYOR, "Ask scout").await;
    c.shows("Iron found").await;
    let questions = c.ask(json!({"type":"open","chat":{"with":{"hamlet":"home","agent":"scout"},"from":{"hamlet":"home","number":0},"kind":"questions"}})).await;
    assert!(
        questions["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["text"] == "In the northern hill")
    );
    c.open().await;
    assert!(
        c.state()
            .hamlets
            .iter()
            .flat_map(|h| h.roads.iter().flatten())
            .any(|r| r.messages == 2)
    );
    c.finish("society").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn leaving() {
    let mut c =
        build(script().errands([words("# Notes\nFind and forge.")]))
            .await;
    c.say(MAYOR, "/remove scout Finished").await;
    c.until(|c| {
        c.state()
            .agents
            .iter()
            .any(|a| a.name == "scout" && a.left.is_some())
    })
    .await;
    c.say(MAYOR, "/restore scout").await;
    c.say(MAYOR, "/merge scout into smith Together").await;
    c.until(|c| {
        c.has(|m| {
            m["changes"].as_array().is_some_and(|updates| {
                updates.iter().any(|u| {
                    u["agent"]["name"] == "scout"
                        && u["agent"]["left"]["how"] == "merged"
                })
            })
        })
    })
    .await;
    c.open().await;
    c.finish("leaving").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn vote() {
    let yes = || {
        call(
            "cast_ballot",
            json!({"choice":"yes","reason":"A new direction","evidence":""}),
        )
    };
    let mut c = build(script().errands([
        words("Let them decide"),
        yes(),
        yes(),
        yes(),
    ]))
    .await;
    c.say(MAYOR, "/vote smith New direction").await;
    c.until(|c| {
        c.state().hamlets[0]
            .votes
            .iter()
            .flatten()
            .any(|v| !v.open && v.report.contains("Passed:"))
    })
    .await;
    assert!(
        c.state()
            .agents
            .iter()
            .any(|a| a.name == "smith" && a.mayor)
    );
    c.open().await;
    c.finish("vote").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn realm() {
    let mut c = build(script()
        .agent("mill:0", [
            call("create_agent", json!({"name":"mason","purpose":"Stonework","reason":"Build"})), words("Mason ready"),
            words("Considering"),
            call("agree_road", json!({"a":"mason","b":"scout","reason":"Trade"})), words("Road agreed"),
        ])
        .agent("home:0", [call("agree_road", json!({"a":"scout","b":"mason","reason":"Trade"})), words("Road proposed")])).await;
    c.say(MAYOR, "/found mill miller Grind flour").await;
    c.say(MAYOR, "/move-hamlet mill 60 80").await;
    assert_eq!(c.ask(json!({"type":"say","to":{"hamlet":"mill","agent":"mayor"},"text":"Make a mason"})).await["kind"], "notice");
    c.say(MAYOR, "Propose a road").await;
    c.idle().await;
    assert_eq!(c.ask(json!({"type":"say","to":{"hamlet":"mill","agent":"mayor"},"text":"Agree the road"})).await["kind"], "notice");
    c.until(|c| {
        c.state()
            .hamlets
            .iter()
            .flat_map(|h| h.roads.iter().flatten())
            .any(|r| {
                r.a.hamlet != r.b.hamlet
                    && (r.a.number == 1 || r.b.number == 1)
            })
    })
    .await;
    c.say(MAYOR, "/remove-hamlet mill Finished").await;
    c.until(|c| {
        c.state().closed.iter().flatten().any(|h| h.id == "mill")
    })
    .await;
    c.say(MAYOR, "/erase-hamlet mill").await;
    c.until(|c| c.state().closed.as_ref().is_some_and(Vec::is_empty))
        .await;
    c.finish("realm").await;
}

#[tokio::test]
#[ignore = "records real sessions; see the acceptance README"]
async fn library() {
    let mut c = build(script()
        .agent("home:1", [call("submit_skill", json!({"skill":"map-iron","reason":"Useful maps"})), words("Shared")])
        .errands([words("ACCEPT Useful maps")])).await;
    let skill = c.home.file("houses/home/1/skills/map-iron");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: map-iron\ndescription: Maps iron \
         deposits.\n---\nRead the map.\n",
    )
    .unwrap();
    c.say("scout", "Share the map").await;
    c.until(|c| {
        c.state()
            .library
            .iter()
            .flatten()
            .any(|s| s.name == "map-iron")
    })
    .await;
    assert!(c.has(|m| m["changes"].as_array().is_some_and(
        |updates| {
            updates.iter().any(|u| {
                u["agent"]["name"] == "scout"
                    && u["agent"]["consulting"] == true
            })
        }
    )));
    c.say(MAYOR, "/unshelve map-iron Retire").await;
    c.until(|c| {
        !c.state()
            .library
            .iter()
            .flatten()
            .any(|s| s.name == "map-iron")
    })
    .await;
    c.finish("library").await;
}
