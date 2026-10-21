//! A conversation's transcript as a chief's client holds it (C30),
//! folded from the entries its runtime keeps, with the ops that keep
//! a client in step.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};
use hamlet_protocol::{
    Change, Item, ItemKind, ItemTool, LastActivity, Op,
};
use serde_json::Value;

use crate::harness::InputMark;
use crate::harness::runtime::{self, Block};
use crate::scrub::Scrubber;

/// Characters of a tool's arguments a transcript shows.
const SUMMARY: usize = 200;

/// The words a transcript shows where earlier entries were
/// summarised.
pub(crate) const SUMMARY_NOTICE: &str =
    "The conversation grew longer than the model can hold, so the \
     earlier part of it was summarised.";

/// A chat's items, kept in step with its conversation.
#[derive(Debug, Default)]
pub(crate) struct Fold {
    pub items: Vec<Item>,
    pub working: bool,
    pub pending: u64,
    pub last: Option<LastActivity>,
    /// An answer landed without some of what it showed being kept,
    /// which no op takes away.
    pub stale: bool,
    /// Counts each time `last` is set, even to what it was.
    lasts: u64,
    ops: Vec<Op>,
    /// The ids of tool items by their call's id.
    calls: HashMap<String, String>,
    /// The ids of the items of the answer being written, by block.
    live: BTreeMap<usize, String>,
}

impl Fold {
    /// The fold of `items`, kept so far.
    pub fn of(items: &[runtime::Item]) -> Result<Self> {
        let mut fold = Self::default();
        for item in items {
            fold.entry(item)?;
        }
        fold.ops.clear();
        Ok(fold)
    }

    /// The ops of what was folded since they were last taken.
    pub fn take(&mut self) -> Vec<Op> {
        std::mem::take(&mut self.ops)
    }

    /// Takes in what the conversation does now: whether it runs, how
    /// many inputs wait, and the answer being written.
    pub fn live(
        &mut self,
        running: bool,
        pending: u64,
        partial: Option<&runtime::Partial>,
        secrets: &[String],
    ) {
        if let Some(partial) = partial {
            self.partial(partial, secrets);
        }
        let lasts = self.lasts;
        self.status(running, pending, lasts);
    }

    fn status(&mut self, running: bool, pending: u64, lasts: u64) {
        if running != self.working
            || pending != self.pending
            || lasts != self.lasts
        {
            self.working = running;
            self.pending = pending;
            self.ops.push(Op::Status {
                working: running,
                pending,
                last: self.last.clone(),
            });
        }
    }

    /// Takes in one kept entry of the conversation.
    pub fn entry(&mut self, entry: &runtime::Item) -> Result<()> {
        let lasts = self.lasts;
        self.entry_items(entry)?;
        let (working, pending) = (self.working, self.pending);
        self.status(working, pending, lasts);
        Ok(())
    }

    fn entry_items(&mut self, entry: &runtime::Item) -> Result<()> {
        if entry.kind == runtime::ItemKind::ToolResult {
            let Some(id) = entry
                .call
                .as_ref()
                .and_then(|call| self.calls.get(call))
            else {
                return Ok(());
            };
            let id = id.clone();
            let Ending {
                exit,
                ms,
                failure,
                change,
            } = ending(entry)?;
            if let Some(item) =
                self.items.iter_mut().find(|i| i.id == id)
                && let Some(tool) = item.tool.as_mut()
            {
                tool.exit = Some(exit);
                tool.ms = ms;
                tool.failure.clone_from(&failure);
                if change.is_some() {
                    tool.change.clone_from(&change);
                }
            }
            self.ops.push(Op::Close {
                item: id,
                exit,
                ms,
                failure,
                change: change.map(Some),
            });
            if entry.tool.as_deref() == Some("ask_chief")
                && let Some(text) = super::asking::said(&entry.text)
            {
                self.add(
                    Item {
                        id: format!("{}:0", entry.entry),
                        at: entry.at,
                        kind: ItemKind::Answered,
                        text: text.into(),
                        author: None,
                        chief: None,
                        rewindable: None,
                        tool: None,
                    },
                    None,
                );
            }
            return Ok(());
        }
        let answer = entry.kind == runtime::ItemKind::Assistant;
        for (block, item) in items_of(entry)? {
            let live = if answer {
                self.live.remove(&block)
            } else {
                None
            };
            let what = if let Some(tool) = &item.tool {
                Some(format!("tool {}", tool.name))
            } else if item.kind == ItemKind::Assistant
                && entry.stop.as_deref() != Some("toolUse")
            {
                Some("replied".into())
            } else if item.kind == ItemKind::Error {
                Some("error".into())
            } else {
                None
            };
            if let Some(what) = what {
                self.last = Some(LastActivity { what, at: item.at });
                self.lasts += 1;
            }
            if item.tool.is_some()
                && let Some(Block::Call { id, .. }) = entry
                    .blocks
                    .as_ref()
                    .and_then(|blocks| blocks.get(block))
            {
                self.calls.insert(id.clone(), item.id.clone());
            }
            self.add(item, live);
        }
        if answer {
            self.stale |= !self.live.is_empty();
            self.live.clear();
            return Ok(());
        }
        // The answer being written stays last, as it is read back.
        for id in self.live.values().cloned().collect::<Vec<_>>() {
            if let Some(at) =
                self.items.iter().position(|i| i.id == id)
            {
                let item = self.items[at].clone();
                self.add(item, Some(id));
            }
        }
        Ok(())
    }

    /// Takes in the answer being written.
    fn partial(
        &mut self,
        partial: &runtime::Partial,
        secrets: &[String],
    ) {
        for (block, mut item) in
            blocks_of(&partial.blocks, partial.at, None, None, |i| {
                format!("live:{i}")
            })
        {
            if item.tool.is_some() {
                continue;
            }
            // A partial credential's beginning stays held until the
            // next chunk is kept.
            item.text = Scrubber::new(secrets).push(&item.text);
            let shown = self.live.get(&block).and_then(|id| {
                self.items.iter().position(|i| &i.id == id)
            });
            match shown {
                None => {
                    self.live.insert(block, item.id.clone());
                    self.add(item, None);
                }
                Some(at) => {
                    let shown = &mut self.items[at];
                    if item.text.len() > shown.text.len()
                        && item.text.starts_with(&shown.text)
                    {
                        let text =
                            item.text[shown.text.len()..].to_owned();
                        shown.text = item.text;
                        self.ops.push(Op::Append {
                            item: shown.id.clone(),
                            text,
                        });
                    }
                }
            }
        }
    }

    /// Adds `item` last, in the place of the item `replaced` when it
    /// is given.
    fn add(&mut self, item: Item, replaced: Option<String>) {
        if let Some(replaced) = &replaced
            && let Some(at) =
                self.items.iter().position(|i| &i.id == replaced)
        {
            self.items.remove(at);
        }
        self.ops.push(Op::Add {
            item: item.clone(),
            replaces: replaced,
        });
        self.items.push(item);
    }
}

/// The items `entry` shows, by the content block each comes from.
pub(crate) fn items_of(
    entry: &runtime::Item,
) -> Result<Vec<(usize, Item)>> {
    let id = format!("{}:0", entry.entry);
    let item = |kind, text: String, at| Item {
        at,
        author: None,
        chief: None,
        id: id.clone(),
        kind,
        rewindable: None,
        text,
        tool: None,
    };
    let mark = InputMark::of_item(entry.mark.as_ref())?;
    let at = match &mark {
        Some(mark) => {
            u64::try_from(mark.at()).with_context(|| {
                format!("a mark's time {} is before 1970", mark.at())
            })?
        }
        None => entry.at,
    };
    match mark {
        Some(InputMark::Chief { chief, text, .. }) => {
            return Ok(vec![(
                0,
                Item {
                    chief: Some(chief),
                    rewindable: Some(true),
                    ..item(ItemKind::User, text, at)
                },
            )]);
        }
        Some(InputMark::Notice { text, .. }) => {
            return Ok(vec![(0, item(ItemKind::Notice, text, at))]);
        }
        Some(InputMark::Reflection { .. }) => {
            return Ok(vec![(
                0,
                item(ItemKind::Notice, "reflection".into(), at),
            )]);
        }
        Some(InputMark::Retry { .. }) => {
            return Ok(vec![(
                0,
                item(ItemKind::Notice, entry.text.clone(), at),
            )]);
        }
        Some(
            InputMark::Letter(mail)
            | InputMark::Question(mail)
            | InputMark::Reply(mail),
        ) if entry.kind == runtime::ItemKind::User => {
            return Ok(vec![(
                0,
                Item {
                    author: mail.author,
                    ..item(ItemKind::User, entry.text.clone(), at)
                },
            )]);
        }
        _ => {}
    }
    Ok(match entry.kind {
        runtime::ItemKind::Summary => vec![(
            0,
            item(ItemKind::Notice, SUMMARY_NOTICE.into(), entry.at),
        )],
        runtime::ItemKind::User => {
            vec![(
                0,
                item(ItemKind::User, entry.text.clone(), entry.at),
            )]
        }
        runtime::ItemKind::Assistant => blocks_of(
            entry.blocks.as_deref().unwrap_or_default(),
            entry.at,
            entry.stop.as_deref(),
            entry.failure.as_deref(),
            |block| format!("{}:{block}", entry.entry),
        ),
        _ => Vec::new(),
    })
}

/// The items of an answer's content blocks, and the error it ended
/// with.
fn blocks_of(
    blocks: &[Block],
    at: u64,
    stop: Option<&str>,
    failure: Option<&str>,
    id: impl Fn(usize) -> String,
) -> Vec<(usize, Item)> {
    let item = |index, kind, text: &str| Item {
        at,
        author: None,
        chief: None,
        id: id(index),
        kind,
        rewindable: None,
        text: text.into(),
        tool: None,
    };
    let mut items = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        match block {
            Block::Call {
                arguments, name, ..
            } => {
                let arguments = serde_json::to_value(arguments)
                    .unwrap_or(Value::Null);
                let summary = arguments
                    .to_string()
                    .chars()
                    .take(SUMMARY)
                    .collect::<String>();
                let change = if name == "write" {
                    None
                } else {
                    crate::change::change_of(name, &arguments, None)
                };
                items.push((
                    index,
                    Item {
                        tool: Some(ItemTool {
                            change,
                            exit: None,
                            failure: None,
                            ms: None,
                            name: name.clone(),
                            summary,
                        }),
                        ..item(index, ItemKind::Tool, "")
                    },
                ));
            }
            Block::Text { text } if !text.trim().is_empty() => {
                items.push((
                    index,
                    item(index, ItemKind::Assistant, text),
                ));
            }
            Block::Thinking { thinking }
                if !thinking.trim().is_empty() =>
            {
                items.push((
                    index,
                    item(index, ItemKind::Reasoning, thinking),
                ));
            }
            _ => {}
        }
    }
    if let Some(stop @ ("error" | "aborted")) = stop {
        let block = blocks.len();
        items.push((
            block,
            item(block, ItemKind::Error, failure.unwrap_or(stop)),
        ));
    }
    items
}

/// The end of a tool call, as its result tells it.
struct Ending {
    exit: i64,
    ms: Option<u64>,
    failure: Option<String>,
    /// The record change the call made.
    change: Option<Change>,
}

/// The end of a tool call, from its result.
fn ending(entry: &runtime::Item) -> Result<Ending> {
    let error = entry.error == Some(true);
    let details = entry.details.as_ref();
    let code = details
        .and_then(|details| details.get("exitCode"))
        .and_then(Value::as_f64);
    let failure = error.then(|| failure_of(&entry.text));
    let change = details
        .and_then(|details| details.get("change"))
        .map(|change| {
            serde_json::from_value::<Change>(change.clone())
        })
        .transpose()
        .context("reading the change of a tool's result")?;
    Ok(Ending {
        exit: code.map_or(i64::from(error), |code| code as i64),
        ms: entry.ms,
        failure,
        change,
    })
}

/// The first line of a failed call's text that says something.
fn failure_of(text: &str) -> String {
    let text =
        text.replace("<harness>\n", "").replace("</harness>\n", "");
    let text =
        text.replace("<harness>", "").replace("</harness>", "");
    text.split('\n')
        .map(|line| line.strip_prefix("[error] ").unwrap_or(line))
        .find(|line| !line.trim().is_empty())
        .map(|line| {
            line.strip_prefix("Error: ").unwrap_or(line).to_owned()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_is_its_first_line_that_says_something() {
        assert_eq!(
            failure_of("<harness>\n[error] \nError: broken\nmore"),
            "broken"
        );
        assert_eq!(failure_of("\n\n"), "");
    }

    #[test]
    fn a_result_naming_a_change_that_does_not_read_is_an_error() {
        let result = serde_json::from_value::<runtime::Item>(serde_json::json!({
            "at": 1, "entry": "2", "kind": "tool_result", "text": "",
            "details": {"change": {"not": "a change"}},
        }))
        .unwrap();
        assert_eq!(
            ending(&result).err().unwrap().to_string(),
            "reading the change of a tool's result"
        );
    }
}
