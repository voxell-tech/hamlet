//! What the server makes of a runtime's claims, without a runtime:
//! who acts comes from the binding alone, an identity runs one call,
//! and a marked result or a charge stands.

use std::collections::BTreeMap;

use hamlet_protocol::v0::Period;
use hamlet_testing::Scratch;
use serde_json::{Value, json};

use super::link::{ModelRef, Price, Usage};
use super::{Answer, Binding, InputStatus, answers, parse, tools};
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

fn roads(store: &Store) -> usize {
    store.record("home").unwrap().unwrap().roads.len()
}

fn run(
    store: &mut Store,
    binding: &Binding,
    key: &str,
    arguments: &BTreeMap<String, Value>,
) -> Answer {
    let call = match tools::parse("build_road", arguments) {
        Ok(call) => call,
        Err(words) => return Answer::result(words, true),
    };
    let id = OperationId {
        runtime: binding.runtime.clone(),
        key: key.into(),
    };
    let (text, error) =
        tools::run(store, &id, binding, &call, 2).unwrap();
    Answer::result(text, error)
}

fn result(text: &str, error: bool) -> Answer {
    Answer::result(text, error)
}

/// A price with cached tokens at the input rate.
fn price(input: f64, output: f64) -> Price {
    Price {
        input,
        output,
        cache_read: input,
        cache_write: input,
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
        let Answer::Result { text, error, .. } =
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
    // Asking again gives the marked result rather than a refusal or
    // a second road.
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
fn usage_is_charged_once_and_only_to_an_allowed_attempt() {
    let scratch = Scratch::new("harness-usage");
    let (mut store, binding) = bound(&scratch, 0);
    let usage = |cost: f64| Usage {
        input: 3,
        output: 4,
        cache_read: 0,
        cache_write: 0,
        cost,
        ms: None,
        estimated_tokens: None,
        billed: None,
        first_token_ms: None,
        reasoning: None,
        stop: None,
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
        None,
        &price(1.0, 1.0),
        2,
    )
    .unwrap();
    for cost in [0.000_25, 9.0] {
        answers::charge(&mut store, &runtime, "a", &usage(cost), 2)
            .unwrap();
    }
    let charged = store
        .connection()
        .query_row(
            "SELECT count(*), sum(micros) FROM harness_usage",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    // The first report stands; the cost is kept in micro-dollars.
    assert_eq!(charged, (1, 250));
}

#[test]
fn caps_count_once_keep_task_spend_after_reset_and_roll_over_local_days()
 {
    use chrono::{Local, TimeZone};

    use crate::spend;
    let scratch = Scratch::new("spend-caps");
    let (mut store, binding) = bound(&scratch, 0);
    let at = Local
        .with_ymd_and_hms(2026, 5, 15, 12, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    let mut limits =
        crate::limits::of(store.connection(), "home").unwrap();
    limits.budget.cost = Some(0.001);
    limits.budget.minutes = None;
    limits.budget.period = Period::Day;
    limits.task.cost = Some(0.0005);
    crate::limits::set(store.connection(), "home", limits.clone())
        .unwrap();
    let price = price(2.0, 3.0);
    assert!(
        answers::attempt(
            &mut store,
            &binding,
            "first",
            &binding.model,
            Some("task-a"),
            &price,
            at
        )
        .unwrap()
        .is_none()
    );
    let usage = serde_json::from_value::<Usage>(json!({"input":100,"output":200,"cacheRead":0,"cacheWrite":0,"cost":0,"ms":60001})).unwrap();
    answers::charge(
        &mut store,
        &binding.runtime,
        "first",
        &usage,
        at,
    )
    .unwrap();
    answers::charge(
        &mut store,
        &binding.runtime,
        "first",
        &usage,
        at,
    )
    .unwrap();
    assert_eq!(
        spend::usage(store.connection(), binding.id).unwrap().micros,
        800
    );
    assert!(
        answers::attempt(
            &mut store,
            &binding,
            "second",
            &binding.model,
            Some("task-a"),
            &price,
            at
        )
        .unwrap()
        .unwrap()
        .words
        .starts_with("This task")
    );
    // A reset and a later charge in the same millisecond are ordered
    // by the ledger.
    store
        .connection()
        .execute(
            "INSERT INTO spend_reset(hamlet,at,by,models,roads) \
             VALUES ('home',?1,'chief',1,0)",
            [at],
        )
        .unwrap();
    assert_eq!(
        spend::books(
            store.connection(),
            "home",
            "day",
            Some("task-a"),
            at
        )
        .unwrap()
        .hamlet
        .micros,
        0
    );
    assert!(
        spend::can_spend(
            store.connection(),
            "home",
            Some("task-a"),
            at
        )
        .unwrap()
        .unwrap()
        .starts_with("This task")
    );
    assert!(
        answers::attempt(
            &mut store,
            &binding,
            "new",
            &binding.model,
            Some("task-b"),
            &price,
            at
        )
        .unwrap()
        .is_none()
    );
    answers::charge(&mut store, &binding.runtime, "new", &usage, at)
        .unwrap();
    assert_eq!(
        spend::books(store.connection(), "home", "day", None, at)
            .unwrap()
            .hamlet
            .micros,
        800
    );
    limits.budget.minutes = Some(1);
    crate::limits::set(store.connection(), "home", limits.clone())
        .unwrap();
    assert!(
        spend::can_spend(store.connection(), "home", None, at)
            .unwrap()
            .unwrap()
            .contains("model time")
    );
    let tomorrow = Local
        .with_ymd_and_hms(2026, 5, 16, 0, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    assert!(
        spend::can_spend(store.connection(), "home", None, tomorrow)
            .unwrap()
            .is_none()
    );
    assert!(
        spend::can_spend(
            store.connection(),
            "home",
            Some("task-a"),
            tomorrow
        )
        .unwrap()
        .unwrap()
        .starts_with("This task")
    );
    limits.budget.period = Period::Month;
    crate::limits::set(store.connection(), "home", limits).unwrap();
    assert!(
        spend::can_spend(store.connection(), "home", None, tomorrow)
            .unwrap()
            .unwrap()
            .contains("this month")
    );
    store.close().unwrap();
    let store = Store::open(&scratch.join("realm.sqlite")).unwrap();
    assert_eq!(
        spend::usage(store.connection(), binding.id).unwrap().calls,
        2
    );
}

#[test]
fn billed_zero_beats_prices_and_unreported_success_counts_estimates()
{
    let scratch = Scratch::new("spend-billed");
    let (mut store, binding) = bound(&scratch, 0);
    let price = price(10.0, 20.0);
    for (key, usage) in [
        (
            "reported",
            json!({"input":17000,"output":1240,"cacheRead":7,"cacheWrite":11,"cost":1,"billed":0.4249,"reasoning":9}),
        ),
        (
            "free",
            json!({"input":100,"output":20,"cacheRead":0,"cacheWrite":0,"cost":12,"billed":0}),
        ),
        (
            "estimated",
            json!({"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"cost":0,"estimatedTokens":2350}),
        ),
        (
            "failed",
            json!({"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"cost":0,"stop":"error"}),
        ),
    ] {
        assert!(
            answers::attempt(
                &mut store,
                &binding,
                key,
                &binding.model,
                None,
                &price,
                0
            )
            .unwrap()
            .is_none()
        );
        answers::charge(
            &mut store,
            &binding.runtime,
            key,
            &serde_json::from_value(usage).unwrap(),
            0,
        )
        .unwrap();
    }
    let view =
        crate::spend::usage(store.connection(), binding.id).unwrap();
    assert_eq!(
        (
            view.calls,
            view.input,
            view.output,
            view.micros,
            view.unreported
        ),
        (4, 17118, 1260, 448400, 2)
    );
    assert_eq!(
        (view.cached, view.reasoning, view.last_input),
        (7, 9, 100)
    );
    assert_eq!(view.summary, "18.4k tok, $0.45");
    let books = crate::spend::books(
        store.connection(),
        "home",
        "day",
        None,
        0,
    )
    .unwrap();
    assert_eq!(books.hamlet.tokens, 20728);
}

#[test]
fn every_attempt_uses_the_current_pin_then_hamlet_then_home_model() {
    let scratch = Scratch::new("model-choice");
    let (store, binding) = bound(&scratch, 0);
    std::fs::write(scratch.join("models.json"), r#"{"endpoint":[{"provider":"endpoint","id":"pinned","cost":{"input":8,"output":9}}]}"#).unwrap();
    let settings = crate::settings::Settings {
        model: "endpoint::home".into(),
        ..Default::default()
    };
    crate::settings::keep(store.connection(), &settings).unwrap();
    let mut limits =
        crate::limits::of(store.connection(), "home").unwrap();
    limits.model = Some("endpoint::hamlet".into());
    limits.pinned = BTreeMap::from([(
        hamlet_protocol::MAYOR.to_owned(),
        "endpoint::pinned".to_owned(),
    )]);
    crate::limits::set(store.connection(), "home", limits.clone())
        .unwrap();
    crate::service::save(
        store.connection(),
        "catalogue",
        "home",
        &json!({"endpoint::pinned":{"price":{"input":0,"output":2}}}),
    )
    .unwrap();
    let chosen = |store: &Store| {
        crate::models::Choices::read(store, &scratch, false)
            .unwrap()
            .select(store, &binding.agent)
            .unwrap()
    };
    let pinned = chosen(&store);
    assert_eq!(pinned.model.id, "pinned");
    assert_eq!(pinned.price, price(0.0, 2.0));
    store.close().unwrap();
    let store = Store::open(&scratch.join("realm.sqlite")).unwrap();
    assert_eq!(chosen(&store).model.id, "pinned");
    limits.pinned.clear();
    crate::limits::set(store.connection(), "home", limits.clone())
        .unwrap();
    let hamlet = chosen(&store);
    assert_eq!(hamlet.model.id, "hamlet");
    // Unlisted and unpriced, at the rate of a remote endpoint.
    assert_eq!(hamlet.price, price(1.0, 1.0));
    limits.model = None;
    crate::limits::set(store.connection(), "home", limits).unwrap();
    assert_eq!(chosen(&store).model.id, "home");
}

#[test]
fn a_hello_lacking_the_link_version_or_a_needed_capability_is_refused()
 {
    use std::path::Path;

    use super::link::{Capability, Hello, HelloStore};
    use super::{LINK_VERSION, NEEDED, refusal};

    let row = super::bindings::RuntimeRow {
        id: RuntimeInstance::new("pi-1").unwrap(),
        adapter: Some("hamlet-pi 0.1.0".into()),
        version: Some("pi-durable 1.2.0".into()),
        begun: true,
    };
    let hello = Hello {
        adapter: "hamlet-pi 0.1.0".into(),
        capabilities: NEEDED.to_vec(),
        harness: "pi-durable 1.2.0".into(),
        id: "r1".into(),
        runtime: Some("pi-1".into()),
        store: HelloStore::Opened,
        v: LINK_VERSION,
        versions: vec![LINK_VERSION],
    };
    let store = Path::new("pi.sqlite");
    assert_eq!(refusal(&hello, &row, store), None);
    let words = |hello: &Hello| refusal(hello, &row, store).unwrap();
    let mut other = hello.clone();
    other.versions = vec![LINK_VERSION + 1];
    assert!(words(&other).contains("link versions"), "{other:?}");
    // History is optional; recovery is not.
    let mut lacking = hello.clone();
    lacking.capabilities = vec![Capability::History];
    lacking.capabilities.extend(
        NEEDED
            .iter()
            .filter(|needed| **needed != Capability::Recovery)
            .cloned(),
    );
    assert!(words(&lacking).ends_with("harness: recovery."));
    let mut upgraded = hello.clone();
    upgraded.harness = "pi-durable 1.3.0".into();
    assert!(words(&upgraded).contains("never changes"));
    let mut recorded = hello;
    recorded.runtime = None;
    assert!(words(&recorded).contains("records no runtime"));
}

/// Harnesses that start no runtime, with `model` as if
/// `HAMLET_MODEL` named it.
fn idle(
    home: &crate::Home,
    model: Option<ModelRef>,
) -> super::Harnesses {
    use super::{Config, Harnesses, Pi};

    let (sender, receiver) = super::mpsc::unbounded_channel();
    Harnesses {
        config: Config {
            pi: Pi {
                node: "node".into(),
                entry: "unused".into(),
                faux: None,
            },
            model,
            tools: vec![],
        },
        hand_tools: vec![],
        layout: home.layout().clone(),
        runtimes: vec![],
        sender,
        receiver,
        session: Default::default(),
        held_turns: Default::default(),
        provider_requests: Default::default(),
        credentials: Default::default(),
        closing: false,
    }
}

#[tokio::test]
async fn a_new_conversation_runs_the_agents_model_with_no_model_in_the_environment()
 {
    let scratch = Scratch::new("bind-model");
    let home = crate::Home::open(&scratch.join("layout")).unwrap();
    let (mut store, binding) = bound(&scratch, 1);
    let settings = crate::settings::Settings {
        model: "endpoint::home".into(),
        ..Default::default()
    };
    crate::settings::keep(store.connection(), &settings).unwrap();
    let mut harnesses = idle(&home, None);
    let ready = harnesses.add(binding.runtime.clone());
    harnesses.runtimes[ready].phase = super::Phase::Ready;
    let (made, _) = harnesses
        .bind(&mut store, &binding.agent, false)
        .await
        .unwrap();
    let model = store.binding(made).unwrap().unwrap().model;
    assert_eq!(
        (model.provider.as_str(), model.id.as_str()),
        ("endpoint", "home")
    );
}

#[tokio::test]
async fn a_bad_created_reply_stops_only_its_runtime_and_refuses_new_bindings()
 {
    use super::{Event, Inner, Notice, Pending, Phase, Received};

    let scratch = Scratch::new("bad-created");
    let home = crate::Home::open(&scratch.join("layout")).unwrap();
    let (mut store, binding) = bound(&scratch, 0);
    store.conversation_made(binding.id, "1", "pi", "1").unwrap();
    let other = store
        .add_binding(
            &binding.agent,
            false,
            &binding.model,
            &binding.runtime,
        )
        .unwrap();
    let mut harnesses = idle(&home, Some(binding.model.clone()));
    let bad = harnesses.add(binding.runtime.clone());
    let good =
        harnesses.add(RuntimeInstance::new("unaffected").unwrap());
    harnesses.runtimes[bad].phase = Phase::Ready;
    harnesses.runtimes[good].phase = Phase::Ready;
    harnesses
        .session
        .expect("create", bad, Pending::Create(other));
    let credentials =
        crate::credentials::Credentials::open(home.layout().root())
            .unwrap();
    let notices = harnesses.handle(&mut store, Event(Inner::Received(Received {
        runtime: bad, generation: 0,
        line: Some(json!({"v":1,"type":"created","id":"r1","re":"create","conversation":"1"}).to_string()),
    })), &credentials).await.unwrap();
    assert!(notices.iter().any(|n| matches!(n, Notice::Refused { runtime, .. } if *runtime == binding.runtime)));
    assert!(harnesses.runtimes[good].phase == Phase::Ready);
    assert!(
        harnesses
            .bind(&mut store, &binding.agent, false)
            .await
            .is_err()
    );
    assert_eq!(store.bindings_of(&binding.runtime).unwrap().len(), 2);
    assert!(
        store
            .binding(other)
            .unwrap()
            .unwrap()
            .conversation
            .is_none()
    );
}

#[test]
fn an_unanswered_input_is_dropped_and_never_resent() {
    let scratch = Scratch::new("unanswered-input");
    let (mut store, binding) = bound(&scratch, 0);
    store
        .keep_input(
            binding.id,
            &super::Input {
                request: "chief:1",
                content: "Hello",
                steer: false,
                mark: None,
                write: false,
                boundary: false,
            },
        )
        .unwrap();
    store
        .input_stands(
            binding.id,
            "chief:1",
            Some("1"),
            InputStatus::Unanswered,
            Some("withdrawn"),
            None,
        )
        .unwrap();
    assert!(store.owed(binding.id, true).unwrap().is_empty());
    let state = store
        .connection()
        .query_row(
            "SELECT state FROM harness_input WHERE request = \
             'chief:1'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(state, "dropped");
    store
        .input_stands(
            binding.id,
            "chief:1",
            Some("1"),
            InputStatus::Queued,
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        store
            .standing(binding.id, "chief:1")
            .unwrap()
            .unwrap()
            .status,
        Some(InputStatus::Unanswered)
    );
}
