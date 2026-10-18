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
use hamlet_server::harness::{
    Answer, BindingId, Call, Config, Harnesses, Input, Notice,
    answers,
};
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
    for tool in &mut config.tools {
        if listed("HAMLET_UNSAFE_TOOLS", &tool.name) {
            tool.replay = ToolSpecReplay::Unsafe;
        }
    }
    let (mut harnesses, notices) =
        Harnesses::start(&layout, home.store(), config)?;
    answer(&mut harnesses, &mut home, notices).await?;
    println!("{}", json!({"ready": true}));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            event = harnesses.next() => {
                let notices = harnesses.handle(home.store(), event).await?;
                answer(&mut harnesses, &mut home, notices).await?;
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

/// Prints `notices`, answering each call as a host with no gate of
/// its own: the tools `HAMLET_ASK_TOOLS` names wait for a chief.
async fn answer(
    harnesses: &mut Harnesses,
    home: &mut Home,
    notices: Vec<Notice>,
) -> Result<()> {
    print(&notices);
    let asks = |tool: &str| listed("HAMLET_ASK_TOOLS", tool);
    for notice in notices {
        let Notice::Call { binding, re, call } = notice else {
            continue;
        };
        let store = home.store();
        let bound =
            store.binding(binding)?.context("a call's binding")?;
        let answer = match call {
            Call::Allow {
                key,
                tool,
                arguments,
                ..
            } => {
                let asked = answers::allow(
                    store, &bound, &key, &tool, &arguments, asks,
                )?;
                if asked.new_wait {
                    println!(
                        "{}",
                        json!({"notice": {"kind": "waiting", "binding": binding.0, "key": key, "tool": tool}})
                    );
                }
                asked.answer
            }
            Call::Operation {
                key,
                tool,
                arguments,
                ..
            } => {
                let at = chrono::Utc::now().timestamp_millis();
                let asked = answers::operation(
                    store, &bound, &key, &tool, &arguments, asks, at,
                )?;
                if asked.new_wait {
                    println!(
                        "{}",
                        json!({"notice": {"kind": "waiting", "binding": binding.0, "key": key, "tool": tool}})
                    );
                }
                asked.answer
            }
            Call::Prompt { .. } => {
                Answer::Sections(answers::prompt(store, &bound)?)
            }
            Call::Yield {
                answer,
                history,
                retried,
                summary_failure,
                ..
            } => Answer::Verdict {
                words: None,
                notice: None,
                compact: answer.stop.as_deref() == Some("length")
                    && history
                    && !retried
                    && summary_failure.is_none(),
            },
        };
        harnesses.answer(re, answer).await;
    }
    Ok(())
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
            answer(harnesses, home, notices).await?;
            let bound =
                home.store().binding(binding)?.context("bound")?;
            Ok(
                json!({"bound": binding.0, "runtime": bound.runtime.as_str()}),
            )
        }
        "submit" => {
            let request = text("request")?;
            let content = text("content")?;
            harnesses
                .submit(
                    home.store(),
                    binding()?,
                    &Input {
                        request: &request,
                        content: &content,
                        steer: command["steer"]
                            .as_bool()
                            .unwrap_or(false),
                        mark: None,
                        write: false,
                        boundary: false,
                    },
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
        Notice::Admitted {
            binding, request, ..
        } => {
            json!({"kind": "admitted", "binding": binding.0, "request": request})
        }
        Notice::Settled {
            binding, request, ..
        } => {
            json!({"kind": "settled", "binding": binding.0, "request": request})
        }
        Notice::Kept { binding, item } => {
            json!({"kind": "kept", "binding": binding.0, "item": item})
        }
        Notice::Live {
            binding, running, ..
        } => {
            json!({"kind": "running", "binding": binding.0, "running": running})
        }
        Notice::Attempted { binding } => {
            json!({"kind": "attempted", "binding": binding.0})
        }
        Notice::Call { binding, call, .. } => {
            json!({"kind": "call", "binding": binding.0, "call": format!("{call:?}")})
        }
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
