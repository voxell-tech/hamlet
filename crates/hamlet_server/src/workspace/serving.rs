//! The realm working in a remote workspace: each agent's session
//! there, its lodge's remote copy kept in step by tasks of their own,
//! and every lodge moved there at a chief's word. No handler awaits
//! the workspace: a push, a pull or a move runs as a task.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use hamlet_protocol::client::{
    RequestId, SettingsViewWorkspaceMove,
    SettingsViewWorkspaceMoveState,
};
use hamlet_store::ids::AgentId;

use super::lodges::{self, Synced};
use super::{Policy, Session, Transport, workspace_of};
use crate::clients::check::reply;
use crate::core::Core;
use crate::diagnostic::{Secrets, tell};
use crate::jobs::Job;
use crate::knowledge;
use crate::knowledge::runner::Writing;
use crate::later::Finished;
use crate::society::refusal::Refusal;
use crate::society::{agents, hamlets, hires};
use crate::work::hamlet_of;

/// A lodge's remote copy: what both copies held alike when last in
/// step, or nothing before its first push. The tasks that push to it
/// and pull from it take it one at a time.
#[derive(Clone, Default)]
pub(crate) struct RemoteCopy(Arc<tokio::sync::Mutex<Option<Synced>>>);

/// What a task needs to keep one lodge's remote copy in step.
#[derive(Clone)]
pub(crate) struct Lodging {
    session: Session,
    path: PathBuf,
    copy: RemoteCopy,
    writing: Writing,
}

impl Lodging {
    /// Sends the remote copy what the server's lodge changed since
    /// they were last in step, or all of it when they never were.
    pub async fn push(&self) -> Result<()> {
        let mut held = self.copy.0.lock().await;
        let synced =
            lodges::push(&self.session, &self.path, held.as_ref())
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
                lodges::push(&self.session, &self.path, None).await?,
            );
        }
        Ok(())
    }

    /// Brings home what the remote copy changed since they were last
    /// in step. Never in step, nothing the remote holds is known to
    /// be newer, so the server's lodge stands and is pushed.
    pub async fn pull(&self) -> Result<()> {
        let mut held = self.copy.0.lock().await;
        let Some(base) = held.as_ref() else {
            *held = Some(
                lodges::push(&self.session, &self.path, None).await?,
            );
            return Ok(());
        };
        let (pulled, synced) =
            lodges::pull(&self.session, base).await?;
        {
            let _held = self.writing.hold();
            pulled.apply(&self.path)?;
        }
        *held = Some(synced);
        Ok(())
    }
}

impl Core<'_, '_> {
    /// The session of `agent`'s tools in the remote workspace, when
    /// the realm works in one; its commands reach the network with
    /// `net`.
    pub(crate) fn remote_session(
        &mut self,
        agent: AgentId,
        net: bool,
    ) -> Result<Option<Session>> {
        let Some(transport) = self.workspace.clone() else {
            return Ok(None);
        };
        Ok(Some(self.session_in(&transport, agent, net)?))
    }

    /// The session of `agent`'s tools in the workspace `transport`
    /// reaches.
    fn session_in(
        &mut self,
        transport: &Transport,
        agent: AgentId,
        net: bool,
    ) -> Result<Session> {
        let db = self.realm.store().connection();
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
                network: net,
                read_only: false,
            },
        )
    }

    /// What keeps `agent`'s lodge's remote copy in step, when the
    /// realm works in a remote workspace and the agent has a lodge.
    pub(crate) fn lodging(
        &mut self,
        agent: AgentId,
    ) -> Result<Option<Lodging>> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let Ok(found) = knowledge::lodging(db, &layout, agent) else {
            return Ok(None);
        };
        let Some(session) = self.remote_session(agent, false)? else {
            return Ok(None);
        };
        Ok(Some(Lodging {
            session,
            path: found.path,
            copy: self.synced.entry(agent).or_default().clone(),
            writing: self.writing.clone(),
        }))
    }

    /// Owes the remote workspace a push of the lodges the jobs `done`
    /// changed, when the realm works in one.
    pub(crate) fn owe_pushes(&mut self, done: &[Job]) {
        if self.workspace.is_none() {
            return;
        }
        for job in done {
            let lodges = job.lodges(self.realm.store().connection());
            self.unpushed.extend(lodges);
        }
    }

    /// Starts pushing each lodge owed a push, on a task of its own; a
    /// push that fails is logged, and the next push sends what it
    /// would have.
    pub(crate) fn push_owed(&mut self) -> Result<()> {
        let secrets = self.credentials.secrets();
        for agent in std::mem::take(&mut self.unpushed.0) {
            let Some(lodging) = self.lodging(agent)? else {
                continue;
            };
            let secrets = secrets.clone();
            tokio::spawn(async move {
                if let Err(error) = lodging.push().await {
                    tell(
                        Secrets::Listed(&secrets),
                        format!(
                            "could not bring a lodge to the \
                             workspace: {error:#}"
                        ),
                    );
                }
            });
        }
        Ok(())
    }

    /// Starts copying every lodge to the workspace chosen in the
    /// settings, replacing what is there, on a task of its own, which
    /// reports how it goes and answers `id` once done. The realm goes
    /// on working where it works until its next start.
    pub(crate) fn move_lodges(
        &mut self,
        client: u64,
        id: &RequestId,
    ) -> Result<()> {
        let Some(chosen) = self.credentials.workspace().cloned()
        else {
            let refusal = Refusal::Usage {
                words: "Choose a remote workspace in the settings \
                        first."
                    .into(),
            };
            return self.answer(client, reply(id, Err(refusal)));
        };
        let transport = Transport::new(chosen)?;
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let mut lodged = vec![crate::society::hand::id(db)?];
        lodged.extend(agents::living(db)?.into_iter().filter(
            |agent| {
                hires::find(db, *agent)
                    .is_ok_and(|found| found.is_some())
            },
        ));
        let mut moves = Vec::new();
        for agent in lodged {
            let db = self.realm.store().connection();
            let Ok(found) = knowledge::lodging(db, &layout, agent)
            else {
                continue;
            };
            moves.push(Lodging {
                session: self.session_in(&transport, agent, false)?,
                path: found.path,
                // Nothing is in step with another workspace yet.
                copy: RemoteCopy::default(),
                writing: self.writing.clone(),
            });
        }
        let of = i64::try_from(moves.len())?;
        self.moving.0 = Some(progress(0, of, None));
        self.changed.republish = true;
        let secrets = self.credentials.secrets();
        let id = id.clone();
        self.later.spawn_many(move |report| async move {
            let mut failed = None;
            for (done, lodging) in moves.into_iter().enumerate() {
                let done = i64::try_from(done).unwrap_or(of);
                report.send(Finished::Moving {
                    progress: progress(done, of, None),
                    answer: None,
                });
                if let Err(error) = lodging.push().await {
                    failed = Some(hamlet_sandbox::scrub::scrub_text(
                        &secrets,
                        &format!("{error:#}"),
                    ));
                    break;
                }
            }
            let last = progress(of, of, Some(failed.as_deref()));
            let answer = match failed {
                None => Ok(last.text.clone()),
                Some(_) => Err(Refusal::Usage {
                    words: last.text.clone(),
                }),
            };
            report.send(Finished::Moving {
                progress: last,
                answer: Some((client, reply(&id, answer))),
            });
        });
        Ok(())
    }
}

/// How moving the lodges goes: `done` of `of` copied, and, once over,
/// whether it failed and why.
fn progress(
    done: i64,
    of: i64,
    over: Option<Option<&str>>,
) -> SettingsViewWorkspaceMove {
    let (state, text) = match over {
        None => (
            SettingsViewWorkspaceMoveState::Moving,
            "Copying the lodges.".to_owned(),
        ),
        Some(None) => (
            SettingsViewWorkspaceMoveState::Moved,
            "Every lodge is in the chosen workspace; the realm \
             works there from its next start."
                .to_owned(),
        ),
        Some(Some(error)) => (
            SettingsViewWorkspaceMoveState::Failed,
            format!("Moving the lodges failed: {error}"),
        ),
    };
    SettingsViewWorkspaceMove {
        done,
        of,
        state,
        text,
    }
}
