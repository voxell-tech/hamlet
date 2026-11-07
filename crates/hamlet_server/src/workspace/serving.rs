//! The realm working in a remote workspace: each agent's session
//! there, its lodge kept in step, and every lodge moved there at a
//! chief's word.

use std::sync::PoisonError;

use anyhow::Result;
use hamlet_protocol::client::{
    RequestId, SettingsViewWorkspaceMove,
    SettingsViewWorkspaceMoveState,
};
use hamlet_store::ids::AgentId;

use super::{Policy, Session, Transport, lodges, workspace_of};
use crate::clients::check::reply;
use crate::core::Core;
use crate::diagnostic::{Secrets, tell};
use crate::jobs::Job;
use crate::knowledge;
use crate::society::refusal::Refusal;
use crate::society::{agents, hamlets, hires, merges};
use crate::work::hamlet_of;

impl Core {
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
        let db = self.realm.store().connection();
        let hamlet = hamlet_of(db, agent)?;
        let project = match &hamlet {
            Some(id) => hamlets::get(db, id)?.folder.is_some(),
            None => false,
        };
        Ok(Some(transport.session(
            agent,
            &workspace_of(hamlet.as_ref()),
            Policy {
                project,
                network: net,
                read_only: false,
            },
        )?))
    }

    /// Sends the remote copy of `agent`'s lodge what the server's
    /// changed since they were last in step, or all of it when they
    /// never were.
    pub(crate) async fn push_lodge(
        &mut self,
        agent: AgentId,
    ) -> Result<()> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let Ok(lodging) = knowledge::lodging(db, &layout, agent)
        else {
            return Ok(());
        };
        let Some(session) = self.remote_session(agent, false)? else {
            return Ok(());
        };
        let base = self.synced.get(&agent);
        let synced =
            lodges::push(&session, &lodging.path, base).await?;
        self.synced.insert(agent, synced);
        Ok(())
    }

    /// Brings home what the remote copy of `agent`'s lodge changed
    /// since they were last in step.
    pub(crate) async fn pull_lodge(
        &mut self,
        agent: AgentId,
    ) -> Result<()> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let Ok(lodging) = knowledge::lodging(db, &layout, agent)
        else {
            return Ok(());
        };
        let Some(session) = self.remote_session(agent, false)? else {
            return Ok(());
        };
        // Never in step: nothing the remote holds is known to be
        // newer, so the server's lodge stands.
        let Some(base) = self.synced.get(&agent) else {
            return self.push_lodge(agent).await;
        };
        let (pulled, synced) = lodges::pull(&session, base).await?;
        let writing = self.writing.clone();
        let held =
            writing.lock().unwrap_or_else(PoisonError::into_inner);
        pulled.apply(&lodging.path)?;
        drop(held);
        self.synced.insert(agent, synced);
        Ok(())
    }

    /// Makes sure the remote copy of `agent`'s lodge was in step once
    /// before a call runs there.
    pub(crate) async fn lodge_in_step(
        &mut self,
        agent: AgentId,
    ) -> Result<()> {
        if self.synced.contains_key(&agent) {
            return Ok(());
        }
        self.push_lodge(agent).await
    }

    /// Pushes the lodges the jobs `done` changed, when the realm
    /// works in a remote workspace.
    pub(crate) async fn push_lodges(&mut self, done: &[Job]) {
        if self.workspace.is_none() {
            return;
        }
        let mut agents = Vec::new();
        for job in done {
            let found = match job {
                Job::Lodge { hire } | Job::TakeIn { hire } => {
                    vec![*hire]
                }
                Job::Merge { merge } | Job::Split { merge } => {
                    let db = self.realm.store().connection();
                    match merges::get(db, *merge) {
                        Ok(found) => vec![found.kept, found.absorbed],
                        Err(_) => Vec::new(),
                    }
                }
                Job::DreamEnd { dream, .. } => {
                    let db = self.realm.store().connection();
                    crate::society::dreams::get(db, *dream)
                        .ok()
                        .and_then(|found| found.hire)
                        .into_iter()
                        .collect()
                }
                Job::Shelve { offer } => {
                    let db = self.realm.store().connection();
                    crate::society::library::get(db, *offer)
                        .map(|found| vec![found.agent])
                        .unwrap_or_default()
                }
                _ => Vec::new(),
            };
            agents.extend(found);
        }
        agents.sort();
        agents.dedup();
        for agent in agents {
            if let Err(error) = self.push_lodge(agent).await {
                tell(
                    Secrets::Of(&self.credentials),
                    format!(
                        "could not bring a lodge to the workspace: \
                         {error:#}"
                    ),
                );
            }
        }
    }

    /// Copies every lodge to the workspace chosen in the settings,
    /// replacing what is there, and answers `id` once done.
    pub(crate) async fn move_lodges(
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
        let previous =
            self.workspace.replace(Transport::new(chosen)?);
        // What was in step was in step with another workspace.
        self.synced.clear();
        let db = self.realm.store().connection();
        let mut lodged = vec![crate::society::hand::id(db)?];
        lodged.extend(agents::living(db)?.into_iter().filter(
            |agent| {
                hires::find(db, *agent)
                    .is_ok_and(|found| found.is_some())
            },
        ));
        let of = i64::try_from(lodged.len())?;
        let mut failed = None;
        for (done, agent) in lodged.into_iter().enumerate() {
            self.moving = Some(SettingsViewWorkspaceMove {
                done: i64::try_from(done)?,
                of,
                state: SettingsViewWorkspaceMoveState::Moving,
                text: "Copying the lodges.".into(),
            });
            self.publish().await?;
            if let Err(error) = self.push_lodge(agent).await {
                failed = Some(
                    self.credentials
                        .scrub_text(&format!("{error:#}")),
                );
                break;
            }
        }
        self.workspace = previous;
        self.synced.clear();
        let (state, text) = match &failed {
            None => (
                SettingsViewWorkspaceMoveState::Moved,
                "Every lodge is in the chosen workspace; the realm \
                 works there from its next start."
                    .to_owned(),
            ),
            Some(error) => (
                SettingsViewWorkspaceMoveState::Failed,
                format!("Moving the lodges failed: {error}"),
            ),
        };
        self.moving = Some(SettingsViewWorkspaceMove {
            done: of,
            of,
            state,
            text: text.clone(),
        });
        self.publish().await?;
        let answer = match failed {
            None => Ok(text),
            Some(_) => Err(Refusal::Usage { words: text }),
        };
        self.answer(client, reply(id, answer))
    }
}
