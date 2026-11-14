//! A stand-in for an agent: the actor `agent/<n>`, which calls the
//! workspace as itself and records what the workspace tells it.
//!
//! ```text
//! demo agent <n> run <call> <script ...>
//!                    run a script in agent <n>'s filesystem, as its
//!                    call <call>
//! demo agent <first>..<last> run <call> <script ...>
//!                    have each agent from <first> to <last> run the
//!                    script at once, agent <k> as its call <call>-<k>
//! demo agent <n> tool <call> <name> <json ...>
//!                    run the tool added by hand <name> with the
//!                    arguments <json>, as agent <n>'s call <call>
//! demo agent <n> read <path>
//!                    read a file as agent <n>, printing its bytes
//! demo agent <n> status <command>
//!                    ask how agent <n>'s command stands
//! demo agent <n> write <call> <path> <content ...>
//!                    write a file for agent <n>, as its call <call>
//! ```
//!
//! Its start makes `actors/agent/<n>.fs/`, the agent's filesystem.
//! `run` prints `done/<call> <exit code>` or `running/<call>
//! <command>`; `status` prints `status/<command>/running 0`,
//! `status/<command>/ended <exit code>` or
//! `status/<command>/cut_off 0`. A refusal prints `refused/<call> 1`
//! and logs why. When the workspace calls the agent with a command's
//! end, the agent keeps it in its `finished` table and prints
//! `finished/<call> <exit code>`. An end with no exit code prints the
//! code 0 after the name of its end, as in `done/<call>/timed_out 0`.
//! An end whose output was cut first prints `cut/<call> <bytes>`, the
//! size of all the command printed, kept in the file the end names.
//! `write` prints `changed/<call> 1` when it made the file and
//! `changed/<call> 0` when it replaced one, or `refused/<call> 1` or
//! `failed/<call> 1` and logs why. A call that waits for a chief
//! prints `waits/<call> <needs>`, how many things it needs. When the
//! wait ends, the agent is called with how the call ended: a command
//! prints `finished/<call> <exit code>` as above, a change
//! `finished/<call>/changed <created>`, `finished/<call>/refused 1`
//! or `finished/<call>/failed 1`, and a call that did not run
//! `finished/<call>/not_run 1`; each logs why.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{
    ActorConfig, ActorFactory, ActorKeySegment, BindParam,
};
use hamlet_actors::workspace;
use hamlet_host::{Address, Caller, Host, HostRef};
use hamlet_messages::agent::{Finished, Outcome};
use hamlet_messages::workspace::files::{
    ChangeAnswer, FileAnswer, ReadFile, WriteFile,
};
use hamlet_messages::workspace::{
    AgentNumber, CallId, CommandEnd, CommandId, CommandStatus,
    ConversationId, Exit, Program, Run, RunAnswer,
};

/// The kind `agent`.
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Agent>(
        workspace::AGENT,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(workspace::AGENT)
        .expect("the registry holds the kind it registered")
}

struct Agent;

/// Asks the agent to call the workspace with `name` and `args`.
#[derive(serde::Serialize, serde::Deserialize)]
struct Ask {
    name: String,
    args: Vec<u8>,
}

impl Action for Ask {
    /// The workspace's answer, as it encoded it.
    type Output = Vec<u8>;
    const NAME: &'static str = "ask";
}

#[async_trait::async_trait]
impl Actor for Agent {
    type Input = ();
    type Actions = (Ask, Finished);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        let Some(ActorKeySegment::String(key)) = ctx.key().first()
        else {
            bail!("an agent's key is its number");
        };
        let folder = HostRef::of(ctx.inner())?
            .folder()
            .join(format!("actors/agent/{key}.fs"));
        std::fs::create_dir_all(folder)?;
        ctx.sql()
            .exec(
                "CREATE TABLE IF NOT EXISTS finished (call TEXT NOT \
                 NULL, command INTEGER NOT NULL, exit TEXT NOT \
                 NULL) STRICT",
            )
            .await?;
        Ok(Self)
    }
}

impl Handler<Ask> for Agent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        ask: Ask,
    ) -> Result<Vec<u8>> {
        HostRef::of(ctx.inner())?
            .call(
                &Address::new(workspace::KIND, workspace::MACHINE)?,
                &ask.name,
                ask.args,
                Duration::from_secs(10),
            )
            .await
    }
}

impl Handler<Finished> for Agent {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        finished: Finished,
    ) -> Result<()> {
        let call = &finished.call.0;
        let (command, word, code) = match &finished.outcome {
            Outcome::Ran(end) => {
                print_cut(call, end)?;
                let (word, code) = exit_word(&end.exit);
                (end.command.0, word, code)
            }
            Outcome::Changed(ChangeAnswer::Changed(change)) => {
                (0, "/changed", i64::from(change.created))
            }
            Outcome::Changed(ChangeAnswer::Refused(why))
            | Outcome::NotRun(why) => {
                tracing::warn!(%why, "agent's call {call} did not change or run");
                let word = match &finished.outcome {
                    Outcome::NotRun(_) => "/not_run",
                    _ => "/refused",
                };
                (0, word, 1)
            }
            Outcome::Changed(ChangeAnswer::Failed(words)) => {
                tracing::warn!(%words, "agent's call {call} failed");
                (0, "/failed", 1)
            }
            Outcome::Changed(ChangeAnswer::WaitsForChief(_)) => {
                bail!("a change ended as waiting")
            }
        };
        ctx.sql()
            .execute(
                "INSERT INTO finished (call, command, exit) VALUES \
                 (?1, ?2, ?3)",
                Some(vec![
                    BindParam::Text(call.clone()),
                    BindParam::Integer(i64::try_from(command)?),
                    BindParam::Text(format!("{word} {code}")),
                ]),
            )
            .await?;
        writeln!(std::io::stdout(), "finished/{call}{word} {code}")?;
        Ok(())
    }
}

/// Runs `demo agent <words>`.
pub async fn run(host: &Host, words: &[String]) -> Result<()> {
    if let [agents, verb, call, script @ ..] = words
        && verb == "run"
        && let Some((first, last)) = agents.split_once("..")
    {
        let mut runs = tokio::task::JoinSet::new();
        for agent in first.parse::<u64>()?..=last.parse()? {
            let host = host.clone();
            let mut words = vec![
                agent.to_string(),
                verb.clone(),
                format!("{call}-{agent}"),
            ];
            words.extend_from_slice(script);
            runs.spawn(async move { run_one(&host, &words).await });
        }
        while let Some(ran) = runs.join_next().await {
            ran??;
        }
        return Ok(());
    }
    run_one(host, words).await
}

async fn run_one(host: &Host, words: &[String]) -> Result<()> {
    let [number, verb, rest @ ..] = words else {
        bail!("usage: demo agent <n> run|status ...");
    };
    let agent = AgentNumber(number.parse()?);
    let conversation = ConversationId("demo".to_owned());
    let (name, args, call) = match (verb.as_str(), rest) {
        ("run", [call, script @ ..]) if !script.is_empty() => {
            let run = Run {
                agent,
                call: CallId(call.clone()),
                conversation,
                realm_task: None,
                program: Program::Script {
                    script: script.join(" "),
                    network: false,
                    timeout_ms: None,
                },
            };
            let args = hamlet_actor::action::encode_positional(&run)?;
            (Run::NAME, args, Some(call))
        }
        ("tool", [call, tool, arguments @ ..]) => {
            let run = Run {
                agent,
                call: CallId(call.clone()),
                conversation,
                realm_task: None,
                program: Program::Tool {
                    name: tool.clone(),
                    arguments: arguments.join(" "),
                },
            };
            let args = hamlet_actor::action::encode_positional(&run)?;
            (Run::NAME, args, Some(call))
        }
        ("read", [path]) => {
            let read = ReadFile {
                agent,
                path: path.into(),
                offset: None,
                limit: None,
            };
            let args =
                hamlet_actor::action::encode_positional(&read)?;
            (ReadFile::NAME, args, None)
        }
        ("status", [command]) => {
            let status = CommandStatus {
                agent,
                command: CommandId(command.parse()?),
            };
            let args =
                hamlet_actor::action::encode_positional(&status)?;
            (CommandStatus::NAME, args, None)
        }
        ("write", [call, path, content @ ..]) => {
            let write = WriteFile {
                agent,
                call: CallId(call.clone()),
                conversation,
                realm_task: None,
                path: path.into(),
                content: content.join(" "),
            };
            let args =
                hamlet_actor::action::encode_positional(&write)?;
            (WriteFile::NAME, args, Some(call))
        }
        _ => bail!(
            "usage: demo agent <n> run <call> <script ...> | tool \
             <call> <name> <json ...> | read <path> | status \
             <command> | write <call> <path> <content ...>"
        ),
    };
    host.get_or_start(&Address::new(
        workspace::KIND,
        workspace::MACHINE,
    )?)
    .await?;
    host.get_or_start(&Address::new(workspace::AGENT, number)?)
        .await?;
    let ask = Ask {
        name: name.to_owned(),
        args,
    };
    let reply = host
        .call(
            &Address::new(workspace::AGENT, number)?,
            Ask::NAME,
            hamlet_actor::action::encode_positional(&ask)?,
            Duration::from_secs(20),
        )
        .await?;
    let reply =
        ciborium::from_reader::<Vec<u8>, _>(reply.as_slice())?;
    let mut out = std::io::stdout();
    if name == ReadFile::NAME {
        // The bytes it read, or 0 and a warning.
        match ciborium::from_reader(reply.as_slice())? {
            FileAnswer::Text(text) => {
                writeln!(out, "read {}", text.text.len())?;
            }
            FileAnswer::Image(image) => {
                writeln!(out, "read {}", image.bytes.len())?;
            }
            FileAnswer::Refused(refusal) => {
                tracing::warn!(%refusal, "the workspace refused agent {number}");
                writeln!(out, "read 0")?;
            }
            FileAnswer::Failed(words) => {
                tracing::warn!(%words, "agent {number}'s read failed");
                writeln!(out, "read 0")?;
            }
        }
        return Ok(());
    }
    if name == WriteFile::NAME {
        let call = call.map_or("", String::as_str);
        match ciborium::from_reader(reply.as_slice())? {
            ChangeAnswer::Changed(change) => writeln!(
                out,
                "changed/{call} {}",
                i64::from(change.created)
            )?,
            ChangeAnswer::Refused(refusal) => {
                tracing::warn!(%refusal, "the workspace refused agent {number}");
                writeln!(out, "refused/{call} 1")?;
            }
            ChangeAnswer::Failed(words) => {
                tracing::warn!(%words, "agent {number}'s write failed");
                writeln!(out, "failed/{call} 1")?;
            }
            ChangeAnswer::WaitsForChief(needs) => {
                writeln!(out, "waits/{call} {}", needs.len())?;
            }
        }
        return Ok(());
    }
    let answer =
        ciborium::from_reader::<RunAnswer, _>(reply.as_slice())?;
    match (answer, call) {
        (RunAnswer::Done(end), Some(call)) => {
            print_cut(call, &end)?;
            let (word, code) = exit_word(&end.exit);
            writeln!(out, "done/{call}{word} {code}")?;
        }
        (RunAnswer::Done(end), None) => {
            let (state, code) = match &end.exit {
                Exit::Code(code) => ("ended", i64::from(*code)),
                Exit::CutOff => ("cut_off", 0),
                Exit::TimedOut => ("timed_out", 0),
                Exit::Failed(_) => ("failed", 0),
                Exit::Cancelled => ("cancelled", 0),
            };
            writeln!(out, "status/{}/{state} {code}", end.command.0)?;
        }
        (RunAnswer::Running { command }, Some(call)) => {
            writeln!(out, "running/{call} {}", command.0)?;
        }
        (RunAnswer::Running { command }, None) => {
            writeln!(out, "status/{}/running 0", command.0)?;
        }
        (RunAnswer::WaitsForChief(needs), call) => {
            let name =
                call.map_or_else(|| rest.join("/"), Clone::clone);
            writeln!(out, "waits/{name} {}", needs.len())?;
        }
        (RunAnswer::Refused(refusal), call) => {
            let name =
                call.map_or_else(|| rest.join("/"), Clone::clone);
            tracing::warn!(%refusal, "the workspace refused agent {number}");
            writeln!(out, "refused/{name} 1")?;
        }
    }
    Ok(())
}

/// The suffix and the code a line prints for `exit`.
fn exit_word(exit: &Exit) -> (&'static str, i64) {
    match exit {
        Exit::Code(code) => ("", i64::from(*code)),
        Exit::TimedOut => ("/timed_out", 0),
        Exit::Failed(_) => ("/failed", 0),
        Exit::CutOff => ("/cut_off", 0),
        Exit::Cancelled => ("/cancelled", 0),
    }
}

/// Prints `cut/<call> <bytes>` when `end`'s output was cut.
fn print_cut(call: &str, end: &CommandEnd) -> Result<()> {
    if let Some(cut) = &end.cut {
        writeln!(
            std::io::stdout(),
            "cut/{call} {}",
            cut.total_bytes
        )?;
    }
    Ok(())
}
