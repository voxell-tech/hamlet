//! The plugins the person switched off, kept in `switched_off.toml`
//! in the home so they stay off at the next start. It is written
//! whole or not at all, and a file that cannot be read is never
//! written over.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use bevy::app::{App, Startup};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use hamlet_engine_api::PluginId;
use hamlet_engine_core::{Home, LogLine, write_whole};
use serde::{Deserialize, Serialize};

use crate::PluginInfo;
use crate::list::Uninstall;

/// The file's name in the home.
pub(crate) const FILE: &str = "switched_off.toml";

pub(crate) fn plugin(app: &mut App) {
    let home = app.world().resource::<Home>().clone();
    app.insert_resource(SwitchedOff::open(&home))
        .add_observer(forget)
        .add_systems(Startup, tell_held);
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SwitchedOffFile {
    #[serde(default)]
    plugins: BTreeSet<PluginId>,
}

/// The ids of the plugins the person switched off, as the file keeps
/// them.
#[derive(Resource, Debug)]
pub struct SwitchedOff {
    path: PathBuf,
    ids: BTreeSet<PluginId>,
    /// Why the file could not be read. It is left as it is, and
    /// nothing is written.
    held: Option<String>,
}

impl SwitchedOff {
    /// The plugins switched off in `home`. A missing file holds none.
    pub fn open(home: &Home) -> Self {
        let path = home.path(FILE);
        let read = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<SwitchedOffFile>(&text)
                .map_err(|e| e.message().to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(SwitchedOffFile::default())
            }
            Err(e) => Err(e.to_string()),
        };
        let (ids, held) = match read {
            Ok(file) => (file.plugins, None),
            Err(why) => (BTreeSet::new(), Some(why)),
        };
        Self { path, ids, held }
    }

    /// Why the file could not be read, while it is left as it is.
    pub fn held(&self) -> Option<&str> {
        self.held.as_deref()
    }

    pub fn contains(&self, id: &PluginId) -> bool {
        self.ids.contains(id)
    }

    /// Keeps the plugin `id` off, or not, and writes the file. The
    /// error says why it cannot be written.
    pub fn set(
        &mut self,
        id: &PluginId,
        off: bool,
    ) -> Result<(), String> {
        let changed = if off {
            self.ids.insert(id.clone())
        } else {
            self.ids.remove(id)
        };
        if !changed || self.held.is_some() {
            return Ok(());
        }
        let file = SwitchedOffFile {
            plugins: self.ids.clone(),
        };
        let unwritten =
            |why: String| format!("{FILE} cannot be written: {why}");
        let text = toml::to_string(&file)
            .map_err(|e| unwritten(e.to_string()))?;
        write_whole(&self.path, text)
            .map_err(|e| unwritten(e.to_string()))
    }
}

/// Takes a plugin being uninstalled off the list.
fn forget(
    event: On<Uninstall>,
    plugins: Query<&PluginInfo>,
    mut off: ResMut<SwitchedOff>,
    mut log: MessageWriter<LogLine>,
) {
    let Ok(info) = plugins.get(event.entity) else {
        return;
    };
    if let Err(why) = off.set(&info.id, false) {
        log.write(LogLine::error("plugins", why));
    }
}

fn tell_held(off: Res<SwitchedOff>, mut log: MessageWriter<LogLine>) {
    if let Some(why) = off.held() {
        log.write(LogLine::error(
            "plugins",
            format!(
                "{FILE} cannot be read ({why}). It is left as it \
                 is, and no plugin is kept off."
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_is_kept_and_an_unreadable_file_stays() {
        let root = std::env::temp_dir().join(format!(
            "hamlet_engine_test_switched_off_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let home = Home::at(&root);
        let id = PluginId::try_from("hamlet.demo").unwrap();

        SwitchedOff::open(&home).set(&id, true).unwrap();
        let again = SwitchedOff::open(&home);
        assert_eq!(again.held(), None);
        assert!(again.contains(&id));

        let broken = "plugins = [\"hamlet.demo\"\n";
        fs::write(home.path(FILE), broken).unwrap();
        let mut off = SwitchedOff::open(&home);
        assert!(off.held().is_some());
        assert!(!off.contains(&id));
        off.set(&id, true).unwrap();
        off.set(&id, false).unwrap();
        assert_eq!(
            fs::read_to_string(home.path(FILE)).unwrap(),
            broken
        );
        let _ = fs::remove_dir_all(&root);
    }
}
