//! The workspaces file, `workspaces.ron` in the home. A file that
//! cannot be read is moved aside, never written over.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use bevy::ecs::message::MessageWriter;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::time::{Real, Time};
use hamlet_engine_core::{Home, LogLine, write_whole};
use ron::ser::PrettyConfig;

use super::{WorkspaceTuning, Workspaces};

/// The file's name in the home.
pub const FILE: &str = "workspaces.ron";

/// What became of reading the file.
#[derive(Debug)]
pub enum Restored {
    Read(Workspaces),
    /// No file yet.
    Fresh,
    /// A file that could not be read, now at `kept`.
    KeptAside {
        why: String,
        kept: PathBuf,
    },
    /// A file that could not be read nor moved: nothing is saved, so
    /// it stays as it is.
    Held {
        why: String,
    },
}

/// Whether the workspaces changed since they were saved, and since
/// when.
#[derive(Resource, Default)]
pub struct Unsaved {
    since: Option<Duration>,
    /// Saving is off: the file could not be read and is still there.
    held: bool,
    /// The workspaces as last read or saved.
    stored: Workspaces,
}

impl Unsaved {
    pub fn pending(&self) -> bool {
        self.since.is_some()
    }

    /// Writes `workspaces` to `home`, unless saving is off.
    fn store(
        &mut self,
        home: &Home,
        workspaces: &Workspaces,
        log: &mut MessageWriter<LogLine>,
    ) {
        self.since = None;
        if self.held {
            return;
        }
        match workspaces.write(home) {
            Ok(()) => self.stored.clone_from(workspaces),
            Err(why) => {
                log.write(LogLine::error(
                    "workspaces",
                    format!("{FILE} could not be saved: {why}"),
                ));
            }
        }
    }
}

impl Restored {
    /// Reads the workspaces in `home`.
    pub fn read(home: &Home) -> Self {
        let path = home.path(FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Self::Fresh;
            }
            Err(e) => return Self::Held { why: e.to_string() },
        };
        let why = match ron::from_str::<Workspaces>(&text) {
            Ok(workspaces) => {
                return Self::Read(workspaces.cleaned());
            }
            Err(e) => e.to_string(),
        };
        let kept = (1..)
            .map(|n| match n {
                1 => home.path("workspaces_broken.ron"),
                n => home.path(&format!("workspaces_broken_{n}.ron")),
            })
            .find(|kept| !kept.exists())
            .unwrap_or_else(|| home.path("workspaces_broken.ron"));
        match fs::rename(&path, &kept) {
            Ok(()) => Self::KeptAside { why, kept },
            Err(e) => Self::Held {
                why: format!(
                    "{why}; and it could not be moved aside: {e}"
                ),
            },
        }
    }
}

impl Workspaces {
    /// Writes the workspaces to `home`, whole or not at all.
    pub fn write(&self, home: &Home) -> Result<(), String> {
        let text =
            ron::ser::to_string_pretty(self, PrettyConfig::default())
                .map_err(|e| e.to_string())?;
        write_whole(&home.path(FILE), text).map_err(|e| e.to_string())
    }
}

/// Puts in the workspaces the home holds.
pub fn restore(
    home: Res<Home>,
    mut unsaved: ResMut<Unsaved>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let workspaces = match Restored::read(&home) {
        Restored::Read(workspaces) => workspaces,
        Restored::Fresh => Workspaces::default(),
        Restored::KeptAside { why, kept } => {
            log.write(LogLine::error(
                "workspaces",
                format!(
                    "{FILE} could not be read ({why}). It is kept \
                     as {}, and the workspaces start afresh.",
                    kept.display()
                ),
            ));
            Workspaces::default()
        }
        Restored::Held { why } => {
            log.write(LogLine::error(
                "workspaces",
                format!(
                    "{FILE} could not be read ({why}). The \
                     workspaces start afresh and are not saved, so \
                     the file stays as it is."
                ),
            ));
            unsaved.held = true;
            Workspaces::default()
        }
    };
    unsaved.stored.clone_from(&workspaces);
    commands.insert_resource(workspaces);
}

/// Notes a change, to be saved once the workspaces are still. A
/// change back to what is stored is not one.
pub fn note(
    time: Res<Time<Real>>,
    workspaces: Res<Workspaces>,
    mut unsaved: ResMut<Unsaved>,
) {
    unsaved.since =
        (*workspaces != unsaved.stored).then(|| time.elapsed());
}

/// Saves the workspaces once they have been still for
/// [`WorkspaceTuning::save_after`].
pub fn save(
    time: Res<Time<Real>>,
    tuning: Res<WorkspaceTuning>,
    home: Res<Home>,
    workspaces: Res<Workspaces>,
    mut unsaved: ResMut<Unsaved>,
    mut log: MessageWriter<LogLine>,
) {
    let Some(since) = unsaved.since else {
        return;
    };
    if (time.elapsed() - since).as_secs_f32() < tuning.save_after {
        return;
    }
    unsaved.store(&home, &workspaces, &mut log);
}

/// Saves what is not saved yet, as the engine closes.
pub fn save_now(
    home: Res<Home>,
    workspaces: Res<Workspaces>,
    mut unsaved: ResMut<Unsaved>,
    mut log: MessageWriter<LogLine>,
) {
    if unsaved.pending() {
        unsaved.store(&home, &workspaces, &mut log);
    }
}
