//! How far an agent's edits and commands go without a chief's answer
//! (C21).

use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::sql_types::Text;
use hamlet_protocol::client::Access as Wire;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

/// An agent's permissions: its own, kept with it, and in force for
/// the workers it starts.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    AsExpression,
    FromSqlRow,
    IntoStaticStr,
    EnumString,
    Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[diesel(sql_type = Text)]
pub(crate) enum Access {
    /// Each edit of a folder and each command waits for a chief.
    #[default]
    Supervised,
    /// Edits of a folder run; each command waits for a chief.
    AcceptEdits,
    /// Edits and commands run, in the sandbox. The network, acts the
    /// chief reserved and new folders still wait.
    Auto,
    /// Edits and commands run with no sandbox and the network open.
    /// Acts the chief reserved and new folders still wait.
    Full,
}
hamlet_store::named_text!(Access);

impl Access {
    /// An edit of a folder waits for a chief.
    pub fn holds_edits(self) -> bool {
        self == Self::Supervised
    }

    /// A command waits for a chief.
    pub fn holds_commands(self) -> bool {
        matches!(self, Self::Supervised | Self::AcceptEdits)
    }

    /// Commands run with no sandbox, and the network is open.
    pub fn is_full(self) -> bool {
        self == Self::Full
    }

    /// What an agent a parent adds starts with: its parent's, short
    /// of `Full`, which only a chief gives.
    pub fn handed_down(self) -> Self {
        match self {
            Self::Full => Self::Auto,
            other => other,
        }
    }

    /// The label the window shows, as T3 Code's.
    pub fn label(self) -> &'static str {
        match self {
            Self::Supervised => "Supervised",
            Self::AcceptEdits => "Auto-accept edits",
            Self::Auto => "Auto",
            Self::Full => "Full access",
        }
    }

    pub fn wire(self) -> Wire {
        match self {
            Self::Supervised => Wire::Supervised,
            Self::AcceptEdits => Wire::AcceptEdits,
            Self::Auto => Wire::Auto,
            Self::Full => Wire::Full,
        }
    }
}
