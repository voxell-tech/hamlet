//! A conversation's transcript as a chief's client holds it (C30),
//! folded from the entries its runtime keeps, with the operations
//! that keep a client in step.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};
use hamlet_protocol::client::{
    AgentId as WireAgent, Change, Item, ItemKind, ItemTool,
    LastActivity, ToolCall, TranscriptOperation,
};
use hamlet_protocol::runtime::{self, Block};
use hamlet_sandbox::scrub::Scrubber;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ConversationId};
use serde_json::Value;

use crate::conversations::marks::{From, Mark};
use crate::conversations::{QUEUED, inputs};
use crate::tools::catalogue::Society;

/// Characters of a tool's arguments a transcript shows.
const SUMMARY: usize = 200;

/// A chat's items, kept in step with its conversation.
#[derive(Debug, Default)]
pub(crate) struct Fold {
    pub items: Vec<Item>,
    pub working: bool,
    pub pending: u64,
    pub last: Option<LastActivity>,
    /// A chief's stop holds what waits.
    pub held: bool,
    /// The `held` the last status carried.
    held_sent: bool,
    /// An answer landed without some of what it showed being kept,
    /// which no operation takes away.
    pub stale: bool,
    /// Counts each time `last` is set, even to what it was.
    lasts: u64,
    operations: Vec<TranscriptOperation>,
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
        fold.operations.clear();
        Ok(fold)
    }

    /// The operations of what was folded since they were last taken.
    pub fn take(&mut self) -> Vec<TranscriptOperation> {
        std::mem::take(&mut self.operations)
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
            || self.held != self.held_sent
        {
            self.working = running;
            self.pending = pending;
            self.held_sent = self.held;
            self.operations.push(TranscriptOperation::Status {
                held: self.held,
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
                millis,
                failure,
                change,
                agent,
            } = ending(entry)?;
            if let Some(item) =
                self.items.iter_mut().find(|i| i.id == id)
                && let Some(tool) = item.tool.as_mut()
            {
                tool.exit = Some(exit);
                tool.ms = millis;
                tool.failure.clone_from(&failure);
                if change.is_some() {
                    tool.change.clone_from(&change);
                }
                if agent.is_some() {
                    tool.agent = agent;
                }
            }
            self.operations.push(TranscriptOperation::Close {
                agent,
                item: id,
                exit,
                ms: millis,
                failure,
                change: change.map(Some),
            });
            if entry.tool.as_deref().and_then(Society::named)
                == Some(Society::RequestChief)
                && let Some(text) =
                    crate::conversations::questions::said(&entry.text)
            {
                self.add(
                    Item {
                        id: format!("{}:0", entry.entry),
                        at: entry.at,
                        files: Vec::new(),
                        from: None,
                        images: Vec::new(),
                        kind: ItemKind::Answered,
                        queued: false,
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
        // The line a turn took up stands where it waited.
        let taken_up = entry
            .request
            .as_ref()
            .map(|request| format!("{QUEUED}{request}"))
            .filter(|id| self.items.iter().any(|i| &i.id == id));
        for (block, item) in items_of(entry)? {
            let live = if answer {
                self.live.remove(&block)
            } else if block == 0 {
                taken_up.clone()
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
                        self.operations.push(
                            TranscriptOperation::Append {
                                item: shown.id.clone(),
                                text,
                            },
                        );
                    }
                }
            }
        }
    }

    /// Adds `item` last, before the queued lines unless it is one, in
    /// the place of the item `replaced` when it is given.
    fn add(&mut self, item: Item, replaced: Option<String>) {
        if let Some(replaced) = &replaced
            && let Some(at) =
                self.items.iter().position(|i| &i.id == replaced)
        {
            self.items.remove(at);
        }
        self.operations.push(TranscriptOperation::Add {
            item: item.clone(),
            replaces: replaced,
        });
        let at = if item.queued {
            self.items.len()
        } else {
            self.items
                .iter()
                .position(|i| i.queued)
                .unwrap_or(self.items.len())
        };
        self.items.insert(at, item);
    }

    /// Shows `queue`: its lines in their order, those no longer
    /// waiting and not taken up yet removed, and whether a stop holds
    /// them.
    pub fn queue(&mut self, queue: Queue) {
        let Queue {
            items: queued,
            held,
        } = queue;
        if held != self.held {
            self.held = held;
            let (working, pending, lasts) =
                (self.working, self.pending, self.lasts);
            self.status(working, pending, lasts);
        }
        let shown = self
            .items
            .iter()
            .filter(|i| i.queued)
            .map(|i| i.id.clone())
            .collect::<Vec<_>>();
        let wanted =
            queued.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        if shown == wanted {
            return;
        }
        for id in shown {
            self.items.retain(|i| i.id != id);
            self.operations
                .push(TranscriptOperation::Remove { item: id });
        }
        for item in queued {
            self.add(item, None);
        }
    }
}

/// The lines of a conversation that wait for a turn, as a transcript
/// shows them, and whether a chief's stop holds them.
pub(crate) struct Queue {
    pub items: Vec<Item>,
    pub held: bool,
}

/// What waits in `conversation`.
pub(crate) fn waiting(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<Queue> {
    Ok(Queue {
        items: queued(db, conversation)?,
        held: crate::conversations::get(db, conversation)?.held,
    })
}

/// The lines of `conversation` that wait for a turn, as a transcript
/// shows them.
pub(crate) fn queued(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<Vec<Item>> {
    let mut out = Vec::new();
    for input in inputs::queued(db, conversation)? {
        let mark = input.mark()?;
        let at = u64::try_from(mark.at()).unwrap_or_default();
        let item = |kind, text: String| Item {
            at,
            author: None,
            chief: None,
            files: Vec::new(),
            from: None,
            id: format!("{QUEUED}{}", input.request),
            images: Vec::new(),
            kind,
            queued: true,
            rewindable: None,
            text,
            tool: None,
        };
        out.push(match mark {
            Mark::Chief {
                chief,
                text,
                images,
                files,
                ..
            } => Item {
                chief: Some(chief),
                images,
                files,
                ..item(ItemKind::User, text)
            },
            Mark::Notification(from)
            | Mark::Request(from)
            | Mark::Response(from) => {
                let from = sender(from);
                Item {
                    author: from.author,
                    from: from.from,
                    images: from.images,
                    ..item(ItemKind::User, from.text)
                }
            }
            Mark::Retry { .. } => {
                item(ItemKind::Notice, input.content.clone())
            }
            _ => continue,
        });
    }
    Ok(out)
}

/// What a row of a message from another agent shows of it.
struct Sender {
    author: Option<String>,
    from: Option<WireAgent>,
    text: String,
    images: Vec<hamlet_protocol::client::AttachedImage>,
}

fn sender(from: From) -> Sender {
    Sender {
        images: from.images(),
        author: Some(from.author),
        from: wire(from.from),
        text: from.text,
    }
}

fn wire(agent: AgentId) -> Option<WireAgent> {
    u64::try_from(agent.0).ok().map(WireAgent)
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
        files: Vec::new(),
        from: None,
        id: id.clone(),
        images: Vec::new(),
        kind,
        queued: false,
        rewindable: None,
        text,
        tool: None,
    };
    let mark = Mark::of_item(entry.mark.as_ref())?;
    let at = match &mark {
        Some(mark) => {
            u64::try_from(mark.at()).with_context(|| {
                format!("a mark's time {} is before 1970", mark.at())
            })?
        }
        None => entry.at,
    };
    match mark {
        Some(Mark::Chief {
            chief,
            text,
            images,
            files,
            ..
        }) => {
            return Ok(vec![(
                0,
                Item {
                    chief: Some(chief),
                    files,
                    images,
                    rewindable: Some(true),
                    ..item(ItemKind::User, text, at)
                },
            )]);
        }
        Some(Mark::Notice { text, .. }) => {
            return Ok(vec![(0, item(ItemKind::Notice, text, at))]);
        }
        Some(Mark::Earlier { chief, .. }) => {
            return Ok(vec![(
                0,
                Item {
                    chief: Some(chief),
                    ..item(ItemKind::User, entry.text.clone(), at)
                },
            )]);
        }
        Some(Mark::Step { tool, summary, .. }) => {
            // Another program's tool, whose shortened input may not
            // be JSON.
            let call = serde_json::from_str::<Value>(&summary)
                .map_or(ToolCall::Other, |arguments| {
                    plain_call(&arguments)
                });
            return Ok(vec![(
                0,
                Item {
                    tool: Some(ItemTool {
                        agent: None,
                        call,
                        change: None,
                        exit: Some(0),
                        failure: None,
                        ms: None,
                        name: tool,
                        summary,
                    }),
                    ..item(ItemKind::Tool, String::new(), at)
                },
            )]);
        }
        Some(Mark::Reflection { .. }) => {
            return Ok(vec![(
                0,
                item(ItemKind::Notice, "reflection".into(), at),
            )]);
        }
        Some(Mark::Retry { .. }) => {
            return Ok(vec![(
                0,
                item(ItemKind::Notice, entry.text.clone(), at),
            )]);
        }
        Some(
            Mark::Notification(message)
            | Mark::Request(message)
            | Mark::Response(message)
            | Mark::ErrorResponse(message),
        ) if entry.kind == runtime::ItemKind::User => {
            let from = sender(message);
            return Ok(vec![(
                0,
                Item {
                    author: from.author,
                    from: from.from,
                    images: from.images,
                    ..item(ItemKind::User, from.text, at)
                },
            )]);
        }
        Some(Mark::Merged { from, author, .. }) => {
            return Ok(vec![(
                0,
                Item {
                    from: wire(from),
                    ..item(ItemKind::Merged, author, at)
                },
            )]);
        }
        _ => {}
    }
    Ok(match entry.kind {
        runtime::ItemKind::Summary => vec![(
            0,
            item(ItemKind::Summary, entry.text.clone(), entry.at),
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
        files: Vec::new(),
        from: None,
        id: id(index),
        images: Vec::new(),
        kind,
        queued: false,
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
                    crate::tools::edit_diff::change_of(
                        name, &arguments, None,
                    )
                };
                items.push((
                    index,
                    Item {
                        tool: Some(ItemTool {
                            agent: None,
                            call: call_of(name, &arguments),
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
    millis: Option<u64>,
    failure: Option<String>,
    /// The record change the call made.
    change: Option<Change>,
    /// The agent a society call reached.
    agent: Option<WireAgent>,
}

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
    let agent = details
        .and_then(|details| details.get("agent"))
        .and_then(Value::as_u64)
        .map(WireAgent);
    Ok(Ending {
        exit: code.map_or(i64::from(error), |code| code as i64),
        millis: entry.ms,
        failure,
        change,
        agent,
    })
}

/// What the row of a call of `name` with `arguments` shows.
fn call_of(name: &str, arguments: &Value) -> ToolCall {
    let text = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    match (Society::named(name), text("to"), text("text")) {
        (Some(Society::Notify), Some(to), Some(text)) => {
            ToolCall::Notification { to, text }
        }
        (Some(Society::Request), Some(to), Some(text)) => {
            ToolCall::Request { to, text }
        }
        (Some(Society::StartWorker), ..)
            if let Some(task) = text("task") =>
        {
            ToolCall::Worker { task }
        }
        _ => plain_call(arguments),
    }
}

/// The command or the path a call's `arguments` name, which tells
/// one call from the next.
fn plain_call(arguments: &Value) -> ToolCall {
    let text = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    if let Some(command) = text("command") {
        return ToolCall::Command { command };
    }
    match text("path") {
        // A search's pattern tells it apart more than its path.
        Some(path) if arguments.get("pattern").is_none() => {
            ToolCall::Path { path }
        }
        _ => ToolCall::Other,
    }
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
