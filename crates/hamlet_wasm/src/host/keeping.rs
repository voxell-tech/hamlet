//! What a plugin keeps: its storage on disk, and the state the host
//! holds for it in memory. Neither needs a grant.

use super::State;

impl State {
    pub(super) fn stored(&self, key: &str) -> Option<Vec<u8>> {
        self.storage.as_ref()?.get(key)
    }

    pub(super) fn store(
        &mut self,
        key: &str,
        value: &[u8],
    ) -> Result<(), String> {
        let storage =
            self.storage.as_ref().ok_or("there is no storage")?;
        storage.set(key, value)?;
        self.wrote(key);
        Ok(())
    }

    pub(super) fn unstore(&mut self, key: &str) {
        if let Some(storage) = &self.storage {
            storage.delete(key);
            self.wrote(key);
        }
    }

    pub(super) fn stored_keys(&self, prefix: &str) -> Vec<String> {
        self.storage
            .as_ref()
            .map(|storage| storage.keys(prefix))
            .unwrap_or_default()
    }

    pub(super) fn keep_state(
        &mut self,
        state: Vec<u8>,
    ) -> Result<(), String> {
        if state.len() > self.state_room {
            return Err(format!(
                "the state has {} bytes, the most is {}",
                state.len(),
                self.state_room
            ));
        }
        self.state = state;
        Ok(())
    }
}
