//! The time each running plugin's calls take in a second. A plugin
//! the person gave a CPU time a second waits, once past it, for the
//! next second; any other is never held back.

use std::time::Duration;

use bevy::ecs::component::Component;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::query::With;
use bevy::ecs::system::{Query, Res};
use bevy::time::{Real, Time};
use hamlet_engine_core::{ActiveLimits, LogLine};
use hamlet_engine_plugins::PluginInfo;

use crate::instance::{Instance, Spent};

/// On a running plugin: the time its calls took in the current
/// second.
#[derive(Component, Default, Debug)]
pub(crate) struct Spending {
    /// When the second began, on the real clock.
    since: Duration,
    time: Duration,
    /// Whether it went past its CPU time this second, and the second
    /// before.
    over: bool,
    was_over: bool,
}

impl Spending {
    pub(crate) fn new(now: Duration) -> Self {
        Self {
            since: now,
            ..Self::default()
        }
    }

    /// Whether the plugin may be called again this second: always,
    /// unless the person gave it a CPU time and it is past it.
    pub(crate) fn may_call(&mut self, limits: &ActiveLimits) -> bool {
        let within =
            limits.0.cpu_time_per_second.is_none_or(|most| {
                self.time < Duration::from_millis(most)
            });
        self.over |= !within;
        within
    }

    /// Counts a call.
    pub(crate) fn charge(&mut self, spent: Spent) {
        self.time = self.time.saturating_add(spent.time);
    }
}

impl Instance {
    /// Tells the log what the plugin `info` logged.
    pub(crate) fn pass_logs(
        &mut self,
        info: &PluginInfo,
        log: &mut MessageWriter<LogLine>,
    ) {
        for (level, text) in self.take_logs() {
            log.write(LogLine {
                level,
                source: info.id.to_string(),
                text,
            });
        }
    }
}

/// Starts each plugin's next second when its last is over, telling
/// the log when one first waits for its CPU time.
pub(crate) fn tally(
    mut plugins: Query<(&PluginInfo, &mut Spending), With<Instance>>,
    time: Res<Time<Real>>,
    mut log: MessageWriter<LogLine>,
) {
    let now = time.elapsed();
    for (info, mut spending) in &mut plugins {
        if now.saturating_sub(spending.since) < Duration::from_secs(1)
        {
            continue;
        }
        if spending.over && !spending.was_over {
            log.write(LogLine::warn(
                info.id.as_str(),
                format!(
                    "{} is past the CPU time a second you set, and \
                     waits",
                    info.name
                ),
            ));
        }
        *spending = Spending {
            since: now,
            was_over: spending.over,
            ..Spending::default()
        };
    }
}
