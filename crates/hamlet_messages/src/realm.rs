//! The calls the `realm` actor takes, and its answers.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use crate::workspace::reach::SecretsVersion;

/// Asks the realm for its list of secrets in force. Called by the
/// workspace, which holds the answer in memory only (W3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secrets;

impl Action for Secrets {
    type Output = SecretList;
    const NAME: &'static str = "secrets";
}

/// The realm's secrets of one version: the values the workspace takes
/// out of everything it hands to an agent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretList {
    pub version: SecretsVersion,
    pub values: Vec<String>,
}

/// Names how many values there are, never a value.
impl std::fmt::Debug for SecretList {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("SecretList")
            .field("version", &self.version)
            .field("values", &self.values.len())
            .finish()
    }
}
