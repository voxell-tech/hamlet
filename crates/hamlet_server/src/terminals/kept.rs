//! The terminals kept across restarts, under the realm's
//! `terminals/`.

use std::path::PathBuf;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bevy_ecs::system::{Res, ResMut};
use bevy_log::warn;
use hamlet_protocol::client::{TerminalKind, TerminalView};
use hamlet_store::Db;
use serde::{Deserialize, Serialize};

use super::Terminals;
use super::screen::lock;
use super::shell::relaunch;
use crate::realm::Realm;
use crate::server::InputSender;
use crate::society::agents::{self, AgentKind};
use crate::society::hamlets;

/// Said in a shell that a restart started again.
const RESTARTED: &str = "\r\n\x1b[2m[The server restarted, so this \
                         is a new shell.]\x1b[0m\r\n";

/// A terminal as it is kept between starts.
#[derive(Serialize, Deserialize)]
struct Kept {
    view: TerminalView,
    /// The bytes that redraw it, in base64.
    replay: String,
}

impl Terminals {
    pub(super) fn file(&self, id: &str) -> PathBuf {
        self.folder.join(format!("{id}.json"))
    }

    /// Writes terminal `id` where the next start finds it.
    pub(super) fn keep(&self, id: &str) {
        let Some(terminal) = self.open.get(id) else {
            return;
        };
        let kept = Kept {
            view: terminal.view.clone(),
            replay: STANDARD.encode(lock(&terminal.screen).replay()),
        };
        let written =
            std::fs::create_dir_all(&self.folder).and_then(|()| {
                let text = serde_json::to_vec(&kept)?;
                let path = self.file(id);
                let part = path.with_extension("part");
                std::fs::write(&part, text)?;
                std::fs::rename(part, path)
            });
        if let Err(error) = written {
            warn!("could not keep a terminal: {error}");
        }
    }

    /// Writes every terminal where the next start finds it.
    pub(crate) fn keep_all(&self) {
        for id in self.open.keys() {
            self.keep(id);
        }
    }
}

/// Opens again the terminals kept at the last stop: a new shell where
/// one ran, below what the old one printed. A terminal whose agent or
/// hamlet is gone is forgotten, and so is a worker's, which was for
/// one task. The server starts without them when they cannot be read.
pub(crate) fn restore(
    mut terminals: ResMut<Terminals>,
    mut realm: ResMut<Realm>,
    inputs: Res<InputSender>,
) {
    let db = realm.store().connection();
    if let Err(error) = reopen(&mut terminals, db, &inputs) {
        warn!("could not open the kept terminals: {error:#}");
    }
}

fn reopen(
    terminals: &mut Terminals,
    db: &mut Db,
    inputs: &InputSender,
) -> Result<()> {
    let folder = terminals.folder.clone();
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Ok(());
    };
    let mut kept = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .filter_map(|path| {
            let text = std::fs::read(&path).ok()?;
            let kept = serde_json::from_slice::<Kept>(&text).ok();
            if kept.is_none() {
                let _ = std::fs::remove_file(&path);
            }
            kept
        })
        .collect::<Vec<_>>();
    kept.sort_by(|a, b| a.view.name.cmp(&b.view.name));
    let mut agents = Vec::new();
    for agent in agents::living(db)? {
        if agents::get(db, agent)?.kind != AgentKind::Worker {
            agents.push(hamlet_protocol::client::AgentId(
                u64::try_from(agent.0)?,
            ));
        }
    }
    let hamlets = hamlets::all(db)?
        .into_iter()
        .filter(hamlets::Hamlet::is_living)
        .map(|hamlet| hamlet.id.0)
        .collect::<Vec<_>>();
    for Kept { mut view, replay } in kept {
        view.running = false;
        let gone = view.agent.is_some_and(|a| !agents.contains(&a))
            || view
                .hamlet
                .as_ref()
                .is_some_and(|h| !hamlets.contains(h));
        if gone {
            let _ = std::fs::remove_file(terminals.file(&view.id));
            continue;
        }
        let mut replay = STANDARD.decode(replay).unwrap_or_default();
        let launch = match (&view.kind, view.exit) {
            (TerminalKind::Commands, _) | (_, Some(_)) => None,
            (TerminalKind::Shell, None) => relaunch(&view),
        };
        if launch.is_some() {
            replay.extend_from_slice(RESTARTED.as_bytes());
        } else if view.exit.is_none()
            && view.kind == TerminalKind::Shell
        {
            view.exit = Some(-1);
        }
        terminals.insert(view, launch, replay, inputs);
    }
    terminals.next = terminals.open.len() as u64 + 1;
    Ok(())
}
