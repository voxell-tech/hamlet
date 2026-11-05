//! What agents know: scholars' repositories, the lodges hires and the
//! Hand work in, and the jobs that keep them.

pub(crate) mod folder;
pub(crate) mod run;
pub(crate) mod runner;

use std::path::PathBuf;

use anyhow::Result;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ScholarId};

use crate::layout::Layout;
use crate::society::agents::{self, AgentKind};
use crate::society::{hand, hires};

/// The character's file in a scholar's repository.
pub(crate) const CHARACTER: &str = "CHARACTER.md";

/// Whether a lodge keeps `path` out of git: the character, which no
/// agent writes (C13), and the inbox, whose gifts are not knowledge.
pub(crate) fn outside_git(path: &str) -> bool {
    path == CHARACTER || path == "inbox" || path.starts_with("inbox/")
}

/// The branch of an agent's lodge.
pub(crate) fn branch(agent: AgentId) -> String {
    format!("hires/{agent}")
}

/// Whose a lodge is, of which scholar, and where it is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lodging {
    pub agent: AgentId,
    pub scholar: ScholarId,
    pub path: PathBuf,
    pub pin: Option<String>,
    pub living: bool,
}

/// The lodge of the Hand or the hire `agent`.
pub(crate) fn lodging(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
) -> Result<Lodging> {
    let found = agents::get(db, agent)?;
    match found.kind {
        AgentKind::Hand => Ok(Lodging {
            agent,
            scholar: hand::scholar(db)?,
            path: layout.hand(),
            pin: None,
            living: found.is_living(),
        }),
        AgentKind::Hire => {
            let hire = hires::get(db, agent)?;
            Ok(Lodging {
                agent,
                scholar: hire.scholar,
                path: layout.lodge(&hire.hamlet, hire.number),
                pin: hire.pin,
                living: found.is_living(),
            })
        }
        AgentKind::Worker => anyhow::bail!("a worker has no lodge"),
    }
}

#[cfg(test)]
mod tests;
