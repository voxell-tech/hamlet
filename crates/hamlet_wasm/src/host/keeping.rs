//! What a plugin keeps on disk, in its storage. It needs no grant.

use super::State;

impl State {
    pub(super) fn stored(&self, key: &str) -> Option<Vec<u8>> {
        self.storage.as_ref()?.get(key)
    }

    pub(super) fn store(
        &self,
        key: &str,
        value: &[u8],
    ) -> Result<(), String> {
        let storage =
            self.storage.as_ref().ok_or("there is no storage")?;
        storage.set(key, value)
    }

    pub(super) fn unstore(&self, key: &str) {
        if let Some(storage) = &self.storage {
            storage.delete(key);
        }
    }

    pub(super) fn stored_keys(&self, prefix: &str) -> Vec<String> {
        self.storage
            .as_ref()
            .map(|storage| storage.keys(prefix))
            .unwrap_or_default()
    }
}
