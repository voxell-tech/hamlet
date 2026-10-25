//! The `hamlet_server` executable refusing to start: it ends before
//! it starts any model provider, so these tests run no pi and need no
//! Node. The ones that reach a served home are in `tests/crashes`.

#![cfg(unix)]

use std::process::Command;

use hamlet_server::Home;
use hamlet_testing::{Scratch, strip_overrides};
use serde_json::json;

/// The `hamlet_server` these tests are built with.
const SERVER: &str = env!("CARGO_BIN_EXE_hamlet_server");

#[test]
fn a_missing_pi_harness_is_refused() {
    let home = Scratch::new("server-no-pi");
    let entry = home.join("missing/main.ts");
    let words = refusal(
        &home,
        &[("HAMLET_PI_HARNESS", entry.to_str().unwrap())],
    );
    assert!(
        words.contains(&format!(
            "HAMLET_PI_HARNESS names {}, which is not a file.",
            entry.display()
        )),
        "{words}"
    );
}

/// The words `hamlet_server` gives on standard error as it refuses to
/// start on `home` with `env`.
fn refusal(home: &Scratch, env: &[(&str, &str)]) -> String {
    #[expect(
        clippy::disallowed_methods,
        reason = "the test runs the server executable"
    )]
    let output = strip_overrides(&mut Command::new(SERVER))
        .args(["--home".as_ref(), home.as_os_str()])
        .args(["--port", "0"])
        .env_remove("HAMLET_HOME")
        .env("HAMLET_USER", "chief")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert!(!output.status.success());
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn a_model_not_named_endpoint_and_id_is_refused() {
    let home = Scratch::new("server-bad-model");
    let words = refusal(&home, &[("HAMLET_MODEL", "garbage")]);
    assert!(
        words.contains(
            "HAMLET_MODEL is `endpoint::id`, not `garbage`"
        ),
        "{words}"
    );
}

#[test]
fn a_home_record_without_a_chief_is_refused() {
    use hamlet_server::{FoundRecord, OperationId, RuntimeInstance};

    let dir = Scratch::new("server-chiefless");
    let mut home = Home::open(&dir).unwrap();
    let id = OperationId {
        runtime: RuntimeInstance::new("seed").unwrap(),
        key: "found".into(),
    };
    let found = FoundRecord {
        hamlet: "home".into(),
        name: "founder".into(),
        purpose: String::new(),
        chief: "chief".into(),
        at: 0,
    };
    home.store().run(&id, &found).unwrap();
    home.store()
        .connection()
        .execute(
            "UPDATE record_hamlet SET document = json_set(document, \
             '$.chiefs', json('[]')) WHERE id = 'home'",
            [],
        )
        .unwrap();
    home.close().unwrap();
    let words = refusal(&dir, &[]);
    assert!(
        words.contains("The home hamlet's record names no chief."),
        "{words}"
    );
}

#[test]
fn a_new_home_without_a_chief_name_is_refused() {
    let home = Scratch::new("server-no-chief");
    #[expect(
        clippy::disallowed_methods,
        reason = "the test runs the server executable"
    )]
    let output = strip_overrides(&mut Command::new(SERVER))
        .args(["--home".as_ref(), home.as_os_str()])
        .args(["--port", "0"])
        .env_remove("HAMLET_HOME")
        .env_remove("HAMLET_USER")
        .env_remove("USER")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let words = String::from_utf8(output.stderr).unwrap();
    assert!(
        words.contains(
            "A new home needs its chief's name: set HAMLET_USER or \
             USER."
        ),
        "{words}"
    );
}

#[test]
fn a_base_url_for_an_endpoint_the_settings_do_not_list_is_refused() {
    let home = Scratch::new("server-unlisted-base-url");
    let words = refusal(
        &home,
        &[
            ("HAMLET_MODEL", "elsewhere::large"),
            ("HAMLET_BASE_URL", "http://localhost:9999/v1"),
        ],
    );
    assert!(
        words.contains(
            "HAMLET_BASE_URL is for the endpoint of the model \
             `elsewhere::large`, which the settings do not list."
        ),
        "{words}"
    );
    let words = refusal(
        &home,
        &[
            ("HAMLET_MODEL", "anthropic::large"),
            ("HAMLET_BASE_URL", "http://localhost:9999/v1"),
        ],
    );
    assert!(
        words.contains(
            "HAMLET_BASE_URL is for the endpoint of the model \
             `anthropic::large`, a provider pi knows the address \
             of. Unset HAMLET_BASE_URL."
        ),
        "{words}"
    );
    // With no model chosen in Settings and none in HAMLET_MODEL.
    let none = Scratch::new("server-base-url-no-model");
    let mut store = Home::open(&none).unwrap();
    let settings = json!({
        "model": "", "reflectEvery": 10, "allowNet": false,
        "endpoints": [], "saved": true,
        "link": {"state": "untested", "models": null, "words": null},
    });
    store
        .store()
        .connection()
        .execute(
            "INSERT INTO chief_document(kind, key, value) VALUES \
             ('settings', 'home', ?1)",
            [settings.to_string()],
        )
        .unwrap();
    store.close().unwrap();
    let words = refusal(
        &none,
        &[("HAMLET_BASE_URL", "http://localhost:9999/v1")],
    );
    assert!(
        words.contains(
            "HAMLET_BASE_URL needs a model: set HAMLET_MODEL or \
             choose one in Settings."
        ),
        "{words}"
    );
}
