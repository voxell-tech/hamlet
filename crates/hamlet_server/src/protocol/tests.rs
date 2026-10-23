//! Chief documents and record views through the server's protocol
//! seam.

use hamlet_protocol::client::{ClientMessage, ServerMessage};
use hamlet_testing::Scratch;
use schemars::generate::SchemaSettings;
use serde_json::{Value, json};

use super::*;
use crate::record::{
    Actor, Admit, Change, Departure, RecordOperation,
};
use crate::service::Service;
use crate::{
    Home, OperationId, Outcome, Overrides, Reply, RuntimeInstance,
};

/// `future` run to its end on a runtime of its own, as the server's
/// house work needs one.
fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}

fn request(service: &mut Service, mut value: Value) -> Value {
    let id = uuid::Uuid::new_v4().to_string();
    value["id"] = id.clone().into();
    let answer = match Protocol::new()
        .unwrap()
        .parse(&value.to_string())
        .unwrap()
    {
        ClientMessage::Edit(edit) => {
            run(service.edit_document(&id, edit))
        }
        ClientMessage::Read(read) => run(service.read(read)),
        message => panic!("not a document request: {message:?}"),
    };
    serde_json::to_value(answer.unwrap()).unwrap()
}

fn open(dir: &Scratch) -> Service {
    Service::new(Home::open(dir).unwrap(), Overrides::none(true))
        .unwrap()
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
    let text = format!(" \u{85} {}tail  ", "🦀".repeat(6001));
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
        json!({"type":"edit","doc":"charter","text":" \u{85} "}),
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
        let mut after = service.state(1, 0).unwrap();
        // Calendar progress can advance independently of an edit.
        for (after, before) in
            after.hamlets.iter_mut().zip(&before.hamlets)
        {
            after.budget.gone = before.budget.gone;
        }
        assert_eq!(after, before);
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
    let roads = &state.hamlets[0].roads;
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
    assert_eq!(state.hamlets[0].roads.len(), 1);
    let papers = request(
        &mut service,
        json!({"type":"read","what":"papers","agent":{"hamlet":"home","agent":"scout"}}),
    );
    assert_eq!(papers["kind"], "error");
    assert_eq!(papers["text"], gone.left.as_ref().unwrap().words);
    let encoded =
        serde_json::to_value(ServerMessage::State(state)).unwrap();
    let schema = SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<ServerMessage>();
    let validator =
        jsonschema::validator_for(&schema.to_value()).unwrap();
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

#[test]
fn numeric_spelling_does_not_change_the_message() {
    let protocol = Protocol::new().unwrap();
    assert!(matches!(
        protocol.parse(r#"{"type":"hello","v":0.0,"token":"test"}"#),
        Ok(ClientMessage::Hello(_))
    ));
    let id = "00000000-0000-4000-8000-000000000001";
    let text = format!(
        r#"{{"type":"edit","doc":"settings","id":"{id}","set":{{"reflectEvery":2e0}}}}"#
    );
    assert!(matches!(
        protocol.parse(&text),
        Ok(ClientMessage::Edit(_))
    ));
    let invalid = text.replace("2e0", "2.5");
    let fault = protocol.parse(&invalid).unwrap_err();
    assert_eq!(fault.to.as_deref(), Some(id));
    assert_eq!(
        fault.text,
        "This `edit` message does not fit the protocol: \
         `set.reflectEvery` must be integer."
    );
}

#[test]
fn malformed_fields_are_correlated_only_to_a_valid_id() {
    let protocol = Protocol::new().unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    for (fields, words) in [
        (
            r#""doc":"character","agent":{"hamlet":"home","agent":"mayor"},"text":7"#,
            "`text` must be string",
        ),
        (
            r#""doc":"limits","hamlet":"home","set":{"section":"budget","key":"cost"}"#,
            "`set` lacks `value`",
        ),
        (
            r#""doc":"limits","hamlet":"home","whole":12"#,
            "`whole` must be object",
        ),
        (
            r#""doc":"settings","set":{"model":null}"#,
            "`set.model` must be string",
        ),
        (r#""doc":"charter","text":7"#, "`text` must be string"),
    ] {
        let message =
            format!(r#"{{"type":"edit","id":"{id}",{fields}}}"#);
        let fault = protocol.parse(&message).unwrap_err();
        assert_eq!(fault.to.as_deref(), Some(id));
        assert_eq!(
            fault.text,
            format!(
                "This `edit` message does not fit the protocol: \
                 {words}."
            )
        );
        assert_eq!(
            protocol
                .parse(&message.replace(id, "not-an-id"))
                .unwrap_err()
                .to,
            None
        );
    }
}

#[test]
fn the_schema_takes_as_an_id_just_what_request_id_does() {
    let protocol = Protocol::new().unwrap();
    let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
    for text in [
        id.to_owned(),
        id.to_uppercase(),
        id.replace('-', ""),
        format!("{{{id}}}"),
        format!("urn:uuid:{id}"),
        id.replacen('-', "", 1) + "0",
        id.replace('e', "g"),
        format!("{}\u{e9}", &id[..34]),
        String::new(),
    ] {
        let message = serde_json::json!({"type": "stop", "id": text})
            .to_string();
        match protocol.parse(&message) {
            Ok(_) => assert!(text.parse::<RequestId>().is_ok()),
            Err(fault) => {
                assert!(text.parse::<RequestId>().is_err());
                assert_eq!(
                    fault.text,
                    "This `stop` message does not fit the protocol: \
                     `id` must be a UUID."
                );
            }
        }
    }
}

#[test]
fn a_malformed_pick_names_what_it_lacks() {
    let protocol = Protocol::new().unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    let message = format!(
        r#"{{"type":"answer","id":"{id}","wait":"w","choice":"reply","picks":[{{"kind":"offer"}}]}}"#
    );
    assert_eq!(
        protocol.parse(&message).unwrap_err().text,
        "This `answer` message does not fit the protocol: `picks.0` \
         lacks `index`."
    );
}
