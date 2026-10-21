//! The limits each plugin is held to, [`PluginBudgets`] on its
//! entity: the person's value, else the plugin's ask, else the
//! engine's. The person's values are kept in `limits.toml` in the
//! home, written whole or not at all; a file that cannot be read is
//! never written over.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::Insert;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use hamlet_engine_api::{Limit, LimitValue, Limits, PluginId};
use hamlet_engine_core::{
    DefaultLimits, Home, LogLine, PluginBudgets, write_whole,
};
use serde::{Deserialize, Serialize};

use crate::list::Uninstall;
use crate::{PluginInfo, PluginKind};

/// The file's name in the home.
pub(crate) const FILE: &str = "limits.toml";

pub(crate) fn plugin(app: &mut App) {
    let home = app.world().resource::<Home>().clone();
    app.insert_resource(ChosenLimits::open(&home))
        .add_observer(resolve)
        .add_observer(forget)
        .add_systems(Startup, tell_held)
        .add_systems(Update, follow);
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitsFile {
    #[serde(default)]
    plugins: BTreeMap<PluginId, Limits>,
}

/// The limits the person set for each plugin, as the file keeps
/// them.
#[derive(Resource, Debug)]
pub struct ChosenLimits {
    path: PathBuf,
    plugins: BTreeMap<PluginId, Limits>,
    /// Why the file could not be read. It is left as it is, and
    /// nothing is written.
    held: Option<String>,
}

impl ChosenLimits {
    /// The limits set in `home`. A missing file holds none.
    pub fn open(home: &Home) -> Self {
        let path = home.path(FILE);
        let read = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<LimitsFile>(&text)
                .map_err(|e| e.message().to_owned())
                .and_then(|file| {
                    for (id, limits) in &file.plugins {
                        limits.check().map_err(|limit| {
                            format!(
                                "{} of {id} is not above zero",
                                limit.name()
                            )
                        })?;
                    }
                    Ok(file)
                }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(LimitsFile::default())
            }
            Err(e) => Err(e.to_string()),
        };
        let (plugins, held) = match read {
            Ok(file) => (file.plugins, None),
            Err(why) => (BTreeMap::new(), Some(why)),
        };
        Self {
            path,
            plugins,
            held,
        }
    }

    /// Why the file could not be read, while it is left as it is.
    pub fn held(&self) -> Option<&str> {
        self.held.as_deref()
    }

    pub fn get(&self, id: &PluginId) -> Option<&Limits> {
        self.plugins.get(id)
    }

    /// Sets `limit` of the plugin `id` to `value`, or back to what
    /// it would be without the person for `None`, and writes the
    /// file. A value that is not above zero is refused.
    pub fn set(
        &mut self,
        id: &PluginId,
        limit: Limit,
        value: Option<LimitValue>,
    ) -> Result<(), String> {
        if value.is_some_and(|value| !value.positive()) {
            return Err(format!(
                "{} must be above zero",
                limit.name()
            ));
        }
        let mut limits =
            self.plugins.get(id).cloned().unwrap_or_default();
        limits.set(limit, value);
        if self.plugins.get(id).cloned().unwrap_or_default() == limits
        {
            return Ok(());
        }
        if limits.is_empty() {
            self.plugins.remove(id);
        } else {
            self.plugins.insert(id.clone(), limits);
        }
        self.save()
    }

    /// Forgets what the person set for the plugin `id`, and writes
    /// the file.
    pub fn forget(&mut self, id: &PluginId) -> Result<(), String> {
        if self.plugins.remove(id).is_none() {
            return Ok(());
        }
        self.save()
    }

    fn save(&self) -> Result<(), String> {
        if self.held.is_some() {
            return Ok(());
        }
        let file = LimitsFile {
            plugins: self.plugins.clone(),
        };
        let unwritten =
            |why: String| format!("{FILE} cannot be written: {why}");
        let text = toml::to_string(&file)
            .map_err(|e| unwritten(e.to_string()))?;
        write_whole(&self.path, text)
            .map_err(|e| unwritten(e.to_string()))
    }
}

/// Gives a plugin its limits as it comes or its manifest changes.
fn resolve(
    inserted: On<Insert, PluginInfo>,
    plugins: Query<&PluginInfo>,
    defaults: Res<DefaultLimits>,
    chosen: Res<ChosenLimits>,
    mut commands: Commands,
) {
    let Ok(info) = plugins.get(inserted.entity) else {
        return;
    };
    commands
        .entity(inserted.entity)
        .insert(PluginBudgets::resolve(
            &defaults,
            &info.limits,
            chosen.get(&info.id),
        ));
}

/// Gives every plugin its limits again when the engine's or the
/// person's change.
fn follow(
    plugins: Query<(Entity, &PluginInfo, Option<&PluginBudgets>)>,
    defaults: Res<DefaultLimits>,
    chosen: Res<ChosenLimits>,
    mut commands: Commands,
) {
    if !defaults.is_changed() && !chosen.is_changed() {
        return;
    }
    for (plugin, info, now) in &plugins {
        let budgets = PluginBudgets::resolve(
            &defaults,
            &info.limits,
            chosen.get(&info.id),
        );
        if now != Some(&budgets) {
            commands.entity(plugin).insert(budgets);
        }
    }
}

/// Forgets what the person set for a plugin being uninstalled.
fn forget(
    event: On<Uninstall>,
    plugins: Query<&PluginInfo>,
    mut chosen: ResMut<ChosenLimits>,
    mut log: MessageWriter<LogLine>,
) {
    let Ok(info) = plugins.get(event.entity) else {
        return;
    };
    if info.kind != PluginKind::Installed {
        return;
    }
    if let Err(why) = chosen.forget(&info.id) {
        log.write(LogLine::error("plugins", why));
    }
}

fn tell_held(
    chosen: Res<ChosenLimits>,
    mut log: MessageWriter<LogLine>,
) {
    if let Some(why) = chosen.held() {
        log.write(LogLine::error(
            "plugins",
            format!(
                "{FILE} cannot be read ({why}). It is left as it \
                 is, and the limits set there are not used."
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_kept_and_an_unreadable_file_stays() {
        let root = std::env::temp_dir().join(format!(
            "hamlet_engine_test_limits_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let home = Home::at(&root);
        let id = PluginId::try_from("hamlet.realm").unwrap();
        let fuel = Limit::FuelPerCall.parse("3_000_000_000");

        ChosenLimits::open(&home)
            .set(&id, Limit::FuelPerCall, fuel)
            .unwrap();
        let again = ChosenLimits::open(&home);
        assert_eq!(again.held(), None);
        assert_eq!(
            again.get(&id).and_then(|l| l.get(Limit::FuelPerCall)),
            fuel
        );

        let broken = "[plugins.\"hamlet.realm\"]\nmemory = lots\n";
        fs::write(home.path(FILE), broken).unwrap();
        let mut chosen = ChosenLimits::open(&home);
        assert!(chosen.held().is_some());
        chosen.set(&id, Limit::Memory, fuel).unwrap();
        chosen.forget(&id).unwrap();
        assert_eq!(
            fs::read_to_string(home.path(FILE)).unwrap(),
            broken
        );
        fs::write(
            home.path(FILE),
            "[plugins.\"hamlet.realm\"]\nmemory = 0\n",
        )
        .unwrap();
        assert!(ChosenLimits::open(&home).held().is_some());
        let _ = fs::remove_dir_all(&root);
    }
}
