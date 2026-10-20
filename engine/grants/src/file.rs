//! The grants file, `grants.toml` in the home: the person's answers
//! for each installed plugin. It is written whole or not at all, and
//! a file that cannot be read is never written over.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use bevy::ecs::resource::Resource;
use hamlet_engine_api::{Capability, PluginId};
use hamlet_engine_core::{Home, write_whole};
use serde::{Deserialize, Serialize};

/// The file's name in the home.
pub const FILE: &str = "grants.toml";

/// The person's answer to an ask.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Allow,
    /// Ask next time it is used.
    Ask,
    /// Don't allow.
    Deny,
}

impl Answer {
    /// Every answer, in the order the person is offered them.
    pub const ALL: [Answer; 3] =
        [Answer::Allow, Answer::Ask, Answer::Deny];

    /// What the person is shown.
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Ask => "Ask next time",
            Self::Deny => "Don't allow",
        }
    }
}

/// The answer to one ask.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answered {
    pub capability: Capability,
    pub target: String,
    pub answer: Answer,
}

/// What the person answered for one plugin.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Whether the person let it in.
    pub allowed: bool,
    /// The version it was answered for.
    pub version: String,
    #[serde(default)]
    pub answers: Vec<Answered>,
}

impl Record {
    /// The answer to `capability` on `target`, if it was asked.
    pub fn answer(
        &self,
        capability: Capability,
        target: &str,
    ) -> Option<Answer> {
        self.answers
            .iter()
            .find(|answered| {
                answered.capability == capability
                    && answered.target == target
            })
            .map(|answered| answered.answer)
    }

    /// Sets the answer to `capability` on `target`.
    pub fn set(
        &mut self,
        capability: Capability,
        target: &str,
        answer: Answer,
    ) {
        match self.answers.iter_mut().find(|answered| {
            answered.capability == capability
                && answered.target == target
        }) {
            Some(answered) => answered.answer = answer,
            None => self.answers.push(Answered {
                capability,
                target: target.into(),
                answer,
            }),
        }
    }
}

#[derive(Default, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantsFile {
    #[serde(default)]
    plugins: BTreeMap<PluginId, Record>,
}

/// The person's answers for every plugin, as the grants file keeps
/// them.
#[derive(Resource, Debug)]
pub struct Grants {
    path: PathBuf,
    records: BTreeMap<PluginId, Record>,
    /// Why the file could not be read. It is left as it is, and
    /// nothing is written.
    held: Option<String>,
}

impl Grants {
    /// The answers in `home`. A missing file holds none.
    pub fn open(home: &Home) -> Self {
        let path = home.path(FILE);
        let read = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<GrantsFile>(&text)
                .map_err(|e| e.message().to_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(GrantsFile::default())
            }
            Err(e) => Err(e.to_string()),
        };
        let (records, held) = match read {
            Ok(file) => (file.plugins, None),
            Err(why) => (BTreeMap::new(), Some(why)),
        };
        Self {
            path,
            records,
            held,
        }
    }

    /// Why the file could not be read, while it is left as it is.
    pub fn held(&self) -> Option<&str> {
        self.held.as_deref()
    }

    pub fn record(&self, id: &PluginId) -> Option<&Record> {
        self.records.get(id)
    }

    pub fn records(
        &self,
    ) -> impl Iterator<Item = (&PluginId, &Record)> {
        self.records.iter()
    }

    /// Keeps `record` for the plugin `id`, and writes the file.
    pub fn set(
        &mut self,
        id: &PluginId,
        record: Record,
    ) -> Result<(), String> {
        if self.records.get(id) == Some(&record) {
            return Ok(());
        }
        self.records.insert(id.clone(), record);
        self.save()
    }

    /// Forgets the plugin `id`, and writes the file.
    pub fn forget(&mut self, id: &PluginId) -> Result<(), String> {
        if self.records.remove(id).is_none() {
            return Ok(());
        }
        self.save()
    }

    fn save(&self) -> Result<(), String> {
        if self.held.is_some() {
            return Ok(());
        }
        let file = GrantsFile {
            plugins: self.records.clone(),
        };
        let text =
            toml::to_string(&file).map_err(|e| e.to_string())?;
        write_whole(&self.path, text).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grants_come_back_and_an_unreadable_file_stays() {
        let root = std::env::temp_dir().join(format!(
            "hamlet_engine_test_grants_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let home = Home::at(&root);
        let id = PluginId::try_from("hamlet.weather").unwrap();
        let mut record = Record {
            allowed: true,
            version: "0.1.0".into(),
            answers: Vec::new(),
        };
        record.set(
            Capability::NetFetch,
            "forecast.example.org",
            Answer::Ask,
        );
        record.set(
            Capability::AssetRead,
            "weather.forecast",
            Answer::Deny,
        );

        let mut grants = Grants::open(&home);
        grants.set(&id, record.clone()).unwrap();
        let names = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, [FILE]);
        let again = Grants::open(&home);
        assert_eq!(again.held(), None);
        assert_eq!(again.record(&id), Some(&record));

        let broken =
            "[plugins.\"hamlet.weather\"]\nallowed = maybe\n";
        fs::write(home.path(FILE), broken).unwrap();
        let mut grants = Grants::open(&home);
        assert!(grants.held().is_some());
        grants.set(&id, record).unwrap();
        grants.forget(&id).unwrap();
        assert_eq!(
            fs::read_to_string(home.path(FILE)).unwrap(),
            broken
        );
        let _ = fs::remove_dir_all(&root);
    }
}
