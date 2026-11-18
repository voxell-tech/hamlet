//! What agents' tools do: the built-in tools under each agent's
//! reach (`builtin`), the custom tools a chief adds by hand
//! (`custom`), the catalogue of what a model may be offered
//! (`catalogue`), and the diff an edit shows (`edit_diff`).

pub(crate) mod builtin;
pub(crate) mod catalogue;
pub(crate) mod custom;
pub(crate) mod edit_diff;

use anyhow::Result;
use bevy_app::App;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, HamletId};

use crate::society::agents::{self, AgentKind};
use crate::society::{hamlet_agents, workers};

/// Adds the plugin of the custom tools.
pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(custom::plugin);
}

/// The hamlet whose folder `agent`'s tools work on: its own, a
/// worker's own, else its starter's; none for the Hand.
pub(crate) fn hamlet_of(
    db: &mut Db,
    agent: AgentId,
) -> Result<Option<HamletId>> {
    Ok(match agents::get(db, agent)?.kind {
        AgentKind::Hand => None,
        AgentKind::Hamlet => {
            Some(hamlet_agents::get(db, agent)?.hamlet)
        }
        AgentKind::Worker => {
            let worker = workers::get(db, agent)?;
            match (worker.hamlet, worker.starter) {
                (Some(hamlet), _) => Some(hamlet),
                (None, Some(starter)) => {
                    hamlet_agents::find(db, starter)?
                        .map(|h| h.hamlet)
                }
                (None, None) => None,
            }
        }
    })
}
