//! The tuning files under `engine/tuning/`, built into the engine.
//! Each section a module reads is a resource. In a debug build, or
//! with `hot-reload`, the files are watched, and a saved change
//! reaches the running engine.

use core::marker::PhantomData;

use bevy::app::{App, Plugin};
use bevy::ecs::resource::Resource;
use serde::Deserialize;
use serde::de::DeserializeOwned;

macro_rules! tuning_file {
    ($name:literal) => {
        TuningFile {
            name: $name,
            text: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tuning/",
                $name
            )),
            source: concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../tuning/",
                $name
            ),
        }
    };
}

/// Sizes, timings and budgets.
pub const ENGINE: TuningFile = tuning_file!("engine.toml");
/// The colours and sizes of the shell.
pub const THEME: TuningFile = tuning_file!("theme.toml");

/// Looks at the watched files every [`Watch::every`].
#[cfg(any(feature = "hot-reload", debug_assertions))]
pub(crate) fn plugin(app: &mut App) {
    use bevy::app::PreUpdate;
    use bevy::ecs::schedule::IntoScheduleConfigs;
    use bevy::time::common_conditions::on_real_timer;

    let every =
        core::time::Duration::from_secs_f32(Watch::designed().every);
    watch::register(app);
    app.add_systems(
        PreUpdate,
        watch::stat.run_if(on_real_timer(every)).in_set(watch::Read),
    );
}

/// Puts `T` in as designed, and while the files are watched keeps it
/// in step with its file.
pub struct TuningPlugin<T>(PhantomData<fn() -> T>);

impl<T> Default for TuningPlugin<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: Tuning> Plugin for TuningPlugin<T> {
    fn build(&self, app: &mut App) {
        app.insert_resource(T::designed());
        #[cfg(any(feature = "hot-reload", debug_assertions))]
        {
            use bevy::app::PreUpdate;
            use bevy::ecs::schedule::IntoScheduleConfigs;

            watch::register(app);
            app.world_mut()
                .resource_mut::<watch::Watched>()
                .add(T::FILE);
            app.add_systems(
                PreUpdate,
                watch::follow::<T>.after(watch::Read),
            );
        }
    }
}

/// A file under `engine/tuning/`, as it was when the engine was
/// built.
#[derive(Clone, Copy, Debug)]
pub struct TuningFile {
    /// Its name under `engine/tuning/`.
    pub name: &'static str,
    pub text: &'static str,
    /// Where it is in the source tree, which is watched.
    pub source: &'static str,
}

/// One section of a tuning file, as a resource.
pub trait Tuning:
    Resource + DeserializeOwned + Clone + PartialEq
{
    const FILE: TuningFile;
    /// Its table in the file, or `None` for the whole file.
    const SECTION: Option<&'static str>;

    /// The values as designed: the file as it was built in.
    fn designed() -> Self {
        read::<Self>(Self::FILE.text).unwrap_or_else(|why| {
            panic!("engine/tuning/{}: {why}", Self::FILE.name)
        })
    }
}

/// `T`'s section of the file `text`.
fn read<T: Tuning>(text: &str) -> Result<T, String> {
    let table = toml::from_str::<toml::Table>(text)
        .map_err(|e| e.to_string())?;
    pick::<T>(table)
}

/// `T`'s section of `table`, a whole file.
fn pick<T: Tuning>(table: toml::Table) -> Result<T, String> {
    let value = match T::SECTION {
        None => toml::Value::Table(table),
        Some(name) => table
            .get(name)
            .cloned()
            .ok_or_else(|| format!("no [{name}]"))?,
    };
    value.try_into::<T>().map_err(|e| e.to_string())
}

/// How often the watched files are looked at.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Watch {
    /// Seconds between two looks.
    pub every: f32,
}

impl Tuning for Watch {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("tuning");
}

#[cfg(any(feature = "hot-reload", debug_assertions))]
mod watch {
    use std::fs;
    use std::time::SystemTime;

    use bevy::app::App;
    use bevy::ecs::message::{Message, MessageReader, MessageWriter};
    use bevy::ecs::resource::Resource;
    use bevy::ecs::schedule::SystemSet;
    use bevy::ecs::system::{Commands, Res, ResMut};

    use super::{Tuning, TuningFile, pick};
    use crate::log::LogLine;

    /// The systems that look at the files.
    #[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct Read;

    /// The files watched, and when each was last saved.
    #[derive(Resource, Default)]
    pub struct Watched(Vec<(TuningFile, Option<SystemTime>)>);

    impl Watched {
        pub fn add(&mut self, file: TuningFile) {
            if !self
                .0
                .iter()
                .any(|(known, _)| known.name == file.name)
            {
                self.0.push((file, None));
            }
        }
    }

    /// A file read again after it was saved.
    #[derive(Message, Clone)]
    pub struct Saved {
        name: &'static str,
        table: toml::Table,
    }

    /// What watching needs, put in once whoever asks first.
    pub fn register(app: &mut App) {
        app.init_resource::<Watched>()
            .add_message::<Saved>()
            .add_message::<LogLine>();
    }

    /// Reads each file whose time moved.
    pub fn stat(
        mut watched: ResMut<Watched>,
        mut saved: MessageWriter<Saved>,
        mut log: MessageWriter<LogLine>,
    ) {
        for (file, seen) in &mut watched.0 {
            let Ok(time) = fs::metadata(file.source)
                .and_then(|meta| meta.modified())
            else {
                continue;
            };
            // The first look only learns the time.
            let first = seen.is_none();
            if seen.replace(time) == Some(time) || first {
                continue;
            }
            let table = fs::read_to_string(file.source)
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    toml::from_str::<toml::Table>(&text)
                        .map_err(|e| e.to_string())
                });
            match table {
                Ok(table) => {
                    saved.write(Saved {
                        name: file.name,
                        table,
                    });
                }
                Err(why) => {
                    log.write(LogLine::error(
                        "tuning",
                        format!(
                            "engine/tuning/{}: {why}; the last good \
                             values stay",
                            file.name
                        ),
                    ));
                }
            }
        }
    }

    /// Puts in `T` as a saved file has it, where it differs.
    pub fn follow<T: Tuning>(
        mut saved: MessageReader<Saved>,
        now: Res<T>,
        mut log: MessageWriter<LogLine>,
        mut commands: Commands,
    ) {
        let name = T::FILE.name;
        for file in saved.read().filter(|file| file.name == name) {
            match pick::<T>(file.table.clone()) {
                Ok(read) if read != *now => {
                    commands.insert_resource(read);
                    log.write(LogLine::info(
                        "tuning",
                        format!("engine/tuning/{name} read again"),
                    ));
                }
                Ok(_) => {}
                Err(why) => {
                    log.write(LogLine::error(
                        "tuning",
                        format!(
                            "engine/tuning/{name}: {why}; the last \
                             good values stay"
                        ),
                    ));
                }
            }
        }
    }
}
