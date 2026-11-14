//! The calls the `realm` actor takes, and its answers.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use crate::workspace::files::EditDiff;
use crate::workspace::reach::{Need, SecretsVersion};
use crate::workspace::{
    AgentNumber, CallId, ConversationId, ReachVersion, RealmTask,
};

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

/// Tells the realm that an agent's call waits for a chief, so it asks
/// one (H11). Called by the workspace once, when the wait opens. The
/// realm answers the workspace with a
/// [`crate::workspace::ChiefAnswer`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeedsChief {
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    /// The realm's task of the call, as the call carried it (W1).
    pub realm_task: Option<RealmTask>,
    pub needs: Vec<Need>,
    /// The call in words for the chief, the secrets taken out, at
    /// most 2000 characters.
    pub subject: String,
    /// An edit's change, the secrets taken out.
    pub change: Option<EditDiff>,
    /// The agent's reach the call was weighed under.
    pub reach_version: ReachVersion,
}

impl Action for NeedsChief {
    type Output = ();
    const NAME: &'static str = "needs_chief";
}
