//! What agents know: scholars' repositories, the lodges hires and the
//! Hand work in, and the jobs that keep them.

pub(crate) mod dreams;
mod edits;
pub(crate) mod folder;
pub(crate) mod library;
mod merges;
pub(crate) mod promotion;
pub(crate) mod run;
pub(crate) mod versions;
pub(crate) mod worker;

use std::path::PathBuf;

use anyhow::Result;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ScholarId};

use crate::realm::layout::Layout;
use crate::society::agents::{self, AgentKind};
use crate::society::refusal::Refusal;
use crate::society::{hand, hires};

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_plugins(worker::plugin);
}

/// The character's file in a scholar's repository.
pub(crate) const CHARACTER: &str = "CHARACTER.md";

/// Whether a lodge keeps `path` out of git: the character, which no
/// agent writes (C13), and the inbox, whose files are not knowledge.
pub(crate) fn outside_git(path: &str) -> bool {
    path == CHARACTER || path == "inbox" || path.starts_with("inbox/")
}

/// Refuses a memory path that is not a plain relative path under
/// `memory/`.
pub(crate) fn memory_path(path: &str) -> Result<String, Refusal> {
    let path = path.trim().trim_start_matches("memory/");
    let plain = !path.is_empty()
        && !path.starts_with('/')
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".."
        });
    if plain {
        Ok(format!("memory/{path}"))
    } else {
        Err(Refusal::NoFile { path: path.into() })
    }
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
                path: layout.lodge(&hire.hamlet, hire.agent),
                pin: hire.pin,
                living: found.is_living(),
            })
        }
        AgentKind::Worker => anyhow::bail!("a worker has no lodge"),
    }
}

#[cfg(test)]
mod tests;
