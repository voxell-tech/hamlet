//! Chief documents and record views through the server's protocol
//! seam.

use hamlet_protocol::v0::{ReplyKind, ServerMessage};
use hamlet_server::protocol::Protocol;
use hamlet_server::record::{
    Actor, Admit, Change, Departure, RecordOperation,
};
use hamlet_server::service::Service;
use hamlet_server::{
    Home, OperationId, Outcome, Reply, RuntimeInstance,
};
use hamlet_testing::Scratch;
use serde_json::{Value, json};

fn request(service: &mut Service, mut value: Value) -> Value {
    value["id"] = uuid::Uuid::new_v4().to_string().into();
    let message =
        Protocol::new().unwrap().parse(&value.to_string()).unwrap();
    serde_json::to_value(service.handle(message).unwrap().unwrap())
        .unwrap()
}

fn open(dir: &Scratch) -> Service {
    Service::new(Home::open(dir).unwrap(), true).unwrap()
}

fn mayor() -> Value {
    json!({"hamlet":"home","agent":"mayor"})
}

#[test]
fn papers_keep_unicode_and_survive_restart_without_credentials() {
    let dir = Scratch::new("protocol-papers");
    let mut service = open(&dir);
    let credentials =
        std::fs::read(dir.join("credentials.json")).unwrap();
    let token = serde_json::from_slice::<Value>(&credentials)
        .unwrap()["connection"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let text = format!(" \u{feff} {}tail  ", "🦀".repeat(6001));
    for doc in ["character", "notes"] {
        assert_eq!(
            request(
                &mut service,
                json!({"type":"edit","doc":doc,"agent":mayor(),"text":text})
            )["kind"],
            "notice"
        );
    }
    assert_eq!(
        request(
            &mut service,
            json!({"type":"read","what":"papers","agent":mayor()})
        )["papers"]["character"],
        "🦀".repeat(6000)
    );
    assert_eq!(
        request(
            &mut service,
            json!({"type":"read","what":"papers","agent":mayor(),"full":true})
        )["papers"]["notes"],
        text
    );
    request(
        &mut service,
        json!({"type":"edit","doc":"charter","text":format!("Keep {token} secret.")}),
    );
    service.home.close().unwrap();
    let mut service = open(&dir);
    assert_eq!(
        request(
            &mut service,
            json!({"type":"read","what":"papers","agent":mayor(),"full":true})
        )["papers"]["character"],
        text
    );
    assert_eq!(
        request(
            &mut service,
            json!({"type":"read","what":"charter"})
        )["charter"]["text"],
        "Keep [REDACTED] secret."
    );
    assert_eq!(
        std::fs::read(dir.join("credentials.json")).unwrap(),
        credentials
    );
    request(
        &mut service,
        json!({"type":"edit","doc":"charter","text":" \u{feff} "}),
    );
    let default = request(
        &mut service,
        json!({"type":"read","what":"charter"}),
    );
    assert_eq!(default["charter"]["own"], false);
    request(
        &mut service,
        json!({"type":"edit","doc":"charter","text":default["charter"]["text"]}),
    );
    assert_eq!(
        request(
            &mut service,
            json!({"type":"read","what":"charter"})
        )["charter"]["own"],
        false
    );
    service.home.close().unwrap();
    let db = std::fs::read(dir.join("realm.sqlite")).unwrap();
    assert!(!db.windows(token.len()).any(|w| w == token.as_bytes()));
}

fn change(service: &mut Service, change: Change) -> bool {
    let chief = service.state(1, 0).unwrap().chief;
    let result = service
        .home
        .store()
        .run(
            &OperationId {
                runtime: RuntimeInstance::new("test").unwrap(),
                key: uuid::Uuid::new_v4().to_string(),
            },
            &RecordOperation {
                hamlet: "home".into(),
                actor: Actor::Chief(chief),
                at: 100,
                reason: "Because.".into(),
                task: None,
                change,
            },
        )
        .unwrap();
    matches!(result, Reply::Outcome(Outcome::Done(_)))
}

fn admit(name: &str) -> Change {
    Change::Admit(Admit {
        name: name.into(),
        purpose: "Inspect".into(),
        model: None,
        character: String::new(),
        roads: vec![],
    })
}

#[test]
fn a_limits_edit_changes_admission_atomically_and_refusals_change_nothing()
 {
    let dir = Scratch::new("protocol-limits");
    let mut service = open(&dir);
    assert_eq!(
        request(
            &mut service,
            json!({"type":"edit","doc":"limits","hamlet":"home","whole":{"mostAgents":1.0,"votes":{"passYes":1.0,"passOf":4.0}}})
        )["kind"],
        "notice"
    );
    assert!(change(&mut service, admit("scout")));
    assert!(!change(&mut service, admit("third")));
    let before = service.state(1, 0).unwrap();
    for whole in [
        json!({"mostAgents":10,"votes":{"passOf":0}}),
        json!({"permissions":{"network":false}}),
        json!({"hamlets":{"most":1.5}}),
        json!({"pinned":{"x":false}}),
        json!({"budget":{"cost":1e20}}),
        json!({"budget":{"minutes":i64::MAX}}),
        json!({"mostAgents":u64::MAX}),
    ] {
        assert_eq!(
            request(
                &mut service,
                json!({"type":"edit","doc":"limits","hamlet":"home","whole":whole})
            )["kind"],
            "error"
        );
        assert_eq!(service.state(1, 0).unwrap(), before);
        assert!(!change(&mut service, admit("third")));
    }
    let bad = request(
        &mut service,
        json!({"type":"edit","doc":"limits","hamlet":"home","set":{"section":"budget","key":"__proto__","value":null}}),
    );
    assert_eq!(bad["text"], "unknown field `__proto__` under budget");
}

#[test]
fn state_follows_office_roads_and_departures_from_the_record() {
    let dir = Scratch::new("protocol-record-state");
    let mut service = open(&dir);
    assert!(change(&mut service, admit("scout")));
    assert!(change(&mut service, admit("scribe")));
    assert!(change(&mut service, Change::Seat { number: 2 }));
    assert_eq!(
        request(
            &mut service,
            json!({"type":"edit","doc":"limits","hamlet":"home","set":{"section":"pinned","key":"mayor","value":"local::office"}})
        )["kind"],
        "notice"
    );
    let prompt = request(
        &mut service,
        json!({"type":"read","what":"prompt","agent":{"hamlet":"home","agent":"scribe"}}),
    );
    assert!(
        prompt["prompt"]
            .as_str()
            .unwrap()
            .contains("Model: local::office.")
    );
    let state = service.state(7, 3).unwrap();
    assert!(
        state
            .agents
            .iter()
            .find(|a| a.name == "scribe")
            .unwrap()
            .mayor
    );
    assert!(
        !state
            .agents
            .iter()
            .find(|a| a.name == "founder")
            .unwrap()
            .mayor
    );
    let roads = state.hamlets[0].roads.as_ref().unwrap();
    assert_eq!(roads.len(), 2);
    assert!(roads.iter().all(|r| r.a.number == 2 || r.b.number == 2));
    assert!(change(
        &mut service,
        Change::Depart {
            name: "scout".into(),
            how: Departure::Removed
        }
    ));
    let state = service.state(7, 4).unwrap();
    let gone =
        state.agents.iter().find(|a| a.name == "scout").unwrap();
    assert!(
        gone.left
            .as_ref()
            .unwrap()
            .words
            .contains("Its house is archived")
    );
    assert_eq!(state.hamlets[0].roads.as_ref().unwrap().len(), 1);
    let papers = request(
        &mut service,
        json!({"type":"read","what":"papers","agent":{"hamlet":"home","agent":"scout"}}),
    );
    assert_eq!(papers["kind"], "error");
    assert_eq!(papers["text"], gone.left.as_ref().unwrap().words);
    let encoded =
        serde_json::to_value(ServerMessage::State(state)).unwrap();
    let schema = serde_json::from_str::<Value>(include_str!(
        "../../../server/protocol.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(
        &json!({"$ref":"#/$defs/State","$defs":schema["$defs"]}),
    )
    .unwrap();
    assert!(validator.is_valid(&encoded));
}

#[cfg(unix)]
#[test]
fn notes_never_follow_a_link_out_of_the_house() {
    use std::os::unix::fs::symlink;
    let dir = Scratch::new("protocol-notes-links");
    let outside = Scratch::new("protocol-notes-outside");
    std::fs::write(outside.join("NOTES.md"), "Do not change.")
        .unwrap();
    let mut service = open(&dir);
    let notes = dir.join("houses/home/0/NOTES.md");
    std::fs::remove_file(&notes).unwrap();
    symlink(outside.join("NOTES.md"), &notes).unwrap();
    for parent_link in [false, true] {
        if parent_link {
            std::fs::remove_dir_all(dir.join("houses/home/0"))
                .unwrap();
            symlink(&*outside, dir.join("houses/home/0")).unwrap();
        }
        assert_eq!(
            request(
                &mut service,
                json!({"type":"edit","doc":"notes","agent":mayor(),"text":"changed"})
            )["kind"],
            "error"
        );
        assert_eq!(
            request(
                &mut service,
                json!({"type":"read","what":"papers","agent":mayor(),"full":true})
            )["papers"]["notes"],
            ""
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("NOTES.md"))
                .unwrap(),
            "Do not change."
        );
    }
}

#[test]
fn unsupported_owners_refuse_without_changing_the_realm() {
    let dir = Scratch::new("protocol-unsupported");
    let mut service = open(&dir);
    let before = service.state(1, 0).unwrap();
    for mut value in [
        json!({"type":"check","what":"connection"}),
        json!({"type":"say","to":mayor(),"text":"Hello"}),
        json!({"type":"login","start":{"provider":"openai","method":"key"}}),
    ] {
        value["id"] = uuid::Uuid::new_v4().to_string().into();
        let result = service
            .handle(
                Protocol::new()
                    .unwrap()
                    .parse(&value.to_string())
                    .unwrap(),
            )
            .unwrap()
            .unwrap();
        assert!(
            matches!(result,ServerMessage::Reply(r) if r.kind==ReplyKind::Error && r.text.contains("does not yet support"))
        );
        assert_eq!(service.state(1, 0).unwrap(), before);
    }
}

#[test]
fn budget_commands_reset_the_report_without_erasing_spend() {
    let dir = Scratch::new("protocol-budget");
    let mut service = open(&dir);
    for (section, value) in [("budget", 2.0), ("road", 0.125)] {
        assert_eq!(
            request(
                &mut service,
                json!({"type":"edit","doc":"limits","hamlet":"home","set":{"section":section,"key":"cost","value":value}})
            )["kind"],
            "notice"
        );
    }
    assert!(change(&mut service, admit("ana")));
    assert!(change(&mut service, admit("bo")));
    let chief = service.state(1, 0).unwrap().chief;
    let laid = service
        .home
        .store()
        .run(
            &OperationId {
                runtime: RuntimeInstance::new("budget-test").unwrap(),
                key: "road".into(),
            },
            &RecordOperation {
                hamlet: "home".into(),
                actor: Actor::Chief(chief),
                at: chrono::Local::now().timestamp_millis(),
                reason: "Trade".into(),
                task: Some("trade".into()),
                change: Change::LayRoad {
                    a: "ana".into(),
                    b: "bo".into(),
                },
            },
        )
        .unwrap();
    assert!(matches!(laid, Reply::Outcome(Outcome::Done(_))));
    assert_eq!(
        service.state(1, 0).unwrap().hamlets[0].budget.spent.micros,
        125000
    );
    let report =
        service.state(1, 0).unwrap().hamlets[0].budget.clone();
    assert_eq!(report.summary, "Hamlet today: $0.13 of $2.00");
    assert_eq!(report.lines[0], "- mayor: faux::faux-1, $0.13");
    assert_eq!(
        service.reset_budget("home").unwrap(),
        "The budget starts again from here. What was spent before \
         is kept in the ledger."
    );
    service.home.close().unwrap();
    let mut service = open(&dir);
    assert_eq!(
        service.state(1, 0).unwrap().hamlets[0].budget.spent.micros,
        0
    );
    let charges = service.home.store().road_charges("home").unwrap();
    assert_eq!(charges.len(), 1);
    assert_eq!(charges[0].amount.micros, 125000);
}
