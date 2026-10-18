//! How often the files of the installed plugins are looked at. The
//! value is in `assets/wasm.toml`.

use std::time::Duration;

use bevy::prelude::*;

/// How the files of the installed plugins are watched.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Polling {
    /// The time between two looks at the files of the installed
    /// plugins, in milliseconds.
    pub poll_ms: u64,
}

impl Polling {
    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.poll_ms)
    }
}
