//! What agents know: profiles' repositories, the worktrees the agents
//! of hamlets and the Hand work in, and the jobs that keep them.

pub(crate) mod dreams;
pub(crate) mod folder;
pub(crate) mod library;
mod merges;
pub(crate) mod run;
pub(crate) mod worker;

use std::path::PathBuf;

use anyhow::Result;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ProfileId};

use crate::realm::layout::Layout;
use crate::society::agents::{self, AgentKind};
use crate::society::{hamlet_agents, hand};

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_plugins(worker::plugin);
}

/// The character's file in a profile's repository.
pub(crate) const CHARACTER: &str = "CHARACTER.md";

/// Whether a worktree keeps `path` out of git: the character, which
/// no agent writes (C13), and the inbox, whose files are not
/// knowledge.
pub(crate) fn outside_git(path: &str) -> bool {
    path == CHARACTER || path == "inbox" || path.starts_with("inbox/")
}

/// The branch of an agent's worktree.
pub(crate) fn branch(agent: AgentId) -> String {
    format!("agents/{agent}")
}

/// Whose a worktree is, of which profile, and where it is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Worktree {
    pub agent: AgentId,
    pub profile: ProfileId,
    pub path: PathBuf,
    pub pin: Option<String>,
    pub living: bool,
}

/// The worktree of the Hand or the agent `agent` of a hamlet.
pub(crate) fn worktree(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
) -> Result<Worktree> {
    let found = agents::get(db, agent)?;
    match found.kind {
        AgentKind::Hand => Ok(Worktree {
            agent,
            profile: hand::profile(db)?,
            path: layout.hand(),
            pin: None,
            living: found.is_living(),
        }),
        AgentKind::Hamlet => {
            let placed = hamlet_agents::get(db, agent)?;
            Ok(Worktree {
                agent,
                profile: placed.profile,
                path: layout.worktree(&placed.hamlet, placed.agent),
                pin: placed.pin,
                living: found.is_living(),
            })
        }
        AgentKind::Worker => {
            anyhow::bail!("a worker has no worktree")
        }
    }
}

#[cfg(test)]
mod tests;
