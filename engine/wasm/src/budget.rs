//! What each running plugin spends in a second, across every kind of
//! call: time, fuel, calls, events and log lines. A plugin past a
//! budget waits for the next second; one that goes over for
//! [`Budgets::strikes`] seconds in a row is stopped.

use std::time::Duration;

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::time::{Real, Time};
use hamlet_engine_core::{Budgets, LogLine};
use hamlet_engine_plugins::PluginInfo;

use crate::instance::{Fault, Instance, Spent};
use crate::panel::stop;

/// On a running plugin: what it spent in the current second.
#[derive(Component, Default, Debug)]
pub(crate) struct Spending {
    /// When the second began, on the real clock.
    since: Duration,
    time: Duration,
    fuel: u64,
    calls: u32,
    events: u32,
    /// Lines logged, by level.
    lines: [u32; 3],
    /// Lines and events left out.
    dropped_lines: u32,
    pub(crate) dropped_events: u32,
    /// Whether it went past a budget.
    over: bool,
    /// Seconds in a row it went past one.
    strikes: u32,
    /// The frame it was last served in.
    pub(crate) served: u64,
}

impl Spending {
    pub(crate) fn new(now: Duration) -> Self {
        Self {
            since: now,
            ..Self::default()
        }
    }

    /// Whether the plugin may be called again this second, with an
    /// event when `event`. A plugin that may not is over a budget.
    pub(crate) fn may_call(
        &mut self,
        budgets: &Budgets,
        event: bool,
    ) -> bool {
        let within = self.time.as_secs_f32() * 1000.0
            < budgets.time_per_second
            && self.fuel < budgets.fuel_per_second
            && self.calls < budgets.calls_per_second
            && (!event || self.events < budgets.events_per_second);
        self.over |= !within;
        within
    }

    /// Counts a call, given an event when `event`.
    pub(crate) fn charge(&mut self, spent: Spent, event: bool) {
        self.time += spent.time;
        self.fuel = self.fuel.saturating_add(spent.fuel);
        self.calls += 1;
        self.events += u32::from(event);
    }
}

/// Tells the log what the plugin `info` logged, as many lines of each
/// level a second as the budgets allow.
pub(crate) fn pass_logs(
    info: &PluginInfo,
    instance: &mut Instance,
    spending: &mut Spending,
    budgets: &Budgets,
    log: &mut MessageWriter<LogLine>,
) {
    for (level, text) in instance.take_logs() {
        let lines = &mut spending.lines[level as usize];
        if *lines >= budgets.log_lines_per_second {
            spending.dropped_lines += 1;
            continue;
        }
        *lines += 1;
        log.write(LogLine {
            level,
            source: info.id.to_string(),
            text,
        });
    }
}

/// Starts each plugin's next second when its last is over, telling
/// what was left out, and stops a plugin that went over its budgets
/// too many seconds in a row.
pub(crate) fn tally(
    mut plugins: Query<
        (Entity, &PluginInfo, &mut Spending),
        With<Instance>,
    >,
    budgets: Res<Budgets>,
    time: Res<Time<Real>>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let now = time.elapsed();
    for (plugin, info, mut spending) in &mut plugins {
        if now.saturating_sub(spending.since) < Duration::from_secs(1)
        {
            continue;
        }
        let source = info.id.as_str();
        for (count, what) in [
            (spending.dropped_lines, "lines of its log"),
            (spending.dropped_events, "events"),
        ] {
            if count > 0 {
                log.write(LogLine::warn(
                    source,
                    format!(
                        "{count} {what} were left out in a second"
                    ),
                ));
            }
        }
        let strikes = if spending.over {
            spending.strikes + 1
        } else {
            0
        };
        if strikes >= budgets.strikes {
            let fault = Fault::Over(strikes);
            stop(&mut commands, &mut log, plugin, info, &fault);
            continue;
        }
        if strikes == 1 {
            log.write(LogLine::warn(
                source,
                format!(
                    "{} is over its budget, and waits",
                    info.name
                ),
            ));
        }
        *spending = Spending {
            since: now,
            strikes,
            served: spending.served,
            ..Spending::default()
        };
    }
}
