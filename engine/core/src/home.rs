//! The engine's home: the folder it keeps a person's files in, apart
//! from the app's, so running the engine never touches the app's
//! data.

use std::path::{Path, PathBuf};
use std::{env, fs};

use bevy::app::{App, Startup};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Res;

use crate::log::LogLine;

/// The variable that names the home, when set.
pub const HOME_VARIABLE: &str = "HAMLET_ENGINE_HOME";
/// The home's folder under the person's own, when the variable is
/// not set.
pub const HOME_FOLDER: &str = ".hamlet_engine";
/// The OS keychain service the engine keeps its secrets under, apart
/// from the app's, so neither reads or deletes the other's keys.
pub const KEYCHAIN_SERVICE: &str = "hamlet_engine";

/// Takes the home from [`HOME_VARIABLE`], unless the app put one in
/// already, and makes its folder.
pub(crate) fn plugin(app: &mut App) {
    if !app.world().contains_resource::<Home>() {
        app.insert_resource(Home::from_env());
    }
    app.add_message::<LogLine>().add_systems(Startup, make);
}

/// The home folder.
#[derive(Resource, Clone, Debug)]
pub struct Home {
    root: PathBuf,
}

impl Home {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// [`HOME_VARIABLE`] when it is set, and otherwise
    /// [`HOME_FOLDER`] in the person's own folder.
    pub fn from_env() -> Self {
        if let Some(root) = env::var_os(HOME_VARIABLE)
            && !root.is_empty()
        {
            return Self::at(root);
        }
        let mine = env::home_dir().unwrap_or_else(env::temp_dir);
        Self::at(mine.join(HOME_FOLDER))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file or folder `name` in the home.
    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

fn make(home: Res<Home>, mut log: MessageWriter<LogLine>) {
    if let Err(why) = fs::create_dir_all(home.root()) {
        log.write(LogLine::error(
            "home",
            format!(
                "{} cannot be made: {why}",
                home.root().display()
            ),
        ));
    }
}
