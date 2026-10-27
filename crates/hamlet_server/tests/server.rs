//! The `hamlet_server` executable refusing to start: it ends before
//! it starts any model provider, so these tests run no pi and need no
//! Node. The ones that reach a served home are in `tests/crashes`.

#![cfg(unix)]

use std::process::Command;

use hamlet_server::Home;
use hamlet_testing::{Scratch, strip_overrides};

/// The `hamlet_server` these tests are built with.
const SERVER: &str = env!("CARGO_BIN_EXE_hamlet_server");

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
