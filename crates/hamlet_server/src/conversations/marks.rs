//! What an input is, as its runtime keeps it with the input and a
//! transcript shows it. The runtime protocol carries it untyped.

use anyhow::{Context, Result};
use hamlet_protocol::client::{AttachedFile, AttachedImage};
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
        /// The images the line carried, at their places in `text`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<AttachedImage>,
        /// The files the line carried, at their places in `text`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<AttachedFile>,
    },
    /// A failed or stopped turn carried on.
    #[serde(rename = "hamlet.retry")]
    Retry { at: i64 },
    #[serde(rename = "hamlet.reflection")]
    Reflection { at: i64 },
    /// Words for people alone, which the model never sees (C30).
    #[serde(rename = "hamlet.notice")]
    Notice { text: String, at: i64 },
    #[serde(rename = "hamlet.notification")]
    Notification(From),
    #[serde(rename = "hamlet.request")]
    Request(From),
    #[serde(rename = "hamlet.response")]
    Response(From),
    /// Why a request or a worker has no answer, written in without a
    /// turn.
    #[serde(rename = "hamlet.error-response")]
    ErrorResponse(From),
    /// A one-off call's instructions.
    #[serde(rename = "hamlet.errand")]
    Errand { at: i64 },
    /// A worker's task, from whoever started it.
    #[serde(rename = "hamlet.worker")]
    Worker { author: String, at: i64 },
    #[serde(rename = "hamlet.dream")]
    Dream { at: i64 },
    /// A chief's message in a conversation brought in from another
    /// program (C35).
    #[serde(rename = "hamlet.earlier")]
    Earlier { chief: String, at: i64 },
    /// A tool call another program made, for people alone (C35).
    #[serde(rename = "hamlet.step")]
    Step {
        tool: String,
        summary: String,
        at: i64,
    },
    /// A summary asked of the conversation now.
    #[serde(rename = "hamlet.compaction")]
    Compaction { at: i64 },
    /// The conversation of a hire merged into this one, given to the
    /// model as what it said and did.
    #[serde(rename = "hamlet.merged")]
    Merged {
        from: AgentId,
        author: String,
        at: i64,
    },
}

/// Who a message from another agent is from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct From {
    pub from: AgentId,
    /// The sender as a transcript shows it.
    pub author: String,
    /// Its words without the heading the model reads them under.
    pub text: String,
    pub at: i64,
    /// The images sent with it, by their ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
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

    /// The images a chief's line or an agent's message carried.
    pub fn images(&self) -> Vec<AttachedImage> {
        match self {
            Self::Chief { images, .. } => images.clone(),
            Self::Notification(From { images, .. })
            | Self::Request(From { images, .. }) => images
                .iter()
                .filter_map(|image| image.parse().ok())
                .map(AttachedImage::last)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The files a chief's line carried.
    pub fn files(&self) -> Vec<AttachedFile> {
        match self {
            Self::Chief { files, .. } => files.clone(),
            _ => Vec::new(),
        }
    }

    /// The words a model reads for a message from another agent:
    /// what it is and who sent it, then its words.
    pub fn heading(&self) -> Option<String> {
        let (heading, from) = match self {
            Self::Notification(from) => (
                format!("[A notification from {}]", from.author),
                from,
            ),
            Self::Request(from) => (
                format!(
                    "[A request from {}; your final answer goes \
                     back to it as the response]",
                    from.author
                ),
                from,
            ),
            Self::Response(from) => {
                (format!("[A response from {}]", from.author), from)
            }
            Self::ErrorResponse(from) => (
                format!("[An error response from {}]", from.author),
                from,
            ),
            _ => return None,
        };
        Some(format!("{heading}\n\n{}", from.text))
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
            | Self::Dream { at }
            | Self::Earlier { at, .. }
            | Self::Step { at, .. }
            | Self::Compaction { at }
            | Self::Merged { at, .. } => *at,
            Self::Notification(from)
            | Self::Request(from)
            | Self::Response(from)
            | Self::ErrorResponse(from) => from.at,
        }
    }

    /// The mark of an item that may have none.
    pub fn of_item(
        mark: Option<&runtime::Mark>,
    ) -> Result<Option<Self>> {
        mark.map_or(Ok(None), Self::of)
    }
}
