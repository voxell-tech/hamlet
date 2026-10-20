//! The engine's log: what the engine and its plugins tell the
//! person, kept for the console and passed on to the terminal.

use std::collections::VecDeque;

use bevy::app::{App, Last};
use bevy::ecs::message::{Message, MessageReader};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::on_message;
use bevy::ecs::system::{Res, ResMut};
use bevy::log::{error, info, warn};
use serde::Deserialize;

use crate::tuning::{ENGINE, Tuning, TuningFile, TuningPlugin};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<LogTuning>::default())
        .add_message::<LogLine>()
        .init_resource::<Log>()
        .add_systems(Last, keep.run_if(on_message::<LogLine>));
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// One line of the log, from the part of the engine or the plugin
/// it names.
#[derive(Message, Clone, PartialEq, Debug)]
pub struct LogLine {
    pub level: LogLevel,
    pub source: String,
    pub text: String,
}

impl LogLine {
    pub fn info(
        source: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self::new(LogLevel::Info, source, text)
    }

    pub fn warn(
        source: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self::new(LogLevel::Warn, source, text)
    }

    pub fn error(
        source: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self::new(LogLevel::Error, source, text)
    }

    fn new(
        level: LogLevel,
        source: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            level,
            source: source.into(),
            text: text.into(),
        }
    }
}

/// The latest lines of the log, oldest first, each with its number
/// in the whole log.
#[derive(Resource, Default, Debug)]
pub struct Log {
    lines: VecDeque<(u64, LogLine)>,
    written: u64,
}

impl Log {
    pub fn lines(
        &self,
    ) -> impl DoubleEndedIterator<Item = (u64, &LogLine)> {
        self.lines.iter().map(|(number, line)| (*number, line))
    }
}

/// How much of the log is kept.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogTuning {
    /// The lines kept for the console; older ones are dropped.
    pub keep: usize,
}

impl Tuning for LogTuning {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("log");
}

fn keep(
    mut lines: MessageReader<LogLine>,
    tuning: Res<LogTuning>,
    mut log: ResMut<Log>,
) {
    for line in lines.read() {
        let LogLine {
            level,
            source,
            text,
        } = line;
        match level {
            LogLevel::Info => info!("{source}: {text}"),
            LogLevel::Warn => warn!("{source}: {text}"),
            LogLevel::Error => error!("{source}: {text}"),
        }
        let number = log.written;
        log.written += 1;
        log.lines.push_back((number, line.clone()));
    }
    let over = log.lines.len().saturating_sub(tuning.keep);
    log.lines.drain(..over);
}
