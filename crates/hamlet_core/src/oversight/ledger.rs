//! The record of what waited for a chief and how it ended, kept in
//! `oversight.jsonl` in the home.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use bevy_ecs::prelude::Resource;
use hamlet_model::log;
use serde_json::{Value, json};

/// Something that was still waiting when the record was last
/// written.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Unsettled {
    pub id: String,
    pub kind: String,
    pub by: String,
    pub about: String,
}

/// The things raised in `events` and never settled, oldest first.
pub(crate) fn unsettled(events: &[Value]) -> Vec<Unsettled> {
    let mut open = Vec::<Unsettled>::new();
    let text = |e: &Value, key: &str| {
        e[key].as_str().unwrap_or_default().to_string()
    };
    for e in events {
        match e["type"].as_str() {
            Some("raised") => open.push(Unsettled {
                id: text(e, "id"),
                kind: text(e, "kind"),
                by: text(e, "by"),
                about: text(e, "about"),
            }),
            Some("settled") => {
                open.retain(|u| e["id"] != u.id.as_str())
            }
            _ => {}
        }
    }
    open
}

/// The record of what waited, in `oversight.jsonl`.
#[derive(Resource)]
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub fn open(root: &Path) -> Self {
        Ledger {
            path: root.join("oversight.jsonl"),
        }
    }

    fn append(&self, event: Value) -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{event}")?;
        Ok(())
    }

    pub fn events(&self) -> Vec<Value> {
        log::read_events(&self.path).unwrap_or_default()
    }

    pub fn raised(
        &self,
        id: &str,
        kind: &str,
        by: &str,
        agent: Option<u64>,
        about: &str,
    ) -> Result<()> {
        self.append(json!({
            "v": 1,
            "type": "raised",
            "ts": log::now_millis(),
            "id": id,
            "kind": kind,
            "by": by,
            "agent": agent,
            "about": about,
        }))
    }

    /// Notes how `id` ended: the answer and who gave it, or that
    /// nobody did.
    pub fn settled(
        &self,
        id: &str,
        how: &str,
        text: &str,
        chief: Option<&str>,
    ) -> Result<()> {
        self.append(json!({
            "v": 1,
            "type": "settled",
            "ts": log::now_millis(),
            "id": id,
            "how": how,
            "text": text,
            "chief": chief,
        }))
    }
}
