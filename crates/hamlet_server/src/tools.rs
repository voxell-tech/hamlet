//! What agents' tools do: the built-in tools under each agent's
//! reach (`builtin`), the custom tools a chief adds by hand
//! (`custom`), the tools of the MCP servers a chief lists (`mcp`),
//! the catalogue of what a model may be offered (`catalogue`), and
//! the diff an edit shows (`edit_diff`).

pub(crate) mod builtin;
pub(crate) mod catalogue;
pub(crate) mod custom;
pub(crate) mod edit_diff;
pub(crate) mod mcp;

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use bevy_app::App;
use bevy_ecs::message::Message;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, HamletId};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::app::input::InputSender;
use crate::society::agents::{self, AgentKind};
use crate::society::{hamlet_agents, workers};

/// Adds the plugins of the custom tools and the MCP tools.
pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((custom::plugin, mcp::plugin));
}

/// The time the changes to a watched folder settle before what it
/// holds loads again.
const SETTLE: Duration = Duration::from_millis(200);

/// Watches `folder`, sending `inputs` the message `made` gives once
/// the changes of the paths `wanted` takes settle. The watch ends
/// when the watcher drops.
fn watch<M: Message>(
    folder: &Path,
    mode: RecursiveMode,
    wanted: fn(&Path) -> bool,
    inputs: InputSender,
    made: fn() -> M,
) -> notify::Result<RecommendedWatcher> {
    let (changed, mut changes) = mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(
        move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|event| {
                !event.kind.is_access()
                    && event.paths.iter().any(|path| wanted(path))
            }) {
                let _ = changed.send(());
            }
        },
    )?;
    watcher.watch(folder, mode)?;
    tokio::spawn(async move {
        while changes.recv().await.is_some() {
            // A save is many changes: once they settle, one load
            // takes them all.
            tokio::time::sleep(SETTLE).await;
            while changes.try_recv().is_ok() {}
            if inputs.send(made()).is_err() {
                return;
            }
        }
    });
    Ok(watcher)
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
