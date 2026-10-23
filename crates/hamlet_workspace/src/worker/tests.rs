use hamlet_testing::Scratch;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::*;

const TOKEN: &str = "worker-test-token-123456";
const ID: &str = "5b0f3f7e-8a52-4c2b-9d1e-7f6a3c2b1a09";

/// The shared protocol fixture, whose replies the worker gives.
const FIXTURE: &str = include_str!(
    "../../../hamlet_workspace_wire/fixtures/protocol-v1.json"
);

/// A worker on `root` serving the home `home` with [`TOKEN`].
fn worker(root: &Path) -> Worker {
    Worker {
        root: root.to_path_buf(),
        homes: tokens(&HashMap::from([(
            "home".to_owned(),
            TOKEN.to_owned(),
        )]))
        .unwrap(),
        network: root.join("network.sock"),
        running: HashMap::new(),
        tasks: JoinSet::new(),
    }
}

/// The worker's reply to `message` sent with `token`, which it must
/// give before running anything.
fn call(worker: &mut Worker, token: &str, message: &Value) -> Value {
    let (reply, mut receive) = oneshot::channel();
    worker
        .on_call(http::Call {
            authorization: format!("Bearer {token}"),
            body: message.to_string().into_bytes(),
            reply,
        })
        .unwrap();
    assert!(worker.tasks.is_empty(), "nothing may run");
    serde_json::to_value(receive.try_recv().expect("a reply"))
        .unwrap()
}

fn execute(id: &str) -> Value {
    json!({"kind": "execute", "request": {
        "version": 1, "id": id, "workspace": "fixture", "agent": 1,
        "project": false, "network": false, "read_only": false,
        "operation": {"kind": "exec", "argv": ["/bin/true"],
                      "cwd": "/house", "timeout_ms": 10000},
    }})
}

#[test]
fn answers_the_fixture_s_refusals_alike() {
    let scratch = Scratch::new("worker");
    let mut worker = worker(&scratch);
    let fixture = serde_json::from_str::<Value>(FIXTURE).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    for name in [
        "cancel_before_admission",
        "cancelled_execute",
        "unsupported_version",
    ] {
        let case =
            cases.iter().find(|case| case["name"] == name).unwrap();
        assert_eq!(
            call(&mut worker, TOKEN, &case["message"]),
            case["reply"],
            "{name}"
        );
    }
}

#[test]
fn refuses_a_token_of_no_home() {
    let scratch = Scratch::new("worker");
    let mut worker = worker(&scratch);
    let reply =
        call(&mut worker, "another-token-1234567", &execute(ID));
    assert_eq!(reply["code"], "permission_denied");
    assert!(!scratch.join("home").exists());
}

#[test]
fn refuses_a_request_without_a_valid_id() {
    let scratch = Scratch::new("worker");
    let mut worker = worker(&scratch);
    let reply = call(&mut worker, TOKEN, &execute("not-a-uuid"));
    assert_eq!(reply["code"], "invalid");
}

#[test]
fn a_cancellation_before_admission_outlasts_a_restart() {
    let scratch = Scratch::new("worker");
    let cancel = json!({"kind": "cancel", "id": ID});
    assert_eq!(
        call(&mut worker(&scratch), TOKEN, &cancel)["value"]["state"],
        "cancelled"
    );
    let mut restarted = worker(&scratch);
    let reply = call(&mut restarted, TOKEN, &execute(ID));
    assert_eq!(reply["code"], "aborted");
    assert_eq!(reply["uncertain"], false);
    assert_eq!(
        call(&mut restarted, TOKEN, &cancel)["value"]["state"],
        "cancelled"
    );
}

#[test]
fn an_admitted_id_is_never_run_again() {
    let scratch = Scratch::new("worker");
    let journal = scratch.join("home/operations");
    fs::create_dir_all(&journal).unwrap();
    mark(&journal, &journal.join(ID), &[]).unwrap();
    let mut worker = worker(&scratch);
    let reply = call(&mut worker, TOKEN, &execute(ID));
    assert_eq!(reply["code"], "uncertain");
    assert_eq!(reply["uncertain"], true);
    let cancel = json!({"kind": "cancel", "id": ID});
    assert_eq!(
        call(&mut worker, TOKEN, &cancel)["value"]["state"],
        "completed"
    );
}

#[test]
fn homes_need_a_name_and_a_token_of_their_own() {
    let homes = |entries: &[(&str, &str)]| {
        tokens(
            &entries
                .iter()
                .map(|(home, token)| {
                    ((*home).to_owned(), (*token).to_owned())
                })
                .collect(),
        )
    };
    assert!(homes(&[]).is_err());
    assert!(homes(&[("home", "short")]).is_err());
    assert!(homes(&[("a/b", TOKEN)]).is_err());
    assert!(homes(&[("one", TOKEN), ("two", TOKEN)]).is_err());
    assert!(
        homes(&[("one", TOKEN), ("two", "worker-test-token-654321")])
            .is_ok()
    );
}
