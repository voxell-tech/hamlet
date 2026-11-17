//! A watched chat's items, kept up to date by its live operations.

use crate::client::{AgentId, Change, Item, TranscriptOperation};

/// An item of a chat as a client keeps it.
pub trait Kept {
    /// The server's id of the item.
    fn id(&self) -> &str;

    /// Adds streamed words to the item's text.
    fn append(&mut self, text: &str);

    /// Ends the tool call the item holds. False when it holds none.
    fn close(&mut self, closed: Closed<'_>) -> bool;

    /// Whether it is a line its turn has not taken up yet.
    fn is_queued(&self) -> bool;
}

/// The end of a tool call, as a [`TranscriptOperation::Close`] gives
/// it.
#[derive(Clone, Copy, Debug)]
pub struct Closed<'a> {
    pub exit: i64,
    pub ms: Option<u64>,
    pub failure: Option<&'a str>,
    /// The completed write's change, when the operation carries one.
    pub change: Option<Option<&'a Change>>,
    /// The agent a society call reached.
    pub agent: Option<AgentId>,
}

/// An item a [`TranscriptOperation`] refers to that a chat's items
/// do not hold as it says.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Mismatch {
    /// No item has this id.
    #[error("No item {0}")]
    Missing(String),
    /// The item with this id holds no tool call to close.
    #[error("{0} is no tool call")]
    NoToolCall(String),
}

/// Applies `operation` to the `items` of a chat, making an added item
/// with `keep`. `Err` names an item the operation refers to that is
/// missing from `items` or holds no tool call; the rest of the
/// operation still applies.
pub fn apply<T: Kept>(
    items: &mut Vec<T>,
    operation: &TranscriptOperation,
    keep: impl FnOnce(&Item) -> T,
) -> Result<(), Mismatch> {
    let missing = |id: &str| Err(Mismatch::Missing(id.to_owned()));
    match operation {
        TranscriptOperation::Add { item, replaces } => {
            let at = replaces.as_deref().map(|id| {
                items
                    .iter()
                    .position(|kept| kept.id() == id)
                    .ok_or(id)
            });
            if let Some(Ok(at)) = at {
                items.remove(at);
            }
            // Queued items stay last, so what is taken up goes
            // before them.
            let place = if item.queued {
                items.len()
            } else {
                items
                    .iter()
                    .position(Kept::is_queued)
                    .unwrap_or(items.len())
            };
            items.insert(place, keep(item));
            match at {
                Some(Err(id)) => missing(id),
                _ => Ok(()),
            }
        }
        TranscriptOperation::Append { item, text } => {
            match items.iter_mut().find(|kept| kept.id() == item) {
                Some(kept) => {
                    kept.append(text);
                    Ok(())
                }
                None => missing(item),
            }
        }
        TranscriptOperation::Close {
            agent,
            item,
            exit,
            ms,
            failure,
            change,
        } => {
            let closed = Closed {
                exit: *exit,
                ms: *ms,
                failure: failure.as_deref(),
                change: change.as_ref().map(Option::as_ref),
                agent: *agent,
            };
            match items
                .iter_mut()
                .find(|kept| kept.id() == item)
                .map(|kept| kept.close(closed))
            {
                Some(true) => Ok(()),
                Some(false) => {
                    Err(Mismatch::NoToolCall(item.clone()))
                }
                None => missing(item),
            }
        }
        TranscriptOperation::Remove { item } => {
            match items.iter().position(|kept| kept.id() == item) {
                Some(at) => {
                    items.remove(at);
                    Ok(())
                }
                None => missing(item),
            }
        }
        TranscriptOperation::Reset {} => {
            items.clear();
            Ok(())
        }
        TranscriptOperation::Status { .. }
        | TranscriptOperation::Usage { .. } => Ok(()),
    }
}

impl Kept for Item {
    fn id(&self) -> &str {
        &self.id
    }

    fn append(&mut self, text: &str) {
        self.text.push_str(text);
    }

    fn close(&mut self, closed: Closed<'_>) -> bool {
        let Some(tool) = &mut self.tool else {
            return false;
        };
        tool.exit = Some(closed.exit);
        tool.ms = closed.ms;
        tool.failure = closed.failure.map(str::to_owned);
        if let Some(change) = closed.change {
            tool.change = change.cloned();
        }
        if closed.agent.is_some() {
            tool.agent = closed.agent;
        }
        true
    }

    fn is_queued(&self) -> bool {
        self.queued
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fold(
        items: &mut Vec<Item>,
        operation: serde_json::Value,
    ) -> Result<(), Mismatch> {
        apply(
            items,
            &serde_json::from_value(operation).unwrap(),
            Item::clone,
        )
    }

    #[test]
    fn a_durable_item_replaces_its_streamed_one_at_the_end() {
        let mut items = Vec::new();
        let item = |id: &str, text: &str| json!({"id": id, "at": 1, "kind": "assistant", "text": text});
        let tool = json!({"id": "t", "at": 1, "kind": "tool", "text": "",
            "tool": {"name": "run", "summary": "", "exit": null,
                     "ms": null, "failure": null, "change": null}});
        fold(
            &mut items,
            json!({"op": "add", "item": item("live", "Hel")}),
        )
        .unwrap();
        fold(&mut items, json!({"op": "add", "item": tool})).unwrap();
        fold(
            &mut items,
            json!({"op": "append", "item": "live", "text": "lo"}),
        )
        .unwrap();
        fold(
            &mut items,
            json!({"op": "close", "item": "t", "exit": 7,
            "ms": 3, "failure": "broken"}),
        )
        .unwrap();
        fold(
            &mut items,
            json!({"op": "add", "replaces": "live",
            "item": item("entry", "Hello")}),
        )
        .unwrap();
        let ids =
            items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, ["t", "entry"]);
        assert_eq!(items[1].text, "Hello");
        let tool = items[0].tool.as_ref().unwrap();
        assert_eq!((tool.exit, tool.ms), (Some(7), Some(3)));
        assert_eq!(tool.failure.as_deref(), Some("broken"));
        assert_eq!(
            fold(
                &mut items,
                json!({"op": "close", "item": "entry", "exit": 0,
                "ms": null, "failure": null})
            ),
            Err(Mismatch::NoToolCall("entry".into()))
        );
        // The added item is kept even when the one it replaces is
        // gone.
        assert_eq!(
            fold(
                &mut items,
                json!({"op": "add", "replaces": "live",
                "item": item("again", "")})
            ),
            Err(Mismatch::Missing("live".into()))
        );
        assert_eq!(items.len(), 3);
        fold(&mut items, json!({"op": "reset"})).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn queued_lines_stay_last_until_taken_up() {
        let mut items = Vec::new();
        let line = |id: &str, queued: bool| json!({"id": id, "at": 1, "kind": "user", "text": "", "queued": queued});
        for id in ["q1", "q2"] {
            fold(
                &mut items,
                json!({"op": "add", "item": line(id, true)}),
            )
            .unwrap();
        }
        fold(
            &mut items,
            json!({"op": "add", "item": line("a", false)}),
        )
        .unwrap();
        fold(
            &mut items,
            json!({"op": "add", "replaces": "q1", "item": line("1", false)}),
        )
        .unwrap();
        fold(&mut items, json!({"op": "remove", "item": "q2"}))
            .unwrap();
        let ids =
            items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, ["a", "1"]);
    }
}
