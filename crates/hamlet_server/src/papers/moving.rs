//! Copies living houses to the next-start backend while the active
//! backend stays selected.

use anyhow::{Context, Result};
use hamlet_protocol::{
    SettingsViewWorkspaceMove, SettingsViewWorkspaceMoveState,
};
use hamlet_workspace_wire::{TREE_BYTES, TREE_ENTRIES};
use tokio::sync::watch;
use tokio::task::JoinSet;

use super::Houses;
use super::copy::Snapshot;
use crate::protocol::{Refusable, Refusal};
use crate::record::AgentRef;
use crate::service::Service;

pub(crate) struct Moving {
    pub tasks: JoinSet<(u64, String, Refusable<String>)>,
    progress: watch::Receiver<Option<SettingsViewWorkspaceMove>>,
}

impl Default for Moving {
    fn default() -> Self {
        Self {
            tasks: JoinSet::new(),
            progress: watch::channel(None).1,
        }
    }
}

impl Moving {
    /// How the last move went, if one was started.
    pub fn view(&self) -> Option<SettingsViewWorkspaceMove> {
        self.progress.borrow().clone()
    }
}

impl Service {
    pub(crate) fn start_move(
        &mut self,
        owner: u64,
        id: &str,
    ) -> Result<Refusable<()>> {
        let refuse =
            |text: &str| Ok(Err(Refusal::Invalid(text.into())));
        if self.overrides.workspace.is_some() {
            return refuse(
                "HAMLET_WORKSPACE_URL chooses the workspace for \
                 this run, so houses are not moved to the one in \
                 Settings.",
            );
        }
        let from = self.houses.workspace();
        let to = self.credentials.workspace();
        if from.map(|c| (&c.url, &c.token))
            == to.map(|c| (&c.url, &c.token))
        {
            return refuse(
                "Hamlet already works in the workspace chosen in \
                 Settings, so there are no houses to move.",
            );
        }
        if !self.moving.tasks.is_empty() {
            return refuse("The houses are already moving.");
        }
        let to = to.cloned();
        let live =
            crate::realm::live(self.home.store().connection())?;
        let agents =
            crate::service::records(self.home.store().connection())?
                .into_iter()
                .filter(|record| live.contains(&record.id))
                .flat_map(|record| {
                    record.agents.into_iter().map(move |agent| {
                        AgentRef {
                            hamlet: record.id.clone(),
                            number: agent.number,
                        }
                    })
                })
                .collect::<Vec<_>>();
        let root = self.home.layout().root();
        let source = Houses::open(root, from.cloned())?;
        let target = Houses::open(root, to.clone())?;
        let of = i64::try_from(agents.len())
            .context("the number of houses to move")?;
        let (progress, receiver) =
            watch::channel(Some(SettingsViewWorkspaceMove {
                state: SettingsViewWorkspaceMoveState::Moving,
                done: 0,
                of,
                text: String::new(),
            }));
        self.moving.progress = receiver;
        let id = id.to_owned();
        let remote = to.is_some();
        self.moving.tasks.spawn(async move {
            let moved = move_houses(
                &source, &target, &agents, of, remote, &progress,
            )
            .await;
            (owner, id, moved)
        });
        Ok(Ok(()))
    }
}

/// Moves each agent's house from `source` to `target`, keeping
/// `progress` up to date, and says what was moved.
async fn move_houses(
    source: &Houses,
    target: &Houses,
    agents: &[AgentRef],
    of: i64,
    remote: bool,
    progress: &watch::Sender<Option<SettingsViewWorkspaceMove>>,
) -> Refusable<String> {
    let tell = |state, done, text: &str| {
        progress.send_replace(Some(SettingsViewWorkspaceMove {
            state,
            done,
            of,
            text: text.into(),
        }));
    };
    let mut skipped = Vec::new();
    for (done, agent) in (0..).zip(agents) {
        match move_house(source, target, agent).await {
            Ok(left) => {
                skipped.extend(left.into_iter().map(|path| {
                    format!(
                        "{}/{}/{path}",
                        agent.hamlet, agent.number
                    )
                }))
            }
            Err(error) => {
                let text = format!(
                    "The house of agent {} in {} was not moved: \
                     {error} {done} houses before it moved; moving \
                     again copies them all once more. Inspect the \
                     target before retrying.",
                    agent.number, agent.hamlet
                );
                tell(
                    SettingsViewWorkspaceMoveState::Failed,
                    done,
                    &text,
                );
                return Err(Refusal::Invalid(text));
            }
        }
        tell(SettingsViewWorkspaceMoveState::Moving, done + 1, "");
    }
    let mut text = format!(
        "Moved {of} {} to {}. Restart Hamlet to work there; what \
         agents change before then stays behind.",
        if of == 1 { "house" } else { "houses" },
        if remote {
            "the remote server"
        } else {
            "this computer"
        },
    );
    if !skipped.is_empty() {
        let more = if skipped.len() > 5 {
            format!(", and {} more", skipped.len() - 5)
        } else {
            String::new()
        };
        text.push_str(&format!(
            " Links and special files are not moved: {}{more}.",
            skipped[..skipped.len().min(5)].join(", "),
        ));
    }
    tell(SettingsViewWorkspaceMoveState::Moved, of, &text);
    Ok(text)
}

async fn move_house(
    source: &Houses,
    target: &Houses,
    agent: &AgentRef,
) -> Result<Vec<String>> {
    let snapshot = Snapshot::read(
        &source.house(&agent.hamlet, agent.number)?,
        TREE_BYTES,
        TREE_ENTRIES as usize,
        true,
    )
    .await?;
    target
        .replace(&agent.hamlet, agent.number, &snapshot)
        .await?;
    Ok(snapshot.skipped)
}
