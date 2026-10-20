//! Adding a plugin from a folder the chief picks.

use std::path::Path;

use bevy::ecs::event::Event;
use bevy::ecs::resource::Resource;

/// The chief wants to add a plugin from a folder of their own.
#[derive(Event, Clone, Copy, PartialEq, Eq, Debug)]
pub struct AddPlugin;

/// The chief's answer to the question of [`Adding::Asking`]: go on,
/// or leave it.
#[derive(Event, Clone, Copy, PartialEq, Eq, Debug)]
pub struct AddAnswer {
    pub go: bool,
}

/// How adding a plugin from a folder goes, as the Plugins page shows
/// it. Whoever adds plugins keeps it.
#[derive(Resource, Clone, PartialEq, Eq, Default, Debug)]
pub enum Adding {
    /// Nothing is under way.
    #[default]
    Idle,
    /// The dialog is open.
    Choosing,
    /// The folder chosen is read.
    Reading,
    /// The folder waits for the chief to go on.
    Asking(Question),
    /// The plugin of this name is built: the last line the build
    /// wrote.
    Building { name: String, line: String },
    /// The plugin is in the install folder: what becomes of it.
    Added(String),
    /// The folder was not added: why, and what the build wrote where
    /// there was one.
    Refused { why: String, log: String },
}

impl Adding {
    /// Whether the chief may start adding another.
    pub fn free(&self) -> bool {
        matches!(
            self,
            Adding::Idle | Adding::Added(_) | Adding::Refused { .. }
        )
    }
}

/// What the chief is asked before a folder is added.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Question {
    pub name: String,
    pub version: String,
    /// The folder holds the plugin's source, which is built first.
    pub build: bool,
    /// The plugin it takes the place of, by its name and version.
    pub replaces: Option<String>,
    /// It is a native plugin.
    pub native: bool,
}

/// What a version of a plugin is made of beside its manifest: its id,
/// name and version, and each file by its name in the folder with the
/// most bytes it may have.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Contents {
    pub id: String,
    pub name: String,
    pub version: String,
    pub files: Vec<(String, u64)>,
}

/// Reads the folder of a native plugin, whose manifest is the text
/// given: what it is made of, or why it is not one. `own` says the
/// manifest's id must be the folder's name, as in the install folder.
pub type ReadNative =
    fn(&Path, &str, bool) -> Result<Contents, String>;

/// The reading of a native plugin's folder, which the host of native
/// plugins puts in.
#[derive(Resource, Clone, Copy)]
pub struct NativeFolders(pub ReadNative);
