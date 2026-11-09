//! What agents' tools do: the built-in tools under each agent's
//! reach, the tools a chief adds by hand, and how an edit's change is
//! shown.

pub(crate) mod added;
pub(crate) mod catalogue;
pub(crate) mod cut;
pub(crate) mod tools;

use anyhow::Result;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, HamletId};

use crate::society::agents::{self, AgentKind};
use crate::society::{hires, workers};

/// The hamlet whose folder `agent`'s tools work on: a hire's, a
/// worker's own, else its starter's; none for the Hand.
pub(crate) fn hamlet_of(
    db: &mut Db,
    agent: AgentId,
) -> Result<Option<HamletId>> {
    Ok(match agents::get(db, agent)?.kind {
        AgentKind::Hand => None,
        AgentKind::Hire => Some(hires::get(db, agent)?.hamlet),
        AgentKind::Worker => {
            let worker = workers::get(db, agent)?;
            match (worker.hamlet, worker.starter) {
                (Some(hamlet), _) => Some(hamlet),
                (None, Some(starter)) => {
                    hires::find(db, starter)?.map(|h| h.hamlet)
                }
                (None, None) => None,
            }
        }
    })
}
