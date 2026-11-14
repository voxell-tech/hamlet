//! An agent's file tools on its project files: each call passes the
//! gate with the agent's reach, runs under the sandbox's policy for
//! file tools, and hands back only what the realm's secrets were
//! taken out of. A read, a search and a find leave no row; a write
//! and an edit leave one, which makes a repeat safe.

mod change;
mod diff;
mod edit;
mod read;
mod walk;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_messages::workspace::files::{
    FileAnswer, Find, ReadFile, Search, Text,
};
use hamlet_messages::workspace::reach::{
    Need, Reach, SecretsVersion,
};
use hamlet_messages::workspace::{AgentNumber, CallRefusal};
use hamlet_os::fs::path::{is_absolute, text};
use hamlet_sandbox::Env;
use hamlet_sandbox::scrub::scrub_text;

pub(super) use self::change::{Paths, cut_off_changes};
use super::reach::{self, Gate, Op, Placement, Purpose};
use super::{Workspace, calling_agent};

/// A file tool's call the gate let through: where it runs, and the
/// secrets its answer is scrubbed of.
struct Passed {
    placement: Placement,
    sandbox: hamlet_sandbox::Sandbox,
    secrets: Arc<Vec<String>>,
}

impl Passed {
    /// The answer of `work`, run off the runtime on the file tools'
    /// environment, with the secrets taken out.
    async fn run(
        self,
        work: impl FnOnce(&Env) -> FileAnswer + Send + 'static,
    ) -> Result<FileAnswer> {
        let Self {
            placement,
            sandbox,
            secrets,
        } = self;
        let answer = tokio::task::spawn_blocking(move || {
            match Env::new(
                &text(&placement.cwd),
                placement.policy,
                sandbox,
            ) {
                Ok(env) => work(&env),
                Err(error) => FileAnswer::Failed(error.to_string()),
            }
        })
        .await
        .context("a file tool's thread ended before it")?;
        Ok(scrubbed(&secrets, answer))
    }
}

/// `answer` with `secrets` taken out of every word (C32).
fn scrubbed(secrets: &[String], answer: FileAnswer) -> FileAnswer {
    let scrub = |words: &str| scrub_text(secrets, words);
    match answer {
        FileAnswer::Text(Text {
            text,
            mut notes,
            truncation,
        }) => {
            for note in &mut notes {
                note.code = scrub(&note.code);
                note.message = scrub(&note.message);
            }
            FileAnswer::Text(Text {
                text: scrub(&text),
                notes,
                truncation,
            })
        }
        FileAnswer::Failed(words) => {
            FileAnswer::Failed(scrub(&words))
        }
        image @ (FileAnswer::Image(_) | FileAnswer::Refused(_)) => {
            image
        }
    }
}

/// What the gate makes of a file tool's call.
enum Gated {
    Pass(Passed),
    /// It waits for a chief to allow these.
    Chief(Passed, Vec<Need>),
    Refused(CallRefusal),
    /// The secrets held are older than this version.
    Fetch(SecretsVersion),
}

impl Workspace {
    /// The gate's verdict on the file tool's `op` of `agent` under
    /// `reach`, with the placement and the secrets of a call it lets
    /// through.
    fn gated(
        &self,
        agent: AgentNumber,
        reach: Option<&Reach>,
        op: &Op<'_>,
    ) -> Result<Gated> {
        let (held, secrets) = self.held();
        let passed = || -> Result<Passed> {
            Ok(Passed {
                placement: self.placement(
                    agent,
                    reach.context(
                        "the gate passed a file tool with no reach",
                    )?,
                    Purpose::FileTool,
                )?,
                sandbox: self.sandbox,
                secrets: secrets.clone(),
            })
        };
        Ok(match reach::gate(&self.home, agent, reach, held, op) {
            Gate::Run { .. } => Gated::Pass(passed()?),
            Gate::NeedsChief(needs) => Gated::Chief(passed()?, needs),
            Gate::Refuse(refusal) => {
                Gated::Refused(CallRefusal::Reach(refusal))
            }
            Gate::NeedsSecrets(version) => Gated::Fetch(version),
        })
    }

    /// Passes a read of `path` for the agent `named` through the
    /// gate, as the caller the host stamped, and answers what `work`
    /// reads, or why it is refused.
    async fn read_tool(
        &self,
        ctx: &Ctx<Self>,
        named: AgentNumber,
        path: &Path,
        action: &str,
        work: impl FnOnce(&Env) -> FileAnswer + Send + 'static,
    ) -> Result<FileAnswer> {
        let agent = match calling_agent(ctx, named, action)? {
            Ok(agent) => agent,
            Err(refusal) => return Ok(FileAnswer::Refused(refusal)),
        };
        if !is_absolute(&text(path)) {
            return Ok(FileAnswer::Refused(CallRefusal::Invalid(
                format!("{} is not an absolute path", path.display()),
            )));
        }
        let mut fetched = false;
        loop {
            let reach = super::read_only(ctx, async |tx| {
                reach::read(tx, agent).await
            })
            .await?;
            match self.gated(
                agent,
                reach.as_ref(),
                &Op::Read { path },
            )? {
                Gated::Pass(reading) => {
                    return reading.run(work).await;
                }
                Gated::Chief(..) => {
                    anyhow::bail!("the gate asked a chief for a read")
                }
                Gated::Refused(refusal) => {
                    return Ok(FileAnswer::Refused(refusal));
                }
                Gated::Fetch(version) => {
                    self.fetch_secrets(ctx, version, &mut fetched)
                        .await?;
                }
            }
        }
    }
}

impl Handler<ReadFile> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: ReadFile,
    ) -> Result<FileAnswer> {
        let path = text(&asked.path);
        self.read_tool(
            &ctx,
            asked.agent,
            &asked.path,
            ReadFile::NAME,
            move |env| {
                read::read(env, &path, asked.offset, asked.limit)
                    .unwrap_or_else(|error| {
                        FileAnswer::Failed(error.message)
                    })
            },
        )
        .await
    }
}

impl Handler<Search> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Search,
    ) -> Result<FileAnswer> {
        let under = text(&asked.under);
        self.read_tool(
            &ctx,
            asked.agent,
            &asked.under,
            Search::NAME,
            move |env| {
                listed(walk::search(
                    env,
                    &under,
                    &asked.pattern,
                    asked.name.as_deref(),
                    asked.ignore_case,
                ))
            },
        )
        .await
    }
}

impl Handler<Find> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Find,
    ) -> Result<FileAnswer> {
        let under = text(&asked.under);
        self.read_tool(
            &ctx,
            asked.agent,
            &asked.under,
            Find::NAME,
            move |env| {
                listed(walk::find(env, &under, asked.name.as_deref()))
            },
        )
        .await
    }
}

/// The answer of a search or a find.
fn listed(found: Result<String, walk::Failure>) -> FileAnswer {
    match found {
        Ok(text) => FileAnswer::Text(Text {
            text,
            notes: Vec::new(),
            truncation: None,
        }),
        Err(failure) => FileAnswer::Failed(failure.words()),
    }
}
