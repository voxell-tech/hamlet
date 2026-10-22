//! `bought.json`: what was bought at the bazaar and written into the
//! home, by listing, at which version and under which names.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use hamlet_model::market::package::Kind;
use serde::{Deserialize, Serialize};

/// What was bought and written into the home, by listing id.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
pub struct Kept(pub BTreeMap<String, Keeping>);

/// One listing bought and written into the home.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keeping {
    pub kind: Kind,
    pub title: String,
    /// The version written.
    pub version: String,
    /// The names it lives under: the agent's, the hamlet's or the
    /// tools'.
    pub names: Vec<String>,
    /// The hamlet an agent was moved into, or of the agent whose
    /// house the tools were added to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hamlet: Option<String>,
    /// The agent whose house the tools were added to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub house: Option<String>,
}

impl Kept {
    pub fn path(home: &Path) -> PathBuf {
        home.join("bought.json")
    }

    /// The record, or none when there is no file or it cannot be
    /// read.
    pub fn load(home: &Path) -> Kept {
        std::fs::read_to_string(Kept::path(home))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, home: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        let path = Kept::path(home);
        let fresh = path.with_extension("json.new");
        std::fs::write(&fresh, text)?;
        std::fs::rename(fresh, path)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Keeping> {
        self.0.get(id)
    }
}
