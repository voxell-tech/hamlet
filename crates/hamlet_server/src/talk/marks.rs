//! What an input is, as its runtime keeps it with the input and a
//! transcript shows it. The runtime protocol carries it untyped.

use anyhow::{Context, Result};
use hamlet_protocol::runtime;
use hamlet_store::ids::AgentId;
use serde::{Deserialize, Serialize};

/// The mark of an input or a write.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub(crate) enum Mark {
    /// A chief's line.
    #[serde(rename = "hamlet.chief")]
    Chief {
        chief: String,
        text: String,
        at: i64,
    },
    /// A failed or stopped turn carried on.
    #[serde(rename = "hamlet.retry")]
    Retry { at: i64 },
    #[serde(rename = "hamlet.reflection")]
    Reflection { at: i64 },
    /// Words for people alone, which the model never sees (C30).
    #[serde(rename = "hamlet.notice")]
    Notice { text: String, at: i64 },
    #[serde(rename = "hamlet.letter")]
    Letter(From),
    #[serde(rename = "hamlet.question")]
    Question(From),
    #[serde(rename = "hamlet.reply")]
    Reply(From),
    /// A one-off call's instructions.
    #[serde(rename = "hamlet.errand")]
    Errand { at: i64 },
    /// A worker's task, from whoever started it.
    #[serde(rename = "hamlet.worker")]
    Worker { author: String, at: i64 },
    #[serde(rename = "hamlet.dream")]
    Dream { at: i64 },
}

/// Who a message from another agent is from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct From {
    pub from: AgentId,
    /// The sender as a transcript shows it.
    pub author: String,
    pub at: i64,
}

impl Mark {
    /// The mark of the protocol's `mark`; `None` for a kind outside
    /// `hamlet.`, which the server does not make.
    pub fn of(mark: &runtime::Mark) -> Result<Option<Self>> {
        if !mark.kind.starts_with("hamlet.") {
            return Ok(None);
        }
        serde_json::from_value(serde_json::to_value(mark)?)
            .map(Some)
            .with_context(|| {
                format!("reading a `{}` mark", mark.kind)
            })
    }

    pub fn wire(&self) -> runtime::Mark {
        serde_json::from_value(
            serde_json::to_value(self)
                .expect("a mark has only string keys"),
        )
        .expect("each mark's data is an object")
    }

    /// Whether its turn is one a chief asked for.
    pub fn is_chief_turn(&self) -> bool {
        matches!(self, Self::Chief { .. } | Self::Retry { .. })
    }

    /// When it was made.
    pub fn at(&self) -> i64 {
        match self {
            Self::Chief { at, .. }
            | Self::Retry { at }
            | Self::Reflection { at }
            | Self::Notice { at, .. }
            | Self::Errand { at }
            | Self::Worker { at, .. }
            | Self::Dream { at } => *at,
            Self::Letter(from)
            | Self::Question(from)
            | Self::Reply(from) => from.at,
        }
    }

    /// The mark of an item that may have none.
    pub fn of_item(
        mark: Option<&runtime::Mark>,
    ) -> Result<Option<Self>> {
        mark.map_or(Ok(None), Self::of)
    }
}
