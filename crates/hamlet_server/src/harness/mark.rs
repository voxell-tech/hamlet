//! What an input is, as the server marks it for its runtime and its
//! transcript. The runtime protocol carries it as an untyped
//! [`Mark`].

use anyhow::{Context, Result};
use hamlet_protocol::runtime::Mark;
use serde::{Deserialize, Serialize};

use crate::record::AgentRef;

/// The mark of an input or a write.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum InputMark {
    /// A chief's line.
    #[serde(rename = "hamlet.chief")]
    Chief {
        chief: String,
        text: String,
        task: String,
        at: i64,
    },
    /// A failed or stopped turn carried on.
    #[serde(rename = "hamlet.retry")]
    Retry { task: String, at: i64 },
    #[serde(rename = "hamlet.reflection")]
    Reflection { task: String, at: i64 },
    /// Words for people alone, which the model never sees (C30).
    #[serde(rename = "hamlet.notice")]
    Notice { text: String, at: i64 },
    #[serde(rename = "hamlet.letter")]
    Letter(Mail),
    #[serde(rename = "hamlet.question")]
    Question(Mail),
    #[serde(rename = "hamlet.reply")]
    Reply(Mail),
    /// A one-off errand's instructions.
    #[serde(rename = "hamlet.errand")]
    Errand { task: String, at: i64 },
}

/// A letter, question or reply from another agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mail {
    pub from: AgentRef,
    pub task: String,
    pub at: i64,
    /// The conversation a letter's reply goes to.
    #[serde(
        rename = "replyTo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reply_to: Option<i64>,
    /// The sender as a transcript shows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

impl InputMark {
    /// The input mark of the protocol's `mark`; `None` for a kind
    /// outside `hamlet.`, which the server does not make.
    pub fn of(mark: &Mark) -> Result<Option<Self>> {
        if !mark.kind.starts_with("hamlet.") {
            return Ok(None);
        }
        serde_json::from_value(serde_json::to_value(mark)?)
            .map(Some)
            .with_context(|| {
                format!("reading a `{}` mark", mark.kind)
            })
    }

    /// [`Self::of`] the mark of an item that may have none.
    pub fn of_item(mark: Option<&Mark>) -> Result<Option<Self>> {
        mark.map_or(Ok(None), Self::of)
    }

    /// A notice of `text`, made now.
    pub fn notice(text: &str) -> Self {
        Self::Notice {
            text: text.into(),
            at: crate::clock::now(),
        }
    }

    /// The H12 task of its turn.
    pub fn task(&self) -> Option<&str> {
        match self {
            Self::Chief { task, .. }
            | Self::Retry { task, .. }
            | Self::Reflection { task, .. }
            | Self::Errand { task, .. } => Some(task),
            Self::Letter(message)
            | Self::Question(message)
            | Self::Reply(message) => Some(&message.task),
            Self::Notice { .. } => None,
        }
    }

    pub fn at(&self) -> i64 {
        match self {
            Self::Chief { at, .. }
            | Self::Retry { at, .. }
            | Self::Reflection { at, .. }
            | Self::Notice { at, .. }
            | Self::Errand { at, .. } => *at,
            Self::Letter(message)
            | Self::Question(message)
            | Self::Reply(message) => message.at,
        }
    }

    /// The message from another agent it marks.
    pub fn message(&self) -> Option<&Mail> {
        match self {
            Self::Letter(message)
            | Self::Question(message)
            | Self::Reply(message) => Some(message),
            _ => None,
        }
    }

    /// Whether its turn is one a chief asked for: a line or a retry.
    pub fn chief_turn(&self) -> bool {
        matches!(self, Self::Chief { .. } | Self::Retry { .. })
    }
}

impl From<&InputMark> for Mark {
    fn from(mark: &InputMark) -> Self {
        serde_json::from_value(
            serde_json::to_value(mark).expect("a mark is JSON"),
        )
        .expect("a mark has a kind and data")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_kept_mark_reads_as_its_kind() {
        let kept = json!({"kind": "hamlet.letter", "data": {
            "from": {"hamlet": "home", "number": 2}, "task": "t",
            "at": 5, "replyTo": 3}});
        let mark =
            serde_json::from_value::<Mark>(kept.clone()).unwrap();
        let typed = InputMark::of(&mark).unwrap().unwrap();
        assert_eq!(
            typed.message().map(|m| (m.from.number, m.reply_to)),
            Some((2, Some(3)))
        );
        assert_eq!(
            serde_json::to_value(Mark::from(&typed)).unwrap(),
            kept
        );
    }

    #[test]
    fn a_malformed_hamlet_mark_is_an_error_and_another_kind_is_none()
    {
        let mark = |kept: serde_json::Value| {
            serde_json::from_value::<Mark>(kept).unwrap()
        };
        let bare = mark(json!({"kind": "hamlet.notice", "data": {}}));
        assert_eq!(
            InputMark::of(&bare).unwrap_err().to_string(),
            "reading a `hamlet.notice` mark"
        );
        let foreign = mark(json!({"kind": "other.note", "data": {}}));
        assert_eq!(InputMark::of(&foreign).unwrap(), None);
    }
}
