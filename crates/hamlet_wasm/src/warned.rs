//! Warnings about what a plugin keeps doing, logged once a minute for
//! each plugin and kind so that a plugin cannot flood the log.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bevy::log::warn;

/// How long a warning of one kind stays out of the log once logged.
const QUIET: Duration = Duration::from_secs(60);

/// When a warning of each plugin and kind was last logged.
#[derive(Default)]
pub struct Warned(HashMap<(String, String), Instant>);

impl Warned {
    /// Logs `why` for the plugin `plugin`, unless a warning of `kind`
    /// was logged for it in the last minute.
    pub fn warn(&mut self, plugin: &str, kind: &str, why: &str) {
        let now = Instant::now();
        let key = (plugin.to_string(), kind.to_string());
        if self
            .0
            .get(&key)
            .is_some_and(|last| now.duration_since(*last) < QUIET)
        {
            return;
        }
        self.0.insert(key, now);
        warn!("plugin {plugin}: {why}");
    }
}
