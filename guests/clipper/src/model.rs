//! What the plugin knows, which the host keeps across a reload.

use hamlet_guest::Calls;
use serde::{Deserialize, Serialize};

/// What a call that waits for an answer was for.
#[derive(Serialize, Deserialize)]
pub enum Why {
    /// A note with the selected agent's name.
    AddNote,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Model {
    pub calls: Calls<Why>,
    /// The agent selected in the world, as the host last said.
    pub selected: String,
    /// The line under the button: the reply to the last call, or why
    /// there was none.
    pub line: String,
    /// The notes that Notes said it saved since the plugin started.
    pub saved: u32,
}
