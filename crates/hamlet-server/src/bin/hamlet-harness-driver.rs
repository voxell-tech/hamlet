//! `hamlet-harness-driver --home <dir> [--faux <file>]`: holds a home
//! and its runtimes for the harness tests, taking commands on
//! standard input in place of protocol v0 and printing each notice as
//! a JSON line. Built only with the `harness` feature.
//!
//! `HAMLET_ASK_TOOLS` names the tools whose new calls wait for a
//! chief, and `HAMLET_UNSAFE_TOOLS` those not run again after a
//! cut-off call, each as a comma-separated list.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use hamlet_server::harness::link::ToolSpecReplay;
use hamlet_server::harness::{BindingId, Config, Harnesses, Notice};
use hamlet_server::record::AgentRef;
use hamlet_server::{Home, RuntimeInstance};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut home = None;
    let mut faux = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().context("a flag needs a value")?;
        match flag.as_str() {
            "--home" => home = Some(PathBuf::from(value)),
            "--faux" => faux = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {flag}"),
        }
    }
    let home = home.context("--home is needed")?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(drive(home, faux))
}

fn listed(name: &str, tool: &str) -> bool {
    std::env::var(name).is_ok_and(|tools| {
        tools.split(',').any(|listed| listed == tool)
    })
}

async fn drive(dir: PathBuf, faux: Option<PathBuf>) -> Result<()> {
    let mut home = Home::open(&dir)?;
    let layout = home.layout().clone();
    let mut config = Config::from_env(faux)?;
    config.asks = |tool| listed("HAMLET_ASK_TOOLS", tool);
    for tool in &mut config.tools {
        if listed("HAMLET_UNSAFE_TOOLS", &tool.name) {
            tool.replay = ToolSpecReplay::Unsafe;
        }
    }
    let (mut harnesses, notices) =
        Harnesses::start(&layout, home.store(), config)?;
    print(&notices);
    println!("{}", json!({"ready": true}));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            event = harnesses.next() => {
                let notices = harnesses.handle(home.store(), event).await?;
                print(&notices);
            }
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let reply = match command(&mut harnesses, &mut home, &line).await {
                    Ok(reply) => reply,
                    Err(error) => json!({"error": format!("{error:#}")}),
                };
                println!("{reply}");
            }
        }
    }
    harnesses.close().await;
    home.close()
}

async fn command(
    harnesses: &mut Harnesses,
    home: &mut Home,
    line: &str,
) -> Result<Value> {
    let command = serde_json::from_str::<Value>(line)?;
    let text = |name: &str| {
        command[name]
            .as_str()
            .map(str::to_owned)
            .with_context(|| format!("{name} is needed"))
    };
    let binding = || {
        command["binding"]
            .as_i64()
            .map(BindingId)
            .context("binding is needed")
    };
    match command["do"].as_str().unwrap_or_default() {
        "bind" => {
            let agent = AgentRef {
                hamlet: text("hamlet")?,
                number: command["number"]
                    .as_u64()
                    .context("number")?,
            };
            let errand = command["errand"].as_bool().unwrap_or(false);
            let (binding, notices) =
                harnesses.bind(home.store(), &agent, errand).await?;
            print(&notices);
            let bound =
                home.store().binding(binding)?.context("bound")?;
            Ok(
                json!({"bound": binding.0, "runtime": bound.runtime.as_str()}),
            )
        }
        "submit" => {
            harnesses
                .submit(
                    home.store(),
                    binding()?,
                    &text("request")?,
                    &text("content")?,
                    command["steer"].as_bool().unwrap_or(false),
                )
                .await?;
            Ok(json!({"kept": text("request")?}))
        }
        "decide" => {
            let runtime = RuntimeInstance::new(text("runtime")?)?;
            let decided = harnesses
                .decide(
                    home.store(),
                    &runtime,
                    &text("key")?,
                    command["allow"].as_bool().context("allow")?,
                )
                .await?;
            Ok(json!({"decided": format!("{decided:?}")}))
        }
        "cancel" => {
            harnesses.cancel(home.store(), binding()?).await?;
            Ok(json!({"cancelling": binding()?.0}))
        }
        "history" => {
            harnesses
                .history(
                    home.store(),
                    binding()?,
                    command["before"].as_str().map(str::to_owned),
                    command["limit"].as_i64().unwrap_or(100),
                )
                .await?;
            Ok(json!({"asked": binding()?.0}))
        }
        other => bail!("unknown command {other:?}"),
    }
}

fn print(notices: &[Notice]) {
    for notice in notices {
        println!("{}", json!({"notice": notice_json(notice)}));
    }
}

fn notice_json(notice: &Notice) -> Value {
    match notice {
        Notice::Provider(_) => json!({"kind":"provider"}),
        Notice::Started { runtime, pid } => {
            json!({"kind": "started", "runtime": runtime.as_str(), "pid": pid})
        }
        Notice::Ready { runtime } => {
            json!({"kind": "ready", "runtime": runtime.as_str()})
        }
        Notice::Refused { runtime, words } => {
            json!({"kind": "refused", "runtime": runtime.as_str(), "words": words})
        }
        Notice::Exited {
            runtime,
            status,
            restart_in,
        } => json!({
            "kind": "exited",
            "runtime": runtime.as_str(),
            "status": status,
            "restart_ms": restart_in.map(|wait| wait.as_millis() as u64),
        }),
        Notice::Created { binding } => {
            json!({"kind": "created", "binding": binding.0})
        }
        Notice::Admitted { binding, request } => {
            json!({"kind": "admitted", "binding": binding.0, "request": request})
        }
        Notice::Kept { binding, item } => {
            json!({"kind": "kept", "binding": binding.0, "item": item})
        }
        Notice::Running { binding, running } => {
            json!({"kind": "running", "binding": binding.0, "running": running})
        }
        Notice::Waiting { binding, key, tool } => json!({
            "kind": "waiting", "binding": binding.0, "key": key, "tool": tool,
        }),
        Notice::Cancelled { binding } => {
            json!({"kind": "cancelled", "binding": binding.0})
        }
        Notice::Page {
            binding,
            items,
            earlier,
        } => json!({
            "kind": "page", "binding": binding.0, "items": items, "earlier": earlier,
        }),
        Notice::Fault { runtime, words } => {
            json!({"kind": "fault", "runtime": runtime.as_str(), "words": words})
        }
    }
}
