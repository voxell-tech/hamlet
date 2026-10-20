//! A watched chat's items, kept up to date by its live ops.

use crate::v0::{Change, Item, Op};

/// An item of a chat as a client keeps it.
pub trait Kept {
    /// The server's id of the item.
    fn id(&self) -> &str;

    /// Adds streamed words to the item's text.
    fn append(&mut self, text: &str);

    /// Ends the tool call the item holds. False when it holds none.
    fn close(&mut self, closed: Closed<'_>) -> bool;
}

/// The end of a tool call, as an [`Op::Close`] gives it.
#[derive(Clone, Copy, Debug)]
pub struct Closed<'a> {
    pub exit: i64,
    pub ms: Option<u64>,
    pub failure: Option<&'a str>,
    /// The completed write's change, when the op carries one.
    pub change: Option<Option<&'a Change>>,
}

/// Applies `op` to the `items` of a chat, making an added item with
/// `keep`. `Err` names an item the op refers to that is missing from
/// `items` or holds no tool call; the rest of the op still applies.
pub fn apply<T: Kept>(
    items: &mut Vec<T>,
    op: &Op,
    keep: impl FnOnce(&Item) -> T,
) -> Result<(), String> {
    let missing = |id: &str| Err(format!("No item {id}"));
    match op {
        Op::Add { item, replaces } => {
            let at = replaces.as_deref().map(|id| {
                items
                    .iter()
                    .position(|kept| kept.id() == id)
                    .ok_or(id)
            });
            if let Some(Ok(at)) = at {
                items.remove(at);
            }
            items.push(keep(item));
            match at {
                Some(Err(id)) => missing(id),
                _ => Ok(()),
            }
        }
        Op::Append { item, text } => {
            match items.iter_mut().find(|kept| kept.id() == item) {
                Some(kept) => {
                    kept.append(text);
                    Ok(())
                }
                None => missing(item),
            }
        }
        Op::Close {
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
            };
            match items
                .iter_mut()
                .find(|kept| kept.id() == item)
                .map(|kept| kept.close(closed))
            {
                Some(true) => Ok(()),
                Some(false) => Err(format!("{item} is no tool call")),
                None => missing(item),
            }
        }
        Op::Reset {} => {
            items.clear();
            Ok(())
        }
        Op::Status { .. } | Op::Usage { .. } => Ok(()),
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
        true
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fold(
        items: &mut Vec<Item>,
        op: serde_json::Value,
    ) -> Result<(), String> {
        apply(
            items,
            &serde_json::from_value(op).unwrap(),
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
        assert!(
            fold(
                &mut items,
                json!({"op": "close", "item": "entry", "exit": 0,
                "ms": null, "failure": null})
            )
            .is_err()
        );
        // The added item is kept even when the one it replaces is
        // gone.
        assert!(
            fold(
                &mut items,
                json!({"op": "add", "replaces": "live",
                "item": item("again", "")})
            )
            .is_err()
        );
        assert_eq!(items.len(), 3);
        fold(&mut items, json!({"op": "reset"})).unwrap();
        assert!(items.is_empty());
    }
}
