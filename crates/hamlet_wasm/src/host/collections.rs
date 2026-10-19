//! The collections other plugins share: reads and writes through the
//! storage of the plugin that shares them, behind the manifest's
//! imports and the capabilities `collections.read` and
//! `collections.write`. What comes back is a copy.

use bevy::log::warn;
use hamlet_sdk::capability::{Capability, Verdict};
use hamlet_sdk::plugin::{Collection, Imports, link};

use super::State;
use crate::interface::hamlet::plugin::host::{
    CollectionItem, CollectionPage,
};
use crate::storage::Storage;

/// A collection another plugin shares, with the storage it is in.
#[derive(Clone, PartialEq, Debug)]
pub struct Shared {
    pub plugin: String,
    pub name: String,
    pub prefix: String,
    pub writable: bool,
    pub storage: Storage,
}

/// What a plugin may reach of the others' collections, and which of
/// its own are shared.
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Reach {
    pub imports: Imports,
    /// The collections the plugin shares.
    pub own: Vec<Collection>,
    /// The collections it imports that are there to be read.
    pub shared: Vec<Shared>,
    /// The plugins that implement each interface it imports.
    pub implementers: Vec<(String, Vec<String>)>,
}

impl State {
    pub(super) fn collection_page(
        &mut self,
        plugin: &str,
        name: &str,
        after: Option<String>,
        limit: u32,
    ) -> Result<CollectionPage, String> {
        let shared =
            self.open(plugin, name, Capability::CollectionsRead)?;
        let keys = shared
            .storage
            .keys(&shared.prefix)
            .into_iter()
            .filter_map(|key| {
                key.strip_prefix(&shared.prefix).map(str::to_string)
            })
            .filter(|key| {
                after.as_ref().is_none_or(|after| key > after)
            })
            .collect::<Vec<_>>();
        let room = self.page_items.min(limit as usize);
        let mut items = Vec::new();
        let mut bytes = 0;
        for key in &keys {
            let full = format!("{}{key}", shared.prefix);
            let Some(value) = shared.storage.get(&full) else {
                continue;
            };
            if items.len() >= room
                || (!items.is_empty()
                    && bytes + value.len() > self.page_bytes)
            {
                break;
            }
            bytes += value.len();
            items.push(CollectionItem {
                key: key.clone(),
                value,
            });
        }
        let more = items.last().is_some_and(|last| {
            keys.iter().any(|key| *key > last.key)
        });
        Ok(CollectionPage {
            next: items
                .last()
                .filter(|_| more)
                .map(|last| last.key.clone()),
            items,
        })
    }

    pub(super) fn collection_value(
        &mut self,
        plugin: &str,
        name: &str,
        key: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        let shared =
            self.open(plugin, name, Capability::CollectionsRead)?;
        Ok(shared.storage.get(&format!("{}{key}", shared.prefix)))
    }

    pub(super) fn collection_set(
        &mut self,
        plugin: &str,
        name: &str,
        key: &str,
        value: &[u8],
    ) -> Result<(), String> {
        let shared =
            self.open(plugin, name, Capability::CollectionsWrite)?;
        shared
            .storage
            .set(&format!("{}{key}", shared.prefix), value)?;
        self.changed(plugin, name);
        Ok(())
    }

    pub(super) fn collection_remove(
        &mut self,
        plugin: &str,
        name: &str,
        key: &str,
    ) -> Result<(), String> {
        let shared =
            self.open(plugin, name, Capability::CollectionsWrite)?;
        shared.storage.delete(&format!("{}{key}", shared.prefix));
        self.changed(plugin, name);
        Ok(())
    }

    /// Notes that the collection `name` of `plugin` changed.
    pub(super) fn changed(&mut self, plugin: &str, name: &str) {
        let entry = (plugin.to_string(), name.to_string());
        if !self.changes.contains(&entry) {
            self.changes.push(entry);
        }
    }

    /// Notes that the key `key` of the plugin's own storage was
    /// written, for the collection it lies in.
    pub(super) fn wrote(&mut self, key: &str) {
        let hit = self
            .reach
            .own
            .iter()
            .filter(|own| key.starts_with(&own.prefix))
            .map(|own| own.name.clone())
            .collect::<Vec<_>>();
        let plugin = self.plugin.clone();
        for name in hit {
            self.changed(&plugin, &name);
        }
    }

    /// The collection `name` of `plugin`, if the manifest imports it
    /// and `capability` is allowed for it. A write needs it to be
    /// writable.
    fn open(
        &mut self,
        plugin: &str,
        name: &str,
        capability: Capability,
    ) -> Result<Shared, String> {
        let target = link(plugin, name);
        if !self.reach.imports.collection(plugin, name) {
            return Err(format!(
                "`{target}` is not in the manifest's imports"
            ));
        }
        let verdict = self.granted.verdict_for(capability, &target);
        if verdict != Some(Verdict::Allowed) {
            if verdict == Some(Verdict::Ask) {
                self.want_for(capability, Some(&target), "");
            }
            let words = capability.name();
            warn!(
                "plugin {}: {words} not granted for {target}",
                self.plugin
            );
            return Err(format!("{words} not granted"));
        }
        let shared = self
            .reach
            .shared
            .iter()
            .find(|shared| {
                shared.plugin == plugin && shared.name == name
            })
            .cloned()
            .ok_or_else(|| format!("`{target}` is not there"))?;
        if capability == Capability::CollectionsWrite
            && !shared.writable
        {
            return Err(format!("`{target}` is read-only"));
        }
        Ok(shared)
    }
}
