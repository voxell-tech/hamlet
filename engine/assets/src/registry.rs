//! The type registry: each type of asset a plugin makes, and its
//! version. A type belongs to the plugin that makes it.

use std::collections::BTreeMap;

use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::query::Added;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use hamlet_engine_api::{PluginId, TypeName};
use hamlet_engine_core::LogLine;
use hamlet_engine_plugins::PluginInfo;

/// Every type made by a plugin there is: its maker and its version.
#[derive(Resource, Default, Debug)]
pub struct TypeRegistry(BTreeMap<TypeName, (PluginId, u32)>);

impl TypeRegistry {
    /// The types `plugins` make. A type two make is the one's whose
    /// id comes first; the others' claims are given back, with the
    /// type.
    pub fn of<'a>(
        plugins: impl Iterator<Item = &'a PluginInfo>,
    ) -> (Self, Vec<(PluginId, TypeName)>) {
        let mut plugins = plugins.collect::<Vec<_>>();
        plugins.sort_by(|a, b| a.id.cmp(&b.id));
        let mut types = BTreeMap::new();
        let mut taken = Vec::new();
        for info in plugins {
            for kind in &info.assets.makes {
                if types.contains_key(&kind.name) {
                    taken.push((info.id.clone(), kind.name.clone()));
                } else {
                    types.insert(
                        kind.name.clone(),
                        (info.id.clone(), kind.version),
                    );
                }
            }
        }
        (Self(types), taken)
    }

    /// The plugin that makes `kind`, and the version it makes.
    pub fn maker(&self, kind: &TypeName) -> Option<(&PluginId, u32)> {
        self.0.get(kind).map(|(maker, version)| (maker, *version))
    }
}

/// Gathers the types again when a plugin comes or goes.
pub(crate) fn follow(
    added: Query<(), Added<PluginInfo>>,
    mut gone: RemovedComponents<PluginInfo>,
    plugins: Query<&PluginInfo>,
    mut registry: ResMut<TypeRegistry>,
    mut log: MessageWriter<LogLine>,
) {
    if added.is_empty() && gone.read().count() == 0 {
        return;
    }
    let (types, taken) = TypeRegistry::of(plugins.iter());
    *registry = types;
    for (plugin, kind) in taken {
        let maker =
            registry.maker(&kind).map(|(maker, _)| maker.clone());
        log.write(LogLine::warn(
            plugin.as_str(),
            format!(
                "{kind} is made by {} already; this plugin's are \
                 not kept",
                maker.map_or_else(String::new, |maker| maker
                    .to_string())
            ),
        ));
    }
}
