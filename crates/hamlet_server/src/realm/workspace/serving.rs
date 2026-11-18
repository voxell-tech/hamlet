//! The realm working in a remote workspace: each agent's session
//! there, its worktree's remote copy kept in step by tasks of their
//! own, and every worktree moved there at a chief's word. No handler
//! awaits the workspace: a push, a pull or a move runs as a task.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use bevy_log::warn;
use hamlet_protocol::client::{
    RequestId, SettingsViewWorkspaceMove,
    SettingsViewWorkspaceMoveState,
};
use hamlet_sandbox::scrub::scrub_text;
use hamlet_store::Db;
use hamlet_store::ids::AgentId;

use super::worktrees::{self, Synced};
use super::{Policy, Session, Transport, workspace_of};
use crate::app::tasks::BackgroundResult;
use crate::clients::check::reply;
use crate::core::{Core, RemoteWorkspace, UnpushedWorktree};
use crate::knowledge;
use crate::knowledge::worker::RepositoryWriteLock;
use crate::realm::Realm;
use crate::realm::jobs::Job;
use crate::society::refusal::Refusal;
use crate::society::{agents, hamlet_agents, hamlets};
use crate::tools::builtin::Network;
use crate::tools::hamlet_of;

/// A worktree's remote copy: what both copies held alike when last in
/// step, or nothing before its first push. The tasks that push to it
/// and pull from it take it one at a time.
#[derive(Clone, Default)]
pub(crate) struct RemoteCopy(Arc<tokio::sync::Mutex<Option<Synced>>>);

/// The parts a task needs to keep one worktree's remote copy in step.
#[derive(Clone)]
pub(crate) struct RemoteWorktree {
    session: Session,
    path: PathBuf,
    copy: RemoteCopy,
    writing: RepositoryWriteLock,
}

impl RemoteWorktree {
    /// Sends the remote copy what the server's worktree changed since
    /// they were last in step, or all of it when they never were.
    pub async fn push(&self) -> Result<()> {
        let mut held = self.copy.0.lock().await;
        let synced =
            worktrees::push(&self.session, &self.path, held.as_ref())
                .await?;
        *held = Some(synced);
        Ok(())
    }

    /// Makes sure the remote copy was in step once, before a call
    /// runs there.
    pub async fn in_step(&self) -> Result<()> {
        let mut held = self.copy.0.lock().await;
        if held.is_none() {
            *held = Some(
                worktrees::push(&self.session, &self.path, None)
                    .await?,
            );
        }
        Ok(())
    }

    /// Brings home what the remote copy changed since they were last
    /// in step. Never in step, nothing the remote holds is known to
    /// be newer, so the server's worktree stands and is pushed.
    pub async fn pull(&self) -> Result<()> {
        let mut held = self.copy.0.lock().await;
        let Some(base) = held.as_ref() else {
            *held = Some(
                worktrees::push(&self.session, &self.path, None)
                    .await?,
            );
            return Ok(());
        };
        let (pulled, synced) =
            worktrees::pull(&self.session, base).await?;
        {
            let _held = self.writing.hold();
            pulled.apply(&self.path)?;
        }
        *held = Some(synced);
        Ok(())
    }
}

/// The session of `agent`'s tools in the workspace `transport`
/// reaches.
fn session_in(
    db: &mut Db,
    transport: &Transport,
    agent: AgentId,
    network: Network,
) -> Result<Session> {
    let hamlet = hamlet_of(db, agent)?;
    let project = match &hamlet {
        Some(id) => hamlets::get(db, id)?.folder.is_some(),
        None => false,
    };
    transport.session(
        agent,
        &workspace_of(hamlet.as_ref()),
        Policy {
            project,
            network: network.is_open(),
            read_only: false,
        },
    )
}

impl RemoteWorkspace<'_> {
    /// The session of `agent`'s tools in the remote workspace, when
    /// the realm works in one.
    pub(crate) fn session(
        &self,
        db: &mut Db,
        agent: AgentId,
        network: Network,
    ) -> Result<Option<Session>> {
        let Some(transport) = &**self.workspace else {
            return Ok(None);
        };
        Ok(Some(session_in(db, transport, agent, network)?))
    }

    /// The keeper of `agent`'s worktree's remote copy, when the realm
    /// works in a remote workspace and the agent has a worktree.
    pub(crate) fn worktree(
        &mut self,
        realm: &mut Realm,
        writing: &RepositoryWriteLock,
        agent: AgentId,
    ) -> Result<Option<RemoteWorktree>> {
        let layout = realm.layout().clone();
        let db = realm.store().connection();
        let Ok(found) = knowledge::worktree(db, &layout, agent)
        else {
            return Ok(None);
        };
        let Some(session) =
            self.session(db, agent, Network::Closed)?
        else {
            return Ok(None);
        };
        Ok(Some(RemoteWorktree {
            session,
            path: found.path,
            copy: self.worktrees.entry(agent).or_default().clone(),
            writing: writing.clone(),
        }))
    }

    /// Owes the remote workspace a push of the worktrees the jobs
    /// `done` changed, when the realm works in one.
    pub(crate) fn owe_pushes(&mut self, db: &mut Db, done: &[Job]) {
        if self.workspace.is_none() {
            return;
        }
        for job in done {
            self.unpushed.write_batch(
                job.worktrees(db).into_iter().map(UnpushedWorktree),
            );
        }
    }

    /// Starts pushing each worktree owed a push, on a task of its
    /// own; a push that fails is logged with `secrets` left out, and
    /// the next push sends what it would have.
    pub(crate) fn push_owed(
        &mut self,
        realm: &mut Realm,
        writing: &RepositoryWriteLock,
        secrets: &[String],
    ) -> Result<()> {
        let unpushed = self
            .unpushed
            .drain()
            .map(|UnpushedWorktree(agent)| agent)
            .collect::<BTreeSet<_>>();
        for agent in unpushed {
            let Some(worktree) =
                self.worktree(realm, writing, agent)?
            else {
                continue;
            };
            let secrets = secrets.to_vec();
            tokio::spawn(async move {
                if let Err(error) = worktree.push().await {
                    let words =
                        scrub_text(&secrets, &format!("{error:#}"));
                    warn!(
                        "could not bring a worktree to the \
                         workspace: {words}"
                    );
                }
            });
        }
        Ok(())
    }
}

impl Core<'_, '_> {
    /// Starts copying every worktree to the workspace chosen in the
    /// settings, replacing what is there, on a task of its own, which
    /// reports how it goes and answers `id` once done. The realm goes
    /// on working where it works until its next start.
    pub(crate) fn move_worktrees(
        &mut self,
        client: u64,
        id: &RequestId,
    ) -> Result<()> {
        let Some(chosen) = self.credentials.workspace().cloned()
        else {
            return self.answer(
                client,
                reply(id, Err(Refusal::NoWorkspaceChosen)),
            );
        };
        let transport = Transport::new(chosen)?;
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let mut with_worktrees = vec![crate::society::hand::id(db)?];
        with_worktrees.extend(
            agents::living(db)?.into_iter().filter(|agent| {
                hamlet_agents::find(db, *agent)
                    .is_ok_and(|found| found.is_some())
            }),
        );
        let mut moves = Vec::new();
        for agent in with_worktrees {
            let db = self.realm.store().connection();
            let Ok(found) = knowledge::worktree(db, &layout, agent)
            else {
                continue;
            };
            moves.push(RemoteWorktree {
                session: session_in(
                    db,
                    &transport,
                    agent,
                    Network::Closed,
                )?,
                path: found.path,
                // Nothing is in step with another workspace yet.
                copy: RemoteCopy::default(),
                writing: self.writing.clone(),
            });
        }
        let of = i64::try_from(moves.len())?;
        self.remote.moving.0 = Some(progress(0, of, Stage::Copying));
        self.changed.state_changed_outside_change = true;
        let secrets = self.credentials.secrets();
        let id = id.clone();
        self.later.spawn_many(move |report| async move {
            let mut failed = None;
            for (done, worktree) in moves.into_iter().enumerate() {
                let done = i64::try_from(done).unwrap_or(of);
                report.send(BackgroundResult::Moving {
                    progress: progress(done, of, Stage::Copying),
                    answer: None,
                });
                if let Err(error) = worktree.push().await {
                    failed = Some(scrub_text(
                        &secrets,
                        &format!("{error:#}"),
                    ));
                    break;
                }
            }
            let stage = match &failed {
                None => Stage::Moved,
                Some(error) => Stage::Failed(error),
            };
            let last = progress(of, of, stage);
            let answer = match failed {
                None => Ok(last.text.clone()),
                Some(error) => Err(Refusal::MoveFailed { error }),
            };
            report.send(BackgroundResult::Moving {
                progress: last,
                answer: Some((client, reply(&id, answer))),
            });
        });
        Ok(())
    }
}

/// The stage of moving the worktrees.
enum Stage<'a> {
    Copying,
    Moved,
    Failed(&'a str),
}

/// The progress of moving the worktrees: `done` of `of` copied, at
/// `stage`.
fn progress(
    done: i64,
    of: i64,
    stage: Stage<'_>,
) -> SettingsViewWorkspaceMove {
    let (state, text) = match stage {
        Stage::Copying => (
            SettingsViewWorkspaceMoveState::Moving,
            "Copying the worktrees.".to_owned(),
        ),
        Stage::Moved => (
            SettingsViewWorkspaceMoveState::Moved,
            "Every worktree is in the chosen workspace; the realm \
             works there from its next start."
                .to_owned(),
        ),
        Stage::Failed(error) => (
            SettingsViewWorkspaceMoveState::Failed,
            format!("Moving the worktrees failed: {error}"),
        ),
    };
    SettingsViewWorkspaceMove {
        done,
        of,
        state,
        text,
    }
}
