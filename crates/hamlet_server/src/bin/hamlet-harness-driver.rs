//! `hamlet-harness-driver --home <dir> [--faux <file>]`: holds a home
//! and its runtimes for the harness tests, taking commands on
//! standard input in place of the client protocol and printing each
//! notice as a JSON line. Built only with the `harness` feature.
//!
//! Tool calls go through the chats' gate, in conversations of the
//! chief. `HAMLET_UNSAFE_TOOLS` names the tools not run again after a
//! cut-off call, as a comma-separated list.

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use hamlet_server::harness::runtime::{Section, ToolSpecReplay};
use hamlet_server::harness::{
    Answer, Binding, BindingId, Call, Config, Harnesses, Input,
    Notice,
};
use hamlet_server::record::AgentRef;
use hamlet_server::service::Service;
use hamlet_server::{Credentials, Home, RuntimeInstance, Store};
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

/// The driver's state: the served home, its runtimes and the waits
/// already told.
struct Driver {
    service: Service,
    harnesses: Harnesses,
    told: HashSet<String>,
}

async fn drive(dir: PathBuf, faux: Option<PathBuf>) -> Result<()> {
    let mut credentials = Credentials::open(&dir)?;
    let mut service =
        Service::new(Home::open(&dir)?, faux.is_some())?;
    let layout = service.home.layout().clone();
    let mut config = Config::from_env(faux)?;
    for tool in &mut config.tools {
        if listed("HAMLET_UNSAFE_TOOLS", &tool.name) {
            tool.replay = ToolSpecReplay::Unsafe;
        }
    }
    let (harnesses, notices) =
        Harnesses::start(&layout, service.home.store(), config)?;
    let mut driver = Driver {
        service,
        harnesses,
        told: HashSet::new(),
    };
    driver.answer(notices).await?;
    println!("{}", json!({"ready": true}));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            event = driver.harnesses.next() => {
                let notices = driver.harnesses.handle(driver.service.home.store(), event, &credentials).await?;
                driver.harnesses.credentials(driver.service.home.store(), &mut credentials).await?;
                driver.answer(notices).await?;
            }
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let reply = match driver.command(&line).await {
                    Ok(reply) => reply,
                    Err(error) => json!({"error": format!("{error:#}")}),
                };
                println!("{reply}");
            }
        }
    }
    driver.harnesses.close().await;
    driver.service.home.close()
}

impl Driver {
    /// Prints `notices`, answering each tool call through the gate
    /// and the rest as a host with no prompt of its own.
    async fn answer(&mut self, notices: Vec<Notice>) -> Result<()> {
        print(&notices);
        for notice in notices {
            let Notice::Call { binding, re, call } = notice else {
                continue;
            };
            let store = self.service.home.store();
            let bound = store
                .binding(binding)?
                .context("a call's binding")?;
            let answer = match call {
                Call::Allow {
                    ref key, ref tool, ..
                }
                | Call::Operation {
                    ref key, ref tool, ..
                } => {
                    let (key, tool) = (key.clone(), tool.clone());
                    self.service
                        .answer_call(
                            &mut self.harnesses,
                            binding,
                            re,
                            call,
                        )
                        .await?;
                    if let Some(wait) =
                        self.service.wait_of(&bound.runtime, &key)?
                        && self.told.insert(wait.clone())
                    {
                        println!(
                            "{}",
                            json!({"notice": {"kind": "waiting", "binding": binding.0, "key": key, "tool": tool, "wait": wait}})
                        );
                    }
                    continue;
                }
                Call::Prompt { .. } => {
                    Answer::Sections(prompt(store, &bound)?)
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
            self.harnesses.answer(re, answer).await;
        }
        Ok(())
    }

    async fn command(&mut self, line: &str) -> Result<Value> {
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
                let (binding, notices) = self
                    .service
                    .bind_chat(&mut self.harnesses, &agent)
                    .await?;
                self.answer(notices).await?;
                let store = self.service.home.store();
                let bound =
                    store.binding(binding)?.context("bound")?;
                Ok(
                    json!({"bound": binding.0, "runtime": bound.runtime.as_str()}),
                )
            }
            "submit" => {
                let request = text("request")?;
                let content = text("content")?;
                self.harnesses
                    .submit(
                        self.service.home.store(),
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
                let choice =
                    if command["allow"].as_bool().context("allow")? {
                        hamlet_protocol::Choice::Allow
                    } else {
                        hamlet_protocol::Choice::Deny
                    };
                let decided = match self
                    .service
                    .wait_of(&runtime, &text("key")?)?
                {
                    Some(wait) => self
                        .service
                        .answer_wait(
                            &mut self.harnesses,
                            &wait,
                            &choice,
                        )
                        .await?
                        .map_err(anyhow::Error::msg)?,
                    None => "Nothing waits for you.".into(),
                };
                Ok(json!({"decided": decided}))
            }
            "cancel" => {
                self.service
                    .stop_chat(&mut self.harnesses, binding()?)
                    .await?;
                Ok(json!({"cancelling": binding()?.0}))
            }
            "history" => {
                self.harnesses
                    .history(
                        self.service.home.store(),
                        binding()?,
                        command["before"].as_str().map(str::to_owned),
                        command["limit"].as_u64().unwrap_or(100),
                    )
                    .await?;
                Ok(json!({"asked": binding()?.0}))
            }
            other => bail!("unknown command {other:?}"),
        }
    }
}

/// The prompt sections of the conversation of `binding`: who it is
/// with.
fn prompt(store: &Store, binding: &Binding) -> Result<Vec<Section>> {
    let place = &binding.agent;
    let Some(record) = store.record(&place.hamlet)? else {
        return Ok(Vec::new());
    };
    let Some(agent) = record
        .agents
        .iter()
        .find(|agent| agent.number == place.number)
    else {
        return Ok(Vec::new());
    };
    let role = if record.mayor == place.number {
        "the mayor of"
    } else {
        "an agent of"
    };
    Ok(vec![Section {
        name: "identity".into(),
        text: format!(
            "You are {}, {role} the hamlet {}.",
            agent.name, record.id
        ),
    }])
}

fn print(notices: &[Notice]) {
    for notice in notices {
        println!("{}", json!({"notice": notice_json(notice)}));
    }
}

fn notice_json(notice: &Notice) -> Value {
    match notice {
        Notice::Provider(_) => json!({"kind":"provider"}),
        Notice::ProviderEvent(_) => json!({"kind":"provider_event"}),
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
