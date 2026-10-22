//! How loud each category is: the engine's own volumes in `[sound]`,
//! and the person's in `sound.toml` in the home, written whole once
//! they stay still. A file that cannot be read is never written over.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use bevy::app::{App, AppExit, Last, Startup};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::time::Time;
use hamlet_engine_api::PluginId;
use hamlet_engine_api::audio::Category;
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use hamlet_engine_core::{Home, LogLine, TuningPlugin, write_whole};
use serde::{Deserialize, Serialize};

/// The file's name in the home.
pub const FILE: &str = "sound.toml";

pub(crate) fn plugin(app: &mut App) {
    let home = app.world().resource::<Home>().clone();
    let (file, settings) = SoundFile::open(&home);
    app.add_plugins(TuningPlugin::<SoundTuning>::default())
        .insert_resource(file)
        .insert_resource(settings)
        .add_systems(Startup, tell_held)
        .add_systems(Last, save);
}

/// The engine's own volumes, under the person's, and how placed
/// sounds are heard.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundTuning {
    /// The volume of every sound, 0 to 1.
    pub master: f32,
    /// The volume of each category, 0 to 1, under `master`.
    pub interface: f32,
    pub world: f32,
    pub music: f32,
    pub voices: f32,
    /// Distance from the listener within which a placed sound is at
    /// its full volume. Beyond it the sound falls with the square of
    /// the distance.
    pub reach: f32,
    /// Distance between the listener's ears.
    pub ears: f32,
    /// Seconds the person's settings stay still before they are
    /// saved.
    pub save_after: f32,
}

impl Tuning for SoundTuning {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("sound");

    fn check(&self) -> Result<(), String> {
        let sizes = [("reach", self.reach), ("ears", self.ears)];
        match sizes.iter().find(|(_, x)| !(x.is_finite() && *x > 0.0))
        {
            Some((name, _)) => {
                Err(format!("{name} is not a number above zero"))
            }
            None => Ok(()),
        }
    }
}

impl SoundTuning {
    /// The engine's volume of `category`, the master's included.
    pub fn volume(&self, category: Category) -> f32 {
        let level = match category {
            Category::Interface => self.interface,
            Category::World => self.world,
            Category::Music => self.music,
            Category::Voices => self.voices,
        };
        self.master * level
    }
}

/// A volume the person sets: a switch and a level from 0 to 1.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub on: bool,
    pub level: f32,
}

impl Default for Level {
    fn default() -> Self {
        Self {
            on: true,
            level: 1.0,
        }
    }
}

impl Level {
    /// The share of a sound heard: none while it is off.
    pub fn heard(self) -> f32 {
        if self.on && self.level.is_finite() {
            self.level.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// The person's volumes, and the plugins they muted.
#[derive(
    Resource, Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
#[serde(default, deny_unknown_fields)]
pub struct SoundSettings {
    pub master: Level,
    pub interface: Level,
    pub world: Level,
    pub music: Level,
    pub voices: Level,
    pub muted: BTreeSet<PluginId>,
}

impl SoundSettings {
    pub fn of(&self, category: Category) -> &Level {
        match category {
            Category::Interface => &self.interface,
            Category::World => &self.world,
            Category::Music => &self.music,
            Category::Voices => &self.voices,
        }
    }

    pub fn of_mut(&mut self, category: Category) -> &mut Level {
        match category {
            Category::Interface => &mut self.interface,
            Category::World => &mut self.world,
            Category::Music => &mut self.music,
            Category::Voices => &mut self.voices,
        }
    }

    /// The person's share of `category` heard, the master's
    /// included.
    pub fn volume(&self, category: Category) -> f32 {
        self.master.heard() * self.of(category).heard()
    }
}

/// On a plugin whose sounds the person muted.
#[derive(Component, Clone, Copy, Debug)]
pub struct Muted;

/// Where [`SoundSettings`] are kept, and whether they wait to be
/// saved.
#[derive(Resource, Debug)]
pub struct SoundFile {
    path: PathBuf,
    /// Why the file could not be read. It is left as it is, and
    /// nothing is written.
    held: Option<String>,
    /// Seconds the settings have been still since they changed.
    still: Option<f32>,
}

impl SoundFile {
    /// The file of `home`, and the settings it holds. A missing file
    /// holds the defaults.
    pub fn open(home: &Home) -> (Self, SoundSettings) {
        let path = home.path(FILE);
        let read = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<SoundSettings>(&text)
                .map_err(|e| e.message().to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(SoundSettings::default())
            }
            Err(e) => Err(e.to_string()),
        };
        let (settings, held) = match read {
            Ok(settings) => (settings, None),
            Err(why) => (SoundSettings::default(), Some(why)),
        };
        let file = Self {
            path,
            held,
            still: None,
        };
        (file, settings)
    }

    /// Writes `settings`, unless the file could not be read. The
    /// error says why it cannot be written.
    pub fn write(
        &self,
        settings: &SoundSettings,
    ) -> Result<(), String> {
        if self.held.is_some() {
            return Ok(());
        }
        let unwritten =
            |why: String| format!("{FILE} cannot be written: {why}");
        let text = toml::to_string(settings)
            .map_err(|e| unwritten(e.to_string()))?;
        write_whole(&self.path, text)
            .map_err(|e| unwritten(e.to_string()))
    }
}

/// Saves the settings once they stayed still for
/// [`SoundTuning::save_after`], and as the engine closes.
fn save(
    settings: Res<SoundSettings>,
    tuning: Res<SoundTuning>,
    time: Res<Time>,
    mut file: ResMut<SoundFile>,
    mut exit: MessageReader<AppExit>,
    mut log: MessageWriter<LogLine>,
) {
    if settings.is_changed() && !settings.is_added() {
        file.still = Some(0.0);
    }
    let closing = exit.read().count() > 0;
    let Some(still) = file.still.as_mut() else {
        return;
    };
    *still += time.delta_secs();
    if *still < tuning.save_after && !closing {
        return;
    }
    file.still = None;
    if let Err(why) = file.write(&settings) {
        log.write(LogLine::error("sound", why));
    }
}

fn tell_held(file: Res<SoundFile>, mut log: MessageWriter<LogLine>) {
    if let Some(why) = &file.held {
        log.write(LogLine::error(
            "sound",
            format!(
                "{FILE} cannot be read ({why}). It is left as it \
                 is, and the volumes are kept only until the engine \
                 closes."
            ),
        ));
    }
}
