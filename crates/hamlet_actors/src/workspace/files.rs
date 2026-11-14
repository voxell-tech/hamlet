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
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::address::Agent;
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::files::{
    Find, ReadAnswer, ReadFile, Search, SearchAnswer, Text,
};
use hamlet_messages::workspace::reach::{Need, Reach};
use hamlet_messages::workspace::refusal::{InvalidPath, ReadRefusal};
use hamlet_os::fs::path::{is_absolute, text};
use hamlet_sandbox::Env;
use hamlet_sandbox::scrub::scrub_text;

pub(super) use self::change::{EDIT, Paths, WRITE, cut_off_changes};
use super::reach::{self, Gate, Op, Placement, Purpose, Refused};
use super::{Weighing, Workspace, read_only};

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
    async fn run<A: Answered>(
        self,
        work: impl FnOnce(&Env) -> A + Send + 'static,
    ) -> Result<A> {
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
                Err(error) => A::failed(error.to_string()),
            }
        })
        .await
        .context("a file tool's thread ended before it")?;
        Ok(answer.scrubbed(&secrets))
    }
}

/// The answer of a read, a search or a find.
trait Answered: Send + 'static {
    fn failed(words: String) -> Self;
    fn refused(why: ReadRefusal) -> Self;
    /// The answer with `secrets` taken out of every word (C32).
    fn scrubbed(self, secrets: &[String]) -> Self;
}

/// `text` with `secrets` taken out of every word.
fn scrubbed_text(
    secrets: &[String],
    Text {
        text,
        mut notes,
        truncation,
    }: Text,
) -> Text {
    for note in &mut notes {
        note.code = scrub_text(secrets, &note.code);
        note.message = scrub_text(secrets, &note.message);
    }
    Text {
        text: scrub_text(secrets, &text),
        notes,
        truncation,
    }
}

impl Answered for ReadAnswer {
    fn failed(words: String) -> Self {
        Self::Failed { words }
    }

    fn refused(why: ReadRefusal) -> Self {
        Self::Refused { why }
    }

    fn scrubbed(self, secrets: &[String]) -> Self {
        match self {
            Self::Text(text) => {
                Self::Text(scrubbed_text(secrets, text))
            }
            Self::Failed { words } => Self::Failed {
                words: scrub_text(secrets, &words),
            },
            image @ (Self::Image(_) | Self::Refused { .. }) => image,
        }
    }
}

impl Answered for SearchAnswer {
    fn failed(words: String) -> Self {
        Self::Failed { words }
    }

    fn refused(why: ReadRefusal) -> Self {
        Self::Refused { why }
    }

    fn scrubbed(self, secrets: &[String]) -> Self {
        match self {
            Self::Text(text) => {
                Self::Text(scrubbed_text(secrets, text))
            }
            Self::Failed { words } => Self::Failed {
                words: scrub_text(secrets, &words),
            },
            refused @ Self::Refused { .. } => refused,
        }
    }
}

enum Gated {
    Pass(Passed),
    /// It waits for a chief to allow these.
    Chief(Passed, Vec<Need>),
    Refused(Refused),
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
    ) -> Result<Weighing<Gated>> {
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
        Ok(Weighing::Done(
            match reach::gate(&self.home, agent, reach, held, op) {
                Gate::Run { .. } => Gated::Pass(passed()?),
                Gate::NeedsChief(needs) => {
                    Gated::Chief(passed()?, needs)
                }
                Gate::Refuse(refused) => Gated::Refused(refused),
                Gate::NeedsSecrets(version) => {
                    return Ok(Weighing::NeedsSecrets(version));
                }
            },
        ))
    }

    async fn read_tool<A: Answered>(
        &self,
        ctx: &Ctx<Self>,
        agent: AgentNumber,
        path: &Path,
        work: impl FnOnce(&Env) -> A + Send + 'static,
    ) -> Result<A> {
        if !is_absolute(&text(path)) {
            return Ok(A::refused(
                InvalidPath::NotAbsolute {
                    path: path.to_owned(),
                }
                .into(),
            ));
        }
        let gated = self
            .weighed(ctx, move || async move {
                let reach = read_only(ctx, async |tx| {
                    reach::read(tx, agent).await
                })
                .await?;
                self.gated(agent, reach.as_ref(), &Op::Read { path })
            })
            .await?;
        match gated {
            Gated::Pass(reading) => reading.run(work).await,
            Gated::Chief(..) => {
                anyhow::bail!("the gate asked a chief for a read")
            }
            Gated::Refused(refused) => {
                Ok(A::refused(refused.of_read()?))
            }
        }
    }
}

impl Handler<ReadFile> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: ReadFile,
    ) -> Result<ReadAnswer> {
        let Agent(agent) = sender(&ctx, &asked)?;
        let path = text(&asked.path);
        self.read_tool(&ctx, agent, &asked.path, move |env| {
            read::read(env, &path, asked.offset, asked.limit)
                .unwrap_or_else(|error| ReadAnswer::Failed {
                    words: error.message,
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
    ) -> Result<SearchAnswer> {
        let Agent(agent) = sender(&ctx, &asked)?;
        let under = text(&asked.under);
        self.read_tool(&ctx, agent, &asked.under, move |env| {
            listed(walk::search(
                env,
                &under,
                &asked.pattern,
                asked.name.as_deref(),
                if asked.ignore_case {
                    walk::Case::Ignored
                } else {
                    walk::Case::Matched
                },
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
    ) -> Result<SearchAnswer> {
        let Agent(agent) = sender(&ctx, &asked)?;
        let under = text(&asked.under);
        self.read_tool(&ctx, agent, &asked.under, move |env| {
            listed(walk::find(env, &under, asked.name.as_deref()))
        })
        .await
    }
}

/// The answer of a search or a find.
fn listed(found: Result<String, walk::Failure>) -> SearchAnswer {
    match found {
        Ok(text) => SearchAnswer::Text(Text {
            text,
            notes: Vec::new(),
            truncation: None,
        }),
        Err(failure) => SearchAnswer::Failed {
            words: failure.words(),
        },
    }
}
