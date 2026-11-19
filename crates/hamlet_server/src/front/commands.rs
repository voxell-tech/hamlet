//! A chief's slash commands, each to one act of one actor or to the
//! door's own words.

use anyhow::Result;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client as protocol;

use super::call::Calls;
use super::view::View;
use crate::society::refusal::Refusal;

/// What a command came to.
pub(crate) struct Outcome {
    /// The words of the command's reply, or of its refusal.
    pub(crate) reply: Result<String, Refusal>,
    /// The agent whose chat its watchers read again, as after
    /// `/new`.
    pub(crate) reread: Option<AgentNumber>,
}

/// Runs the command `say` holds, with the realm as `view` shows it.
pub(crate) async fn run(
    _calls: &Calls,
    _view: &View,
    say: &protocol::Say,
) -> Result<Outcome> {
    let what = say.text.split_whitespace().next().unwrap_or_default();
    Ok(unsupported(what))
}

/// Makes the edit `edit` asks for.
pub(crate) async fn edit(
    _calls: &Calls,
    _view: &View,
    _edit: &protocol::Edit,
) -> Result<Outcome> {
    Ok(unsupported("edit"))
}

fn unsupported(what: &str) -> Outcome {
    Outcome {
        reply: Err(Refusal::Unsupported {
            what: format!("`{what}`"),
        }),
        reread: None,
    }
}
