//! The plugins that implement an interface a plugin imports, from
//! the copy the host puts in its store before each call.

use super::State;

impl State {
    pub(super) fn implementers_of(
        &self,
        interface: &str,
    ) -> Result<Vec<String>, String> {
        self.reach
            .implementers
            .iter()
            .find(|(name, _)| name == interface)
            .map(|(_, ids)| ids.clone())
            .ok_or_else(|| {
                format!(
                    "`{interface}` is not in the manifest's imports"
                )
            })
    }
}
