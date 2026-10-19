//! `plugins/grants.toml`: the chief's answers for every plugin.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res};
use bevy::log::warn;
use hamlet_base::HomeDir;

use super::record::Record;

const FILE: &str = "grants.toml";

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    plugin: BTreeMap<String, Record>,
}

/// The records of `grants.toml`, kept in step with the file. A file
/// that cannot be read is left as it is and nothing is written over
/// it.
#[derive(Resource, Debug)]
pub struct Store {
    path: PathBuf,
    records: BTreeMap<String, Record>,
    locked: bool,
}

/// Opens the records of the home, once, for whoever holds plugins
/// to read and write them. Those systems run after it.
pub fn open_store(
    home: Option<Res<HomeDir>>,
    mut commands: Commands,
) {
    if let Some(home) = home {
        commands.insert_resource(Store::open(&home.0.plugins()));
    }
}

impl Store {
    /// The records in the install folder `plugins`. A missing file
    /// holds none.
    pub fn open(plugins: &Path) -> Self {
        let path = plugins.join(FILE);
        let (records, locked) = match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<File>(&text) {
                Ok(file) => (file.plugin, false),
                Err(e) => {
                    warn!(
                        "plugins/{FILE}: {}; no plugin is installed \
                         and nothing is written over it",
                        e.message()
                    );
                    (BTreeMap::new(), true)
                }
            },
            Err(_) => (BTreeMap::new(), false),
        };
        Self {
            path,
            records,
            locked,
        }
    }

    pub fn get(&self, id: &str) -> Option<&Record> {
        self.records.get(id)
    }

    /// Keeps `record` for the plugin `id` and writes the file.
    pub fn set(&mut self, id: &str, record: Record) {
        if self.records.get(id) == Some(&record) {
            return;
        }
        self.records.insert(id.into(), record);
        self.save();
    }

    /// Forgets the plugin `id` and writes the file.
    pub fn remove(&mut self, id: &str) {
        if self.records.remove(id).is_some() {
            self.save();
        }
    }

    fn save(&self) {
        if self.locked {
            return;
        }
        let file = File {
            plugin: self.records.clone(),
        };
        let written = toml::to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                if let Some(folder) = self.path.parent() {
                    std::fs::create_dir_all(folder)
                        .map_err(|e| e.to_string())?;
                }
                write_whole(&self.path, &text)
                    .map_err(|e| e.to_string())
            });
        if let Err(why) = written {
            warn!("plugins/{FILE}: not written: {why}");
        }
    }
}

/// Writes `text` to a file beside `path` and renames it over `path`,
/// so that a stop in the middle leaves the old file or the new one.
fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    let part = path.with_extension("toml.part");
    let mut file = std::fs::File::create(&part)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&part, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_leaves_the_file_whole_and_nothing_beside_it() {
        let folder = std::env::temp_dir()
            .join(format!("hamlet_store_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut store = Store::open(&folder);
        store
            .set("p", Record::declined("1.0.0", &Default::default()));
        store
            .set("q", Record::declined("2.0.0", &Default::default()));
        let names = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, [FILE]);
        assert!(Store::open(&folder).get("q").is_some());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
