//! The collections other plugins share: keys and values of another
//! plugin's storage that its manifest offers. The manifest of this
//! plugin imports each as `plugin/name`, and the chief chooses
//! `collections.read` and `collections.write` for it. Every call
//! returns a copy, and a failure says why in a line.

use serde::de::DeserializeOwned;

use crate::bindings::hamlet::plugin::host;
pub use crate::bindings::hamlet::plugin::host::{
    CollectionItem, CollectionPage,
};

/// A page of the collection `name` of `plugin`, from the first key
/// after `after`, at most `limit` items. The host caps a page too:
/// where [`CollectionPage::next`] is a key, ask again `after` it.
pub fn list(
    plugin: &str,
    name: &str,
    after: Option<&str>,
    limit: u32,
) -> Result<CollectionPage, String> {
    host::collection_list(plugin, name, after, limit)
}

/// The value of `key`, if it is there.
pub fn get(
    plugin: &str,
    name: &str,
    key: &str,
) -> Result<Option<Vec<u8>>, String> {
    host::collection_get(plugin, name, key)
}

/// The value of `key` as a `T` written with `postcard`, if it is
/// there.
pub fn load<T: DeserializeOwned>(
    plugin: &str,
    name: &str,
    key: &str,
) -> Result<Option<T>, String> {
    get(plugin, name, key)?
        .map(|bytes| {
            postcard::from_bytes(&bytes).map_err(|e| e.to_string())
        })
        .transpose()
}

/// Sets `key`. It needs `collections.write` and a collection that its
/// plugin says is writable.
pub fn put(
    plugin: &str,
    name: &str,
    key: &str,
    value: &[u8],
) -> Result<(), String> {
    host::collection_put(plugin, name, key, value)
}

/// Removes `key`, under the same conditions as [`put`].
pub fn delete(
    plugin: &str,
    name: &str,
    key: &str,
) -> Result<(), String> {
    host::collection_delete(plugin, name, key)
}
