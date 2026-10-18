//! The server and real pi harness runtimes as processes: a
//! conversation end to end, crashes at each commit point, waits
//! across restarts, admitting inputs once, cancelling and the
//! handshake's refusals.

#![cfg(unix)]

mod support;

use serde_json::{Value, json};
use support::{Driver, Realm};

/// The mayor's call laying the road between `ana` and `bo`.
fn road_call() -> Value {
    json!({"calls": [{"name": "build_road", "arguments": {
        "a": "ana", "b": "bo", "reason": "trade"}}]})
}

const LAID: &str = "Built a road between `ana` and `bo`.";

/// One road, one charge, one marker of it, one kept tool result with
/// the marked words, and the final answer `text` delivered.
fn laid_once(
    realm: &Realm,
    driver: &mut Driver,
    binding: i64,
    runtime: &str,
    text: &str,
) {
    driver.answer(binding, text);
    let look = realm.look();
    assert_eq!(
        look.roads,
        [("ana".to_string(), "bo".to_string())],
        "{look:?}"
    );
    assert_eq!(look.charges, [support::ROAD], "{look:?}");
    assert_eq!(look.markers.len(), 1, "{look:?}");
    let results = realm.kept_tool_results(runtime);
    assert_eq!(results.len(), 1, "{results:#?}");
    assert_eq!(
        results[0]["content"][0]["text"], LAID,
        "{results:#?}"
    );
    assert_eq!(results[0]["isError"], false, "{results:#?}");
}

#[test]
fn a_conversation_lays_a_road_through_the_record_and_answers() {
    let realm = Realm::new("normal");
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": [road_call(), "completed"]}}),
    );
    let mut driver = Driver::start(&realm, &faux, &[]);
    let (binding, runtime) = driver.bind(0);
    driver.submit(
        binding,
        "chief:1",
        "Lay the road between ana and bo.",
    );
    laid_once(&realm, &mut driver, binding, &runtime, "completed");
    let look = realm.look();
    // Two model calls, each allowed first and charged once.
    assert_eq!((look.attempts, look.usage), (2, 2), "{look:?}");
    assert_eq!(
        look.inputs,
        [vec![json!("chief:1"), json!("admitted")]]
    );
    driver.stop();
}

/// Starts a driver whose server stops at `point`, checks what is kept
/// there, kills it, and serves the home again with the script
/// `after`.
fn server_crash(
    tag: &str,
    point: &str,
    split: bool,
) -> (Realm, Driver, i64, String) {
    let realm = Realm::new(tag);
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": [road_call(), "completed"]}}),
    );
    let mut env = vec![("HAMLET_FAULT", point)];
    if split {
        env.push(("HAMLET_FAULT_SPLIT_MARKER", "1"));
    }
    let mut driver = Driver::start(&realm, &faux, &env);
    let (binding, runtime) = driver.bind(0);
    driver.submit(
        binding,
        "chief:1",
        "Lay the road between ana and bo.",
    );
    assert_eq!(realm.wait_paused("server"), point);
    let look = realm.look();
    assert!(realm.kept_tool_results(&runtime).is_empty());
    if point == "before_commit" {
        assert!(look.untouched(), "{look:?}");
    } else {
        assert_eq!(look.roads.len(), 1, "{look:?}");
        assert_eq!(look.charges.len(), 1, "{look:?}");
    }
    let pid = driver.runtime_pid();
    driver.kill();
    // No agent runs without its server (S21).
    support::gone(pid);
    let after = realm.script(
        "after.json",
        json!({"agents": {"home:0": ["continued after recovery"]}}),
    );
    let mut env = Vec::new();
    if split {
        env.push(("HAMLET_FAULT_SPLIT_MARKER", "1"));
    }
    let driver = Driver::start(&realm, &after, &env);
    (realm, driver, binding, runtime)
}

#[test]
fn a_server_killed_before_its_commit_runs_the_call_once_after() {
    let (realm, mut driver, binding, runtime) =
        server_crash("before-commit", "before_commit", false);
    laid_once(
        &realm,
        &mut driver,
        binding,
        &runtime,
        "continued after recovery",
    );
    driver.stop();
}

#[test]
fn a_server_killed_after_its_commit_gives_the_marked_result() {
    let (realm, mut driver, binding, runtime) =
        server_crash("after-commit", "after_commit", false);
    laid_once(
        &realm,
        &mut driver,
        binding,
        &runtime,
        "continued after recovery",
    );
    driver.stop();
}

#[test]
fn control_a_change_and_marker_committed_apart_lose_the_result() {
    let (realm, mut driver, binding, runtime) =
        server_crash("split-marker", "after_commit", true);
    driver.answer(binding, "continued after recovery");
    // The check of a whole run must catch what this order loses.
    let results = realm.kept_tool_results(&runtime);
    assert_eq!(results.len(), 1, "{results:#?}");
    assert_ne!(results[0]["content"][0]["text"], LAID);
    assert!(
        results[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("already"),
        "{results:#?}"
    );
    driver.stop();
}

/// Starts a driver whose runtime stops at `point`, checks what is
/// kept there, and kills the runtime, which the server starts again.
fn runtime_crash(
    tag: &str,
    point: &str,
    env: &[(&str, &str)],
) -> (Realm, Driver, i64, String) {
    let realm = Realm::new(tag);
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": [road_call(), "completed"]}}),
    );
    let mut env = env.to_vec();
    env.push(("HAMLET_HARNESS_FAULT", point));
    let mut driver = Driver::start(&realm, &faux, &env);
    let (binding, runtime) = driver.bind(0);
    driver.submit(
        binding,
        "chief:1",
        "Lay the road between ana and bo.",
    );
    assert_eq!(realm.wait_paused("runtime"), point);
    assert!(realm.kept_tool_results(&runtime).is_empty());
    // The next runtime reads its script afresh.
    realm.script(
        "faux.json",
        json!({"agents": {"home:0": ["continued after recovery"]}}),
    );
    let pid = driver.runtime_pid();
    restarted(&mut driver, pid);
    (realm, driver, binding, runtime)
}

/// Kills the runtime `pid` and waits until the server started it
/// again and took it on.
fn restarted(driver: &mut Driver, pid: u32) {
    let ready = driver
        .notices
        .iter()
        .filter(|n| n["kind"] == "ready")
        .count();
    support::kill(pid);
    driver
        .until(|n| n["kind"] == "exited" && n["restart_ms"].is_u64());
    driver.until_nth(ready + 1, |n| n["kind"] == "ready");
}

#[test]
fn a_runtime_killed_after_the_result_before_pi_kept_it_gets_the_marked_result()
 {
    let realm_check = |realm: &Realm| {
        let look = realm.look();
        assert_eq!(look.roads.len(), 1, "{look:?}");
        assert_eq!(look.markers.len(), 1, "{look:?}");
    };
    let (realm, mut driver, binding, runtime) =
        runtime_crash("after-receive", "after_receive", &[]);
    realm_check(&realm);
    laid_once(
        &realm,
        &mut driver,
        binding,
        &runtime,
        "continued after recovery",
    );
    driver.stop();
}

#[test]
fn control_a_tool_unsafe_to_replay_is_not_run_again() {
    let (realm, mut driver, binding, runtime) = runtime_crash(
        "unsafe-replay",
        "after_receive",
        &[("HAMLET_UNSAFE_TOOLS", "build_road")],
    );
    driver.answer(binding, "continued after recovery");
    let results = realm.kept_tool_results(&runtime);
    assert_eq!(results.len(), 1, "{results:#?}");
    assert_eq!(results[0]["isError"], true, "{results:#?}");
    assert!(
        results[0].to_string().contains("interrupted"),
        "{results:#?}"
    );
    driver.stop();
}

/// The mayor's road call waiting for a chief across a restart of both
/// processes; returns the driver serving again and the wait's key.
fn wait_across_restart(
    tag: &str,
) -> (Realm, Driver, i64, String, String) {
    let realm = Realm::new(tag);
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": [road_call(), "completed"]}}),
    );
    let asks = [("HAMLET_ASK_TOOLS", "build_road")];
    let mut driver = Driver::start(&realm, &faux, &asks);
    let (binding, runtime) = driver.bind(0);
    driver.submit(
        binding,
        "chief:1",
        "Lay the road between ana and bo.",
    );
    let waiting = driver.until(|n| n["kind"] == "waiting");
    let key = waiting["key"].as_str().unwrap().to_string();
    let look = realm.look();
    assert!(look.untouched(), "{look:?}");
    assert_eq!(look.waits, [vec![json!(key), json!("pending")]]);
    let pid = driver.runtime_pid();
    driver.kill();
    support::gone(pid);
    let after = realm.script(
        "after.json",
        json!({"agents": {"home:0": ["continued after recovery"]}}),
    );
    let driver = Driver::start(&realm, &after, &asks);
    // The same wait, under the same identity, and still nothing done.
    let look = realm.look();
    assert!(look.untouched(), "{look:?}");
    assert_eq!(look.waits, [vec![json!(key), json!("pending")]]);
    (realm, driver, binding, runtime, key)
}

#[test]
fn a_wait_survives_a_restart_and_its_allowing_lays_the_road() {
    let (realm, mut driver, binding, runtime, key) =
        wait_across_restart("wait-allow");
    let decided = driver.send(json!({
        "do": "decide", "runtime": runtime, "key": key, "allow": true,
    }));
    assert_eq!(decided["decided"], "Answered");
    laid_once(
        &realm,
        &mut driver,
        binding,
        &runtime,
        "continued after recovery",
    );
    driver.stop();
}

#[test]
fn a_wait_survives_a_restart_and_its_refusal_lays_nothing() {
    let (realm, mut driver, binding, runtime, key) =
        wait_across_restart("wait-refuse");
    let decided = driver.send(json!({
        "do": "decide", "runtime": runtime, "key": key, "allow": false,
    }));
    assert_eq!(decided["decided"], "Answered");
    driver.answer(binding, "continued after recovery");
    let look = realm.look();
    assert!(look.untouched(), "{look:?}");
    let results = realm.kept_tool_results(&runtime);
    assert_eq!(results.len(), 1, "{results:#?}");
    assert_eq!(results[0]["isError"], true, "{results:#?}");
    // Refused by the early ask, before the call was kept as begun.
    let text = results[0]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("A chief refused this call."), "{text}");
    driver.stop();
}

#[test]
fn a_permission_taken_back_after_pi_kept_its_intent_is_refused_as_it_runs()
 {
    let realm = Realm::new("revoked");
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": [road_call(), "completed"]}}),
    );
    let mut driver = Driver::start(
        &realm,
        &faux,
        &[("HAMLET_HARNESS_FAULT", "before_operation")],
    );
    let (binding, runtime) = driver.bind(0);
    driver.submit(
        binding,
        "chief:1",
        "Lay the road between ana and bo.",
    );
    assert_eq!(realm.wait_paused("runtime"), "before_operation");
    // Allowed early, then kept by pi as begun.
    let look = realm.look();
    assert_eq!(look.waits.len(), 1, "{look:?}");
    assert_eq!(look.waits[0][1], "allowed", "{look:?}");
    let key = look.waits[0][0].as_str().unwrap().to_string();
    let decided = driver.send(json!({
        "do": "decide", "runtime": runtime, "key": key, "allow": false,
    }));
    assert_eq!(decided["decided"], "Answered");
    realm.script(
        "faux.json",
        json!({"agents": {"home:0": ["continued after recovery"]}}),
    );
    let pid = driver.runtime_pid();
    restarted(&mut driver, pid);
    driver.answer(binding, "continued after recovery");
    let look = realm.look();
    assert!(look.untouched(), "{look:?}");
    let results = realm.kept_tool_results(&runtime);
    assert_eq!(results.len(), 1, "{results:#?}");
    assert_eq!(results[0]["isError"], true, "{results:#?}");
    // The call's own answer: pi ran it again without the early ask,
    // and the server refused it as it ran.
    assert_eq!(
        results[0]["content"][0]["text"],
        "A chief refused this call."
    );
    driver.stop();
}

/// A road call between `a` and `b`, with `extra` arguments added.
fn road(a: &str, b: &str, extra: Value) -> Value {
    let mut arguments = json!({"a": a, "b": b, "reason": "trade"});
    arguments
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().cloned().unwrap_or_default());
    json!({"calls": [{"name": "build_road", "arguments": arguments}]})
}

/// The text and failure of each tool result the runtime kept, in
/// order.
fn results(realm: &Realm, runtime: &str) -> Vec<(String, bool)> {
    realm
        .kept_tool_results(runtime)
        .iter()
        .map(|result| {
            (
                result["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                result["isError"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_conversation_s_calls_meet_the_hamlet_s_rules_as_they_run() {
    let realm = Realm::new("rules");
    let none = json!({});
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": [
            road("ana", "bo", none.clone()), "one",
            road("ana", "cy", none.clone()), "two",
            road("bo", "ana", none.clone()), "again",
            road("bo", "cy", none), "three",
        ]}}),
    );
    let mut driver = Driver::start(&realm, &faux, &[]);
    let (binding, runtime) = driver.bind(0);
    for (request, answer) in
        [("1", "one"), ("2", "two"), ("3", "again"), ("4", "three")]
    {
        driver.submit(binding, request, "Lay the next road.");
        driver.answer(binding, answer);
    }
    let look = realm.look();
    // Each road charged once; the third would pass the cap.
    assert_eq!(
        look.charges,
        [support::ROAD, support::ROAD],
        "{look:?}"
    );
    assert_eq!(look.roads.len(), 2, "{look:?}");
    let results = results(&realm, &runtime);
    assert_eq!(results.len(), 4, "{results:#?}");
    assert!(!results[0].1 && !results[1].1, "{results:#?}");
    assert!(results[2].1, "{results:#?}");
    assert!(
        results[2].0.contains("already have a road"),
        "{results:#?}"
    );
    assert!(results[3].1, "{results:#?}");
    // Every call was marked, refused ones too, so none runs again.
    assert_eq!(look.markers.len(), 4, "{look:?}");
    driver.stop();
}

#[test]
fn a_member_s_conversation_claiming_the_mayor_is_refused() {
    let realm = Realm::new("member");
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:1": [
            road("ana", "bo", json!({"actor": {"mayor": {"hamlet": "home", "number": 0}}})),
            road("ana", "bo", json!({})),
            "refused",
        ]}}),
    );
    let mut driver = Driver::start(&realm, &faux, &[]);
    // The conversation is bound to ana, a member.
    let (binding, runtime) = driver.bind(1);
    driver.submit(
        binding,
        "chief:1",
        "Lay the road between ana and bo.",
    );
    driver.answer(binding, "refused");
    let look = realm.look();
    assert!(
        look.roads.is_empty() && look.charges.is_empty(),
        "{look:?}"
    );
    let results = results(&realm, &runtime);
    assert_eq!(results.len(), 2, "{results:#?}");
    // An argument naming who acts is no argument of the tool; pi
    // checks the schema first, and the server's own check is
    // tested without it.
    assert!(results[0].1, "{results:#?}");
    assert!(results[0].0.contains("actor"), "{results:#?}");
    // Who acts is the agent the conversation is bound to.
    assert_eq!(
        results[1],
        ("Only the mayor may do that.".to_string(), true)
    );
    driver.stop();
}

/// Stops the runtime at `point` while it admits the first input,
/// kills it, and checks the input was admitted once when served
/// again.
fn admitted_once(tag: &str, point: &str, in_pi: u32) {
    let realm = Realm::new(tag);
    // Held, so the turn cannot end before the runtime is killed.
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": [{"text": "held", "holdUntil": "never"}]}}),
    );
    let mut driver = Driver::start(
        &realm,
        &faux,
        &[("HAMLET_HARNESS_FAULT", point)],
    );
    let (binding, runtime) = driver.bind(0);
    driver.submit(binding, "chief:1", "Hello.");
    assert_eq!(realm.wait_paused("runtime"), point);
    // Kept by the server, and in pi's store or not yet.
    assert_eq!(
        realm.look().inputs,
        [vec![json!("chief:1"), json!("kept")]]
    );
    assert_eq!(realm.submissions(&runtime), in_pi);
    realm.script(
        "faux.json",
        json!({"agents": {"home:0": ["first", "second", "third"]}}),
    );
    let pid = driver.runtime_pid();
    restarted(&mut driver, pid);
    driver.until(|n| {
        n["kind"] == "admitted" && n["request"] == "chief:1"
    });
    driver.answer(binding, "first");
    driver.submit(binding, "chief:2", "Again.");
    driver.answer(binding, "second");
    let said = driver
        .history(binding)
        .into_iter()
        .filter(|item| {
            item["kind"] == "user" || item["kind"] == "assistant"
        })
        .map(|item| item["text"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(said, ["Hello.", "first", "Again.", "second"]);
    assert_eq!(realm.submissions(&runtime), 2);
    let look = realm.look();
    assert_eq!(
        look.inputs,
        [
            vec![json!("chief:1"), json!("admitted")],
            vec![json!("chief:2"), json!("admitted")],
        ]
    );
    driver.stop();
}

#[test]
fn an_input_cut_off_before_pi_admitted_it_is_admitted_once() {
    admitted_once("before-admit", "before_admit", 0);
}

#[test]
fn an_input_cut_off_after_pi_admitted_it_is_admitted_once() {
    admitted_once("after-admit", "after_admit", 1);
}

#[test]
fn cancelling_stops_the_turn_lapses_its_wait_and_drops_its_queue() {
    let realm = Realm::new("cancel");
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": [road_call(), "never"]}}),
    );
    let asks = [("HAMLET_ASK_TOOLS", "build_road")];
    let mut driver = Driver::start(&realm, &faux, &asks);
    let (binding, runtime) = driver.bind(0);
    driver.submit(
        binding,
        "chief:1",
        "Lay the road between ana and bo.",
    );
    let key = driver.until(|n| n["kind"] == "waiting")["key"].clone();
    driver.submit(binding, "chief:2", "Queued.");
    driver.until(|n| {
        n["kind"] == "admitted" && n["request"] == "chief:2"
    });
    driver.send(json!({"do": "cancel", "binding": binding}));
    driver.until(|n| n["kind"] == "cancelled");
    let look = realm.look();
    assert!(look.untouched(), "{look:?}");
    assert_eq!(look.waits, [vec![key.clone(), json!("refused")]]);
    // A chief's later yes does not bring the call back.
    let decided = driver.send(json!({
        "do": "decide", "runtime": runtime, "key": key, "allow": true,
    }));
    assert_eq!(decided["decided"], "Unchanged");
    // Nothing of the stopped turn or its queue comes back after a
    // restart.
    realm.script(
        "faux.json",
        json!({"agents": {"home:0": ["after"]}}),
    );
    let pid = driver.runtime_pid();
    restarted(&mut driver, pid);
    driver.submit(binding, "chief:3", "Next.");
    driver.answer(binding, "after");
    let history = driver.history(binding);
    let said = history
        .iter()
        .filter(|item| {
            item["kind"] == "user" || item["kind"] == "assistant"
        })
        .filter(|item| item["text"] != "")
        .map(|item| item["text"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        said,
        ["Lay the road between ana and bo.", "Next.", "after"]
    );
    let results = results(&realm, &runtime);
    assert_eq!(results.len(), 1, "{results:#?}");
    assert!(
        results[0].1 && results[0].0.contains("aborted"),
        "{results:#?}"
    );
    assert!(realm.look().untouched());
    driver.stop();
}

#[test]
fn a_cancel_while_the_runtime_is_down_reaches_it_when_it_is_back() {
    let realm = Realm::new("cancel-down");
    // Held, so the turn runs until it is stopped.
    let held = json!({"text": "held", "holdUntil": "never"});
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": [held.clone(), held]}}),
    );
    let mut driver = Driver::start(&realm, &faux, &[]);
    let (binding, runtime) = driver.bind(0);
    driver.submit(binding, "chief:1", "Hello.");
    driver.until(|n| n["kind"] == "running" && n["running"] == true);
    driver.submit(binding, "chief:2", "Queued.");
    driver.until(|n| {
        n["kind"] == "admitted" && n["request"] == "chief:2"
    });
    assert_eq!(
        realm.submission_states(&runtime),
        ["placed", "queued"]
    );
    let ready = driver
        .notices
        .iter()
        .filter(|n| n["kind"] == "ready")
        .count();
    let pid = driver.runtime_pid();
    support::kill(pid);
    driver.until(|n| n["kind"] == "exited");
    driver.send(json!({"do": "cancel", "binding": binding}));
    driver.until_nth(ready + 1, |n| n["kind"] == "ready");
    driver.until(|n| n["kind"] == "cancelled");
    // The recovered turn stopped, and the queued input was withdrawn.
    assert_eq!(
        realm.submission_states(&runtime),
        ["unanswered", "unanswered"]
    );
    driver.stop();
}

#[test]
fn a_runtime_whose_store_is_gone_is_refused_in_words() {
    let realm = Realm::new("missing");
    let faux = realm.script(
        "faux.json",
        json!({"agents": {"home:0": ["hello"]}}),
    );
    let mut driver = Driver::start(&realm, &faux, &[]);
    let (binding, runtime) = driver.bind(0);
    driver.submit(binding, "chief:1", "Hello.");
    driver.answer(binding, "hello");
    driver.stop();
    std::fs::remove_dir_all(
        realm.home.join("harnesses").join(&runtime),
    )
    .unwrap();
    let mut driver = Driver::start(&realm, &faux, &[]);
    let refused = driver.until(|n| n["kind"] == "refused");
    let words = refused["words"].as_str().unwrap();
    assert!(
        words.contains(&format!(
            "The store of the runtime {runtime} is missing"
        )),
        "{words}"
    );
    // Left down, and not started again into an empty store.
    driver.until(|n| {
        n["kind"] == "exited" && n["restart_ms"].is_null()
    });
    assert!(
        !realm
            .home
            .join("harnesses")
            .join(&runtime)
            .join("pi.sqlite")
            .exists()
    );
    driver.stop();
}

#[test]
fn a_runtime_given_another_runtime_s_store_is_refused_in_words() {
    let start = |tag: &str| {
        let realm = Realm::new(tag);
        let faux = realm.script(
            "faux.json",
            json!({"agents": {"home:0": ["hello"]}}),
        );
        let mut driver = Driver::start(&realm, &faux, &[]);
        let (binding, runtime) = driver.bind(0);
        driver.submit(binding, "chief:1", "Hello.");
        driver.answer(binding, "hello");
        driver.stop();
        (realm, faux, runtime)
    };
    let (realm, faux, runtime) = start("ours");
    let (other, _, theirs) = start("theirs");
    let ours = realm.home.join("harnesses").join(&runtime);
    std::fs::remove_dir_all(&ours).unwrap();
    std::fs::create_dir_all(&ours).unwrap();
    for file in
        std::fs::read_dir(other.home.join("harnesses").join(&theirs))
            .unwrap()
    {
        let file = file.unwrap();
        std::fs::copy(file.path(), ours.join(file.file_name()))
            .unwrap();
    }
    let mut driver = Driver::start(&realm, &faux, &[]);
    let refused = driver.until(|n| n["kind"] == "refused");
    let words = refused["words"].as_str().unwrap();
    assert!(
        words.contains(&format!(
            "belongs to the runtime {theirs}, not {runtime}."
        )),
        "{words}"
    );
    driver.until(|n| {
        n["kind"] == "exited" && n["restart_ms"].is_null()
    });
    driver.stop();
}

#[test]
fn a_real_endpoint_gets_the_credential_the_server_hands_over_and_nothing_keeps_it()
 {
    let tokio = tokio::runtime::Runtime::new().unwrap();
    let mut endpoint =
        tokio.block_on(hamlet_acceptance::openai::Endpoint::start());
    let realm = Realm::new("endpoint");
    endpoint.enqueue([
        hamlet_acceptance::openai::Response {
            status: 503,
            text: "Busy".into(),
            ..Default::default()
        },
        hamlet_acceptance::openai::Response {
            text: "Hello sk-held.".into(),
            ..Default::default()
        },
    ]);
    let credentials = json!({"models": {"endpoint": {"type": "api_key", "key": "sk-held"}}});
    std::fs::write(
        realm.home.join("credentials.json"),
        credentials.to_string(),
    )
    .unwrap();
    let env = [
        ("HAMLET_MODEL", "endpoint::org/large"),
        ("HAMLET_BASE_URL", endpoint.url.as_str()),
    ];
    let mut driver = Driver::start_on(&realm, None, &env);
    let (binding, runtime) = driver.bind(0);
    driver.submit(binding, "chief:1", "Hello?");
    driver.answer(binding, "Hello [REDACTED].");
    driver.stop();
    let completions = endpoint
        .requests()
        .iter()
        .filter(|request| request.path.ends_with("/chat/completions"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 2, "{completions:#?}");
    assert_eq!(
        completions[0].authorization.as_deref(),
        Some("Bearer sk-held")
    );
    assert_eq!(
        completions[0].body.as_ref().unwrap()["model"],
        "org/large"
    );
    assert_eq!(completions[0].body, completions[1].body);
    assert_eq!(
        completions[0].authorization,
        completions[1].authorization
    );
    let look = realm.look();
    assert_eq!((look.attempts, look.usage), (2, 2), "{look:?}");
    // The endpoint's bill, charged once, in micro-dollars.
    assert_eq!(look.micros, 1_500, "{look:?}");
    let db =
        rusqlite::Connection::open(realm.home.join("realm.sqlite"))
            .unwrap();
    let usage = hamlet_server::spend::usage(
        &db,
        hamlet_server::harness::BindingId(binding),
    )
    .unwrap();
    assert_eq!(
        (usage.input, usage.output, usage.micros),
        (100, 20, 1500)
    );
    // pi was handed the key for the call and stored it nowhere.
    for file in
        std::fs::read_dir(realm.home.join("harnesses").join(&runtime))
            .unwrap()
    {
        let bytes = std::fs::read(file.unwrap().path()).unwrap();
        assert!(!bytes.windows(7).any(|window| window == b"sk-held"));
    }
}
