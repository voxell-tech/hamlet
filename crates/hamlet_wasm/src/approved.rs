//! `plugins/approved.toml`: the ids of the installed plugins that may
//! load.

use std::path::Path;

use bevy::log::warn;

const FILE: &str = "approved.toml";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Approved {
    plugins: Vec<String>,
}

/// The ids in the install folder's list. A missing or unreadable
/// list approves nothing.
pub fn ids(plugins: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(plugins.join(FILE)) else {
        return Vec::new();
    };
    match toml::from_str::<Approved>(&text) {
        Ok(approved) => approved.plugins,
        Err(e) => {
            warn!("plugins/{FILE}: {}", e.message());
            Vec::new()
        }
    }
}
