//! An MCP server listed in the realm: run under the sandbox in its
//! own folder, its tools offered and called, a tool that acts waiting
//! for a chief, its secrets kept out of what the model is given.

use std::path::Path;
use std::time::Duration;

use hamlet_acceptance::*;
use hamlet_protocol::client::{
    Choice, Lasting, WaitDanger, WaitDetail,
};
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
async fn an_mcp_tool_runs_on_its_server_once_allowed_and_gives_its_result_without_secrets()
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
                    ("mcp__echo__token", json!({})),
                    ("mcp__echo__echo", json!({"text": "hello"})),
                ]),
                call("mcp__echo__echo", json!({"text": "again"})),
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
    // A tool its server does not say only reads waits for a chief,
    // and once allowed for the conversation asks no more.
    let asked = s
        .chief
        .wait_for("the echo", |w| {
            w.subject.contains("mcp__echo__echo")
        })
        .await;
    let WaitDetail::Approval { danger, always, .. } = &asked.detail
    else {
        panic!("an approval: {asked:?}");
    };
    assert_eq!(*danger, WaitDanger::Act);
    assert_eq!(*always, Some(Lasting::Conversation));
    let echoed = s.home.file("mcp/echo/echoed");
    assert!(!echoed.exists());
    s.chief.choose(&asked.id, Choice::Always, None).await;
    s.chief.shows(&now, "Called.").await;
    assert!(s.chief.waits().is_empty());
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
            ("mcp__echo__token".to_owned(), None),
            ("mcp__echo__echo".to_owned(), None),
            ("mcp__echo__echo".to_owned(), None),
        ]
    );
    assert_eq!(std::fs::read_to_string(echoed).unwrap(), "again");
    // The harness keeps what its model was given.
    let harness = s.home.file("harnesses");
    assert!(holds(&harness, "[REDACTED]"));
    assert!(!holds(&harness, "mcp-secret-token"));
}

#[tokio::test]
async fn a_server_an_agent_asks_for_is_listed_once_a_chief_allows_it()
{
    let program = Path::new(env!("CARGO_BIN_EXE_mcp_echo"));
    let mut s = Session::start(Faux::new().agent(
        2,
        [
            call(
                "request_mcp_server",
                json!({
                    "name": "echo",
                    "command": [program],
                    "env": ["TOKEN"],
                    "fs_ro": [program.parent().unwrap()],
                    "reason": "Echo text back.",
                }),
            ),
            words("Asked."),
        ],
    ))
    .await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Get an echo server.").await;
    let asked = s
        .chief
        .wait_for("the request", |w| {
            matches!(w.detail, WaitDetail::McpServer { .. })
        })
        .await;
    let WaitDetail::McpServer { name, entry } = &asked.detail else {
        unreachable!();
    };
    assert_eq!(name, "echo");
    assert_eq!(entry.env.keys().collect::<Vec<_>>(), ["TOKEN"]);
    s.chief.choose(&asked.id, Choice::Allow, None).await;
    // Listed with the value for the chief to give, and started.
    let list =
        std::fs::read_to_string(s.home.file("mcp/servers.json"))
            .unwrap();
    let list =
        serde_json::from_str::<serde_json::Value>(&list).unwrap();
    assert_eq!(list["servers"]["echo"]["env"]["TOKEN"], "");
    let listed = s.home.file("mcp/echo/listed");
    s.chief
        .until("the server listed", |_| listed.exists())
        .await;
    s.chief
        .until("the agent told", |c| {
            c.texts(&now).iter().any(|text| {
                text.contains("The chief added the MCP server echo")
            })
        })
        .await;
}
