//! The limits of one plugin's WASM instance: as the engine uses them,
//! and each given or not, as a manifest asks for them and the person
//! sets them.

use serde::{Deserialize, Serialize};

/// The limits one plugin's instance is held to.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginLimits {
    /// Bytes of memory, all of the plugin's memories summed.
    pub memory: u64,
}

/// Limits each given or not, by the names of [`PluginLimits`]'s
/// fields.
#[derive(
    Clone, PartialEq, Eq, Default, Debug, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    // No field is skipped when it is none, as postcard is read by
    // position; toml leaves a none out itself.
    #[serde(default)]
    pub memory: Option<u64>,
}

impl PluginLimits {
    /// These limits with each one `over` gives in its place.
    pub fn with(&self, over: &Limits) -> Self {
        Self {
            memory: over.memory.unwrap_or(self.memory),
        }
    }

    /// The name of the first limit that is not above zero.
    pub fn check(&self) -> Result<(), &'static str> {
        if self.memory == 0 {
            return Err("memory");
        }
        Ok(())
    }
}

impl Limits {
    /// The name of the first limit given that is not above zero.
    pub fn check(&self) -> Result<(), &'static str> {
        if self.memory == Some(0) {
            return Err("memory");
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.memory.is_none()
    }
}
