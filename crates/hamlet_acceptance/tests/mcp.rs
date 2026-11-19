//! An MCP server listed in the realm: run under the sandbox in its
//! own folder, its tools offered and called, its secrets kept out of
//! what the model is given.

use std::path::Path;
use std::time::Duration;

use hamlet_acceptance::*;
use hamlet_testing::{calls, words};
use serde_json::json;

/// Whether a file under `folder` holds `text`.
fn holds(folder: &Path, text: &str) -> bool {
    std::fs::read_dir(folder).unwrap().flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            holds(&path, text)
        } else {
            std::fs::read(&path).is_ok_and(|bytes| {
                bytes
                    .windows(text.len())
                    .any(|at| at == text.as_bytes())
            })
        }
    })
}

#[tokio::test]
async fn an_mcp_tool_runs_on_its_server_and_gives_its_result_without_secrets()
 {
    let home = Home::new();
    let program = Path::new(env!("CARGO_BIN_EXE_mcp_echo"));
    std::fs::create_dir_all(home.file("mcp")).unwrap();
    std::fs::write(
        home.file("mcp/servers.json"),
        json!({"servers": {"echo": {
            "command": [program],
            "env": {"TOKEN": "mcp-secret-token"},
            "sandbox": {"fs_ro": [program.parent().unwrap()]},
        }}})
        .to_string(),
    )
    .unwrap();
    let mut s = Session::start_in(
        home,
        Faux::new().agent(
            2,
            [
                calls([
                    ("mcp__echo__echo", json!({"text": "hello"})),
                    ("mcp__echo__token", json!({})),
                ]),
                words("Called."),
            ],
        ),
        QUIET,
    )
    .await;
    // The tools are offered once the server lists them.
    let listed = s.home.file("mcp/echo/listed");
    while !listed.exists() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Call the server.").await;
    s.chief.shows(&now, "Called.").await;
    let failures = s
        .chief
        .items(&now)
        .into_iter()
        .filter_map(|item| item.tool)
        .map(|tool| (tool.name, tool.failure))
        .collect::<Vec<_>>();
    assert_eq!(
        failures,
        [
            ("mcp__echo__echo".to_owned(), None),
            ("mcp__echo__token".to_owned(), None),
        ]
    );
    assert_eq!(
        std::fs::read_to_string(s.home.file("mcp/echo/echoed"))
            .unwrap(),
        "hello"
    );
    // The harness keeps what its model was given.
    let harness = s.home.file("harnesses");
    assert!(holds(&harness, "[REDACTED]"));
    assert!(!holds(&harness, "mcp-secret-token"));
}
