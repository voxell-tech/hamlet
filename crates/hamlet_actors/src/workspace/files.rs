//! An agent's file tools on its project files: each call passes the
//! gate with the agent's reach, runs under the sandbox's policy for
//! file tools, and hands back only what the realm's secrets were
//! taken out of. A read, a search and a find leave no row.

mod read;
mod walk;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_messages::workspace::files::{
    FileAnswer, Find, ReadFile, Search, Text,
};
use hamlet_messages::workspace::{AgentNumber, CallRefusal};
use hamlet_os::fs::path::{is_absolute, text};
use hamlet_sandbox::Env;
use hamlet_sandbox::scrub::scrub_text;

use super::reach::{self, Gate, Op, Placement, Purpose};
use super::{Workspace, agent_of, caller, refused};

/// A read the gate let through: where it runs, and the secrets its
/// answer is scrubbed of.
struct Reading {
    placement: Placement,
    sandbox: hamlet_sandbox::Sandbox,
    secrets: Arc<Vec<String>>,
}

impl Reading {
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

/// `answer` with `secrets` taken out of every word, as from every
/// tool's result today (C32).
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

impl Workspace {
    /// Passes a read of `path` for the agent `named` through the
    /// gate, as the caller the host stamped, or why it is refused.
    async fn reading(
        &self,
        ctx: &Ctx<Self>,
        named: AgentNumber,
        path: &Path,
        action: &str,
    ) -> Result<Result<Reading, CallRefusal>> {
        let caller = caller(ctx)?;
        let agent = match agent_of(caller.as_ref(), named) {
            Ok(agent) => agent,
            Err(refusal) => {
                refused(caller.as_ref(), action, &refusal);
                return Ok(Err(CallRefusal::Caller(refusal)));
            }
        };
        if !is_absolute(&text(path)) {
            return Ok(Err(CallRefusal::Invalid(format!(
                "{} is not an absolute path",
                path.display()
            ))));
        }
        let mut fetched = false;
        loop {
            let tx = ctx.sql().begin_transaction(None).await?;
            let reach = reach::read(&tx, agent).await;
            tx.rollback().await?;
            let reach = reach?;
            let (held, secrets) = self.held();
            let op = Op::Read { path };
            match reach::gate(
                &self.home,
                agent,
                reach.as_ref(),
                held,
                &op,
            ) {
                Gate::Run { .. } => {
                    let placement = reach
                        .as_ref()
                        .and_then(|reach| {
                            reach::placement(
                                &self.home,
                                agent,
                                reach,
                                Purpose::FileTool,
                            )
                        })
                        .context(
                            "the gate passed a read with no reach",
                        )?;
                    return Ok(Ok(Reading {
                        placement,
                        sandbox: self.sandbox,
                        secrets,
                    }));
                }
                Gate::Refuse(refusal) => {
                    return Ok(Err(CallRefusal::Reach(refusal)));
                }
                Gate::NeedsChief(needs) => {
                    return Ok(Err(CallRefusal::NeedsChief(needs)));
                }
                Gate::NeedsSecrets(version) => {
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
        let reading = match self
            .reading(&ctx, asked.agent, &asked.path, ReadFile::NAME)
            .await?
        {
            Ok(reading) => reading,
            Err(refusal) => return Ok(FileAnswer::Refused(refusal)),
        };
        let path = text(&asked.path);
        reading
            .run(move |env| {
                read::read(env, &path, asked.offset, asked.limit)
                    .unwrap_or_else(|error| {
                        FileAnswer::Failed(error.message)
                    })
            })
            .await
    }
}

impl Handler<Search> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Search,
    ) -> Result<FileAnswer> {
        let reading = match self
            .reading(&ctx, asked.agent, &asked.under, Search::NAME)
            .await?
        {
            Ok(reading) => reading,
            Err(refusal) => return Ok(FileAnswer::Refused(refusal)),
        };
        reading
            .run(move |env| {
                listed(walk::search(
                    env,
                    &text(&asked.under),
                    &asked.pattern,
                    asked.name.as_deref(),
                    asked.ignore_case,
                ))
            })
            .await
    }
}

impl Handler<Find> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Find,
    ) -> Result<FileAnswer> {
        let reading = match self
            .reading(&ctx, asked.agent, &asked.under, Find::NAME)
            .await?
        {
            Ok(reading) => reading,
            Err(refusal) => return Ok(FileAnswer::Refused(refusal)),
        };
        reading
            .run(move |env| {
                listed(walk::find(
                    env,
                    &text(&asked.under),
                    asked.name.as_deref(),
                ))
            })
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
