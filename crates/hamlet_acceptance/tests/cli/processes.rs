//! Local server and window lifetimes through the installed commands.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_acceptance::{Cli, Home, Script, openai, server};
use serde_json::Value;

struct Local(Home);

impl Drop for Local {
    fn drop(&mut self) {
        if let Ok(bytes) = std::fs::read(self.0.file("server.json"))
            && let Ok(record) =
                serde_json::from_slice::<Value>(&bytes)
            && let Some(pid) = record["pid"].as_i64()
            && let Ok(pid) = i32::try_from(pid)
            && pid > 0
        {
            // SAFETY: the positive PID belongs to this test's scratch
            // server.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }
}

fn executable(home: &Home, name: &str, contents: &str) -> PathBuf {
    let path = home.side().join(name);
    std::fs::write(&path, contents).unwrap();
    std::fs::set_permissions(
        &path,
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    path
}

fn app(home: &Home) -> PathBuf {
    executable(
        home,
        "window",
        "#!/bin/sh\nprintf '%s\\n' \"$HAMLET_SERVER\" \
         \"$HAMLET_TOKEN\" \"$HAMLET_VIEW\" > \
         \"$APP_CAPTURE\"\nexit 3\n",
    )
}

async fn captured(path: &Path) -> Vec<String> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(path) {
                let lines = text
                    .lines()
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                if lines.len() == 3 {
                    return lines;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the window received its address and view")
}

fn record(home: &Home) -> Value {
    serde_json::from_slice(
        &std::fs::read(home.file("server.json")).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn bare_command_reuses_the_server_after_the_window_closes() {
    let endpoint = openai::Endpoint::start().await;
    let local = Local(Home::new());
    let home = &local.0;
    let app = app(home);
    let capture = home.side().join("capture");
    let view = home.side().join("view");
    let env = [
        ("HAMLET_APP", app.to_str().unwrap()),
        ("APP_CAPTURE", capture.to_str().unwrap()),
        ("HAMLET_VIEW", view.to_str().unwrap()),
        ("HAMLET_MODEL", "local::org/large"),
        ("HAMLET_BASE_URL", endpoint.url.as_str()),
        ("HAMLET_REFLECT_EVERY", "0"),
    ];
    let started = Cli::run(home.path(), &[], "", &env).await;
    assert_eq!(started.code(), Some(0), "{started:?}");
    let first = record(home);
    let window = captured(&capture).await;
    assert_eq!(window[0], first["url"].as_str().unwrap());
    assert_eq!(window[2], view.to_str().unwrap());
    let credentials = serde_json::from_slice::<Value>(
        &std::fs::read(home.file("credentials.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        window[1],
        credentials["connection"]["token"].as_str().unwrap()
    );
    std::fs::remove_file(&capture).unwrap();
    let again = Cli::run(home.path(), &[], "", &env).await;
    assert_eq!(again.code(), Some(0), "{again:?}");
    assert_eq!(record(home)["pid"], first["pid"]);
    assert_eq!(captured(&capture).await, window);
    let status = Cli::run(home.path(), &["status"], "", &[]).await;
    assert_eq!(status.code(), Some(0), "{status:?}");
    let closed = Cli::run(home.path(), &["window"], "", &env).await;
    assert_eq!(closed.code(), Some(3), "{closed:?}");
    let stopped = Cli::run(home.path(), &["stop"], "", &[]).await;
    assert_eq!(stopped.code(), Some(0), "{stopped:?}");
    assert!(!home.file("server.json").exists());
    let status = Cli::run(home.path(), &["status"], "", &[]).await;
    assert_eq!(status.code(), Some(1), "{status:?}");
}

#[tokio::test]
async fn dev_uses_binary_overrides_and_stops_the_server_at_ctrl_c() {
    let local = Local(Home::new());
    let home = &local.0;
    let app = app(home);
    let capture = home.side().join("capture");
    let selected = home.side().join("selected-server");
    let binary = server::cli().with_file_name("hamlet-server");
    let server = executable(
        home,
        "chosen-server",
        &format!(
            "#!/bin/sh\nprintf chosen > '{}'\nexec '{}' \"$@\"\n",
            selected.display(),
            binary.display(),
        ),
    );
    let script = home.side().join("faux.json");
    Script::new().write(&script);
    let mut dev = Cli::start(
        home.path(),
        &["dev", "--faux", script.to_str().unwrap()],
        &[
            ("HAMLET_APP", app.to_str().unwrap()),
            ("HAMLET_SERVER_BIN", server.to_str().unwrap()),
            ("APP_CAPTURE", capture.to_str().unwrap()),
            ("HAMLET_REFLECT_EVERY", "0"),
        ],
    );
    dev.err_shows("window closed; still serving").await;
    assert_eq!(std::fs::read_to_string(selected).unwrap(), "chosen");
    let window = captured(&capture).await;
    assert_eq!(window[0], record(home)["url"].as_str().unwrap());
    assert_eq!(window[2], home.file("window").to_str().unwrap());
    let status = Cli::run(home.path(), &["status"], "", &[]).await;
    assert_eq!(status.code(), Some(0), "{status:?}");
    dev.interrupt();
    let stopped = dev.wait().await;
    assert_eq!(stopped.code(), Some(0), "{stopped:?}");
    assert!(!home.file("server.json").exists());
}

#[tokio::test]
async fn dev_launches_cargos_reported_server_and_window() {
    let local = Local(Home::new());
    let home = &local.0;
    let app = app(home);
    let capture = home.side().join("capture");
    let selected = home.side().join("selected-server");
    let binary = server::cli().with_file_name("hamlet-server");
    let server = executable(
        home,
        "built-server",
        &format!(
            "#!/bin/sh\nprintf chosen > '{}'\nexec '{}' \"$@\"\n",
            selected.display(),
            binary.display(),
        ),
    );
    let artifact = |name, path: &Path| {
        serde_json::json!({
            "reason": "compiler-artifact", "target": {"name": name},
            "executable": path,
        })
        .to_string()
    };
    executable(
        home,
        "cargo",
        &format!(
            "#!/bin/sh\ncase \"$3\" in\nhamlet-server) printf \
             '%s\\n' '{}' ;;\nhamlet-app) printf '%s\\n' '{}' \
             ;;\n*) exit 1 ;;\nesac\n",
            artifact("hamlet-server", &server),
            artifact("hamlet-app", &app),
        ),
    );
    let path = std::env::join_paths(
        std::iter::once(home.side().to_path_buf()).chain(
            std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            ),
        ),
    )
    .unwrap();
    let script = home.side().join("faux.json");
    Script::new().write(&script);
    let mut dev = Cli::start(
        home.path(),
        &["dev", "--faux", script.to_str().unwrap()],
        &[
            ("HAMLET_APP", ""),
            ("HAMLET_SERVER_BIN", ""),
            ("PATH", path.to_str().unwrap()),
            ("APP_CAPTURE", capture.to_str().unwrap()),
            ("HAMLET_REFLECT_EVERY", "0"),
        ],
    );
    dev.err_shows("window closed; still serving").await;
    assert_eq!(std::fs::read_to_string(selected).unwrap(), "chosen");
    assert_eq!(
        captured(&capture).await[0],
        record(home)["url"].as_str().unwrap()
    );
    dev.interrupt();
    let stopped = dev.wait().await;
    assert_eq!(stopped.code(), Some(0), "{stopped:?}");
    assert!(!home.file("server.json").exists());
}
