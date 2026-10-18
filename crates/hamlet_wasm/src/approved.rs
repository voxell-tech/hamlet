//! `plugins/approved.toml`: the installed plugins that may load, and
//! what each is granted.
//!
//! This file stands in for the chief's answer to a petition. A
//! plugin is a bare id, or a table with the grants:
//!
//! ```toml
//! plugins = ["hello", { id = "roster", grants = ["read.agents"] }]
//! ```

use std::path::Path;

use bevy::log::warn;

const FILE: &str = "approved.toml";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Approved {
    plugins: Vec<Entry>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Entry {
    Id(String),
    Granted {
        id: String,
        #[serde(default)]
        grants: Vec<String>,
    },
}

/// A plugin the chief approved, and what it may do.
#[derive(Clone, PartialEq, Debug)]
pub struct Approval {
    pub id: String,
    pub grants: Vec<String>,
}

impl From<Entry> for Approval {
    fn from(entry: Entry) -> Self {
        match entry {
            Entry::Id(id) => Self {
                id,
                grants: Vec::new(),
            },
            Entry::Granted { id, grants } => Self { id, grants },
        }
    }
}

/// The plugins in the install folder's list. A missing or unreadable
/// list approves nothing.
pub fn list(plugins: &Path) -> Vec<Approval> {
    let Ok(text) = std::fs::read_to_string(plugins.join(FILE)) else {
        return Vec::new();
    };
    parse(&text)
}

fn parse(text: &str) -> Vec<Approval> {
    match toml::from_str::<Approved>(text) {
        Ok(approved) => {
            approved.plugins.into_iter().map(Approval::from).collect()
        }
        Err(e) => {
            warn!("plugins/{FILE}: {}", e.message());
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_is_a_bare_id_or_an_id_with_grants() {
        let approved = parse(
            "plugins = [\"hello\", { id = \"roster\", grants = \
             [\"read.agents\"] }]",
        );
        assert_eq!(
            approved,
            [
                Approval {
                    id: "hello".into(),
                    grants: Vec::new()
                },
                Approval {
                    id: "roster".into(),
                    grants: vec!["read.agents".into()]
                },
            ]
        );
    }
}
