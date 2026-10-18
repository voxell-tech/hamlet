//! The plugin's own store of keys and values, kept on disk. It needs
//! no capability. A key is 1 to a few dozen bytes of `a-z`, `0-9`,
//! `.`, `_` and `-`, not starting with `.`, and the host caps the
//! number of keys and the bytes of a value and of the whole store.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::bindings::hamlet::plugin::host;

/// The value of `key`, if it is there.
pub fn get(key: &str) -> Option<Vec<u8>> {
    host::storage_get(key)
}

/// Sets `key` to `value`. A bad key or a cap that would be passed is
/// an error.
pub fn set(key: &str, value: &[u8]) -> Result<(), String> {
    host::storage_set(key, value)
}

/// Removes `key`, if it is there.
pub fn delete(key: &str) {
    host::storage_delete(key);
}

/// The keys that start with `prefix`, in order.
pub fn keys(prefix: &str) -> Vec<String> {
    host::storage_keys(prefix)
}

/// The value of `key` as a `T` written with [`save`], if it is there
/// and can be read as one.
pub fn load<T: DeserializeOwned>(key: &str) -> Option<T> {
    postcard::from_bytes(&get(key)?).ok()
}

/// Sets `key` to `value`, written with `postcard`.
pub fn save<T: Serialize>(
    key: &str,
    value: &T,
) -> Result<(), String> {
    let bytes =
        postcard::to_allocvec(value).map_err(|e| e.to_string())?;
    set(key, &bytes)
}
