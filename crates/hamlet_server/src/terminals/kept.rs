//! The terminals kept across restarts, under the home's `terminals/`.

use std::path::PathBuf;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_protocol::client::{TerminalKind, TerminalView};
use serde::{Deserialize, Serialize};

use super::Terminals;
use super::screen::lock;
use super::shell::{reach_of, sandboxed, unbounded};
use crate::service::Service;

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
            crate::diagnostic::tell(
                crate::diagnostic::Secrets::Fixed,
                format_args!("could not keep a terminal: {error}"),
            );
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
/// one ran, below what the old one printed.
pub(crate) fn restore(service: &mut Service) -> Result<()> {
    let folder = service.terminals.folder.clone();
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
    for Kept { mut view, replay } in kept {
        let mut replay = STANDARD.decode(replay).unwrap_or_default();
        let launch = match (&view.kind, view.exit, &view.hamlet) {
            (TerminalKind::Commands, ..) | (_, Some(_), _) => None,
            (TerminalKind::Shell, None, None) => Some(unbounded()),
            (TerminalKind::Shell, None, Some(hamlet)) => {
                let record = service.home.store().record(hamlet)?;
                let live = record.filter(|record| {
                    view.agent.as_ref().is_none_or(|agent| {
                        record
                            .agents
                            .iter()
                            .any(|a| a.number == agent.number)
                    })
                });
                let Some(record) = live else {
                    let _ = std::fs::remove_file(
                        service.terminals.file(&view.id),
                    );
                    continue;
                };
                let reach = reach_of(&record, view.agent.as_ref());
                sandboxed(service, &reach, view.agent.is_none())?.ok()
            }
        };
        if launch.is_some() {
            replay.extend_from_slice(RESTARTED.as_bytes());
        } else if view.exit.is_none()
            && view.kind == TerminalKind::Shell
        {
            view.exit = Some(-1);
        }
        service.terminals.insert(view, launch, replay);
    }
    service.terminals.next = service.terminals.open.len() as u64 + 1;
    Ok(())
}
