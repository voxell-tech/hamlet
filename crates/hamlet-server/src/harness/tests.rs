//! What the server makes of a runtime's claims, without a runtime:
//! who acts comes from the binding alone, an identity runs one call,
//! and a marked result or a charge stands.

use std::collections::BTreeMap;

use hamlet_testing::Scratch;
use serde_json::{Value, json};

use super::answers::{self, Answer};
use super::link::{ModelRef, Usage};
use super::{Binding, Decided, parse, tools};
use crate::record::{
    Actor, Admit, AgentRef, Change, FoundRecord, RecordOperation,
};
use crate::{OperationId, RuntimeInstance, Store};

const LAID: &str = "Built a road between `ana` and `bo`.";

/// A store whose hamlet `home` has the mayor `founder` (0) and the
/// member `ana` (1), and a conversation bound to agent `number`.
fn bound(scratch: &Scratch, number: u64) -> (Store, Binding) {
    let mut store =
        Store::open(&scratch.join("realm.sqlite")).unwrap();
    let seed = |key: &str| OperationId {
        runtime: RuntimeInstance::new("seed").unwrap(),
        key: key.into(),
    };
    let found = FoundRecord {
        hamlet: "home".into(),
        name: "founder".into(),
        purpose: "Leads".into(),
        chief: "chief".into(),
        at: 0,
    };
    store.run(&seed("found"), &found).unwrap();
    for name in ["ana", "bo"] {
        let admit = RecordOperation {
            hamlet: "home".into(),
            actor: Actor::Chief("chief".into()),
            at: 1,
            reason: "Seeded".into(),
            task: None,
            change: Change::Admit(Admit {
                name: name.into(),
                purpose: "Works".into(),
                model: None,
                character: String::new(),
                roads: Vec::new(),
            }),
        };
        store.run(&seed(name), &admit).unwrap();
    }
    let agent = AgentRef {
        hamlet: "home".into(),
        number,
    };
    let model = ModelRef {
        provider: "faux".into(),
        id: "faux-1".into(),
        base_url: None,
    };
    let runtime = store.runtime_for("pi").unwrap();
    let id =
        store.add_binding(&agent, false, &model, &runtime).unwrap();
    let binding = store.binding(id).unwrap().unwrap();
    (store, binding)
}

fn arguments(value: Value) -> BTreeMap<String, Value> {
    serde_json::from_value(value).unwrap()
}

fn road() -> BTreeMap<String, Value> {
    arguments(json!({"a": "ana", "b": "bo", "reason": "trade"}))
}

fn never(_: &str) -> bool {
    false
}

fn always(_: &str) -> bool {
    true
}

fn roads(store: &Store) -> usize {
    store.record("home").unwrap().unwrap().roads.len()
}

fn run(
    store: &mut Store,
    binding: &Binding,
    key: &str,
    arguments: &BTreeMap<String, Value>,
) -> Answer {
    answers::operation(
        store,
        binding,
        key,
        "build_road",
        arguments,
        never,
        2,
    )
    .unwrap()
    .answer
}

fn result(text: &str, error: bool) -> Answer {
    Answer::Result {
        text: text.into(),
        error,
    }
}

#[test]
fn a_runtime_claiming_the_mayor_for_a_member_s_conversation_is_refused()
 {
    let scratch = Scratch::new("harness-claim");
    let (mut store, binding) = bound(&scratch, 1);
    // A claim in the call's arguments is no argument of the tool.
    for claim in [
        json!({"mayor": {"hamlet": "home", "number": 0}}),
        json!("mayor"),
    ] {
        let mut claimed = road();
        claimed.insert("actor".into(), claim);
        let Answer::Result { text, error } =
            run(&mut store, &binding, "claimed", &claimed)
        else {
            panic!("not a result")
        };
        assert!(
            error && text.contains("unknown field `actor`"),
            "{text}"
        );
    }
    // A claim beside the call is no field of the link's operation.
    let line = json!({
        "type": "operation", "v": 1, "id": "r1", "conversation": "1",
        "key": "k", "tool": "build_road", "arguments": road(),
        "actor": {"mayor": {"hamlet": "home", "number": 0}},
    });
    let (words, re) = parse(&line.to_string()).err().unwrap();
    assert!(words.contains("not name"), "{words}");
    assert_eq!(re.as_deref(), Some("r1"));
    // The call itself runs as the member the conversation is bound
    // to.
    assert_eq!(
        run(&mut store, &binding, "plain", &road()),
        result("Only the mayor may do that.", true)
    );
    assert_eq!(roads(&store), 0);
    // The mayor's own conversation lays it.
    let founder = AgentRef {
        hamlet: "home".into(),
        number: 0,
    };
    let mayor = store
        .add_binding(
            &founder,
            false,
            &binding.model,
            &binding.runtime,
        )
        .unwrap();
    let mayor = store.binding(mayor).unwrap().unwrap();
    assert_eq!(
        run(&mut store, &mayor, "mayor", &road()),
        result(LAID, false)
    );
}

#[test]
fn an_identity_runs_one_call_and_its_marked_result_stands() {
    let scratch = Scratch::new("harness-identity");
    let (mut store, binding) = bound(&scratch, 0);
    assert_eq!(
        run(&mut store, &binding, "k", &road()),
        result(LAID, false)
    );
    // Taking back a call that ran undoes nothing, and asking again
    // gives the marked result rather than a refusal or a second
    // road.
    let runtime = binding.runtime.clone();
    assert_eq!(
        store.decide(&runtime, "k", false).unwrap(),
        Decided::AlreadyRan
    );
    assert_eq!(
        run(&mut store, &binding, "k", &road()),
        result(LAID, false)
    );
    assert_eq!(roads(&store), 1);
    // The same identity with other arguments runs nothing.
    let other = arguments(
        json!({"a": "ana", "b": "founder", "reason": "trade"}),
    );
    assert_eq!(
        run(&mut store, &binding, "k", &other),
        result(tools::CONFLICT, true)
    );
    assert_eq!(roads(&store), 1);
}

#[test]
fn a_waiting_identity_asked_with_other_arguments_is_refused() {
    let scratch = Scratch::new("harness-waiting");
    let (mut store, binding) = bound(&scratch, 0);
    let asked = answers::allow(
        &mut store,
        &binding,
        "k",
        "build_road",
        &road(),
        always,
    )
    .unwrap();
    assert!(asked.new_wait);
    assert!(matches!(asked.answer, Answer::Waiting(_)));
    let runtime = binding.runtime.clone();
    assert_eq!(
        store.decide(&runtime, "k", true).unwrap(),
        Decided::Answered
    );
    let other = arguments(
        json!({"a": "ana", "b": "founder", "reason": "trade"}),
    );
    let asked = answers::operation(
        &mut store,
        &binding,
        "k",
        "build_road",
        &other,
        always,
        2,
    )
    .unwrap();
    assert_eq!(asked.answer, Answer::Refused(tools::CONFLICT.into()));
    assert_eq!(roads(&store), 0);
}

#[test]
fn usage_is_charged_once_and_only_to_an_allowed_attempt() {
    let scratch = Scratch::new("harness-usage");
    let (mut store, binding) = bound(&scratch, 0);
    let usage = |cost: f64| Usage {
        input: 3,
        output: 4,
        cache_read: 0,
        cache_write: 0,
        cost,
    };
    let runtime = binding.runtime.clone();
    assert!(
        answers::charge(&mut store, &runtime, "a", &usage(1.0), 2)
            .unwrap()
            .is_none()
    );
    answers::attempt(
        &mut store,
        &binding,
        "a",
        &binding.model,
        |_| None,
        2,
    )
    .unwrap();
    for cost in [0.000_25, 9.0] {
        answers::charge(&mut store, &runtime, "a", &usage(cost), 2)
            .unwrap();
    }
    let charged = store
        .db
        .query_row(
            "SELECT count(*), sum(micros) FROM harness_usage",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    // The first report stands; the cost is kept in micro-dollars.
    assert_eq!(charged, (1, 250));
}
