//! The thing of the world a command was run on, from a context menu.

use serde::{Deserialize, Serialize};

use crate::bindings::exports::hamlet::plugin::plugin::CommandCall;

/// A thing of the world, as the host writes it into the arguments of
/// a command a context item runs.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Subject {
    /// What it is: `agent`, `house`, `mayor`, `ruin`, `hamlet`,
    /// `chief`, `hearth`, `library` or `ground`.
    pub kind: String,
    /// Names it among the things of its kind while it is there.
    pub id: String,
    /// What the window calls it.
    pub name: String,
}

/// The thing the command `call` was run on.
pub fn of(call: &CommandCall) -> Result<Subject, String> {
    crate::calls::args(call)
}
