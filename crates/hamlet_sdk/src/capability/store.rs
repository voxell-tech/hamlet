//! `plugins/grants.toml`: the chief's answers for every plugin.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::ecs::resource::Resource;
use bevy::log::warn;

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
                std::fs::write(&self.path, text)
                    .map_err(|e| e.to_string())
            });
        if let Err(why) = written {
            warn!("plugins/{FILE}: not written: {why}");
        }
    }
}
