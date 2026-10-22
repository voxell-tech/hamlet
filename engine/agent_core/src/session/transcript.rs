//! A conversation as a list of entries, built from a turn's replies
//! as they come or from a session's log.

use std::path::Path;

use bevy_ecs::prelude::*;
use hamlet_model::log;
use hamlet_rules::chief::{asking, rulings};
use serde_json::Value;

use crate::agent::{Event as AgentEvent, SUMMARY_CHARS};
use crate::session::Reply;
use crate::tools::Change;
use crate::tools::change::failure;

/// One thing said or done in a conversation.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    User {
        user: String,
        text: String,
    },
    Assistant(String),
    Reasoning(String),
    Tool {
        name: String,
        summary: String,
        exit: Option<i32>,
        /// The change it asks for, when it is an edit.
        change: Option<Change>,
    },
    Error(String),
    Notice(String),
    /// The chief's answer to the questions asked with `ask_chief`,
    /// each question quoted over its answer.
    Answered(String),
    /// Words of an agent of the hamlet to the mayor.
    Agent {
        name: String,
        text: String,
    },
}

/// An entry with its id and its time.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub entry: Entry,
    /// The time it was said, in milliseconds since 1970. 0 when the
    /// log does not say.
    pub at: u64,
}

/// The time the session a conversation is in began, in milliseconds
/// since 1970. 0 for a conversation that has only ever had one.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Began(pub u64);

impl Began {
    /// The beginning of the session kept in the log at `path`, which
    /// holds `said`.
    pub(super) fn of(path: &Path, said: &Transcript) -> Self {
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        // The sessions before it lie beside it, each named by the
        // second it was put away.
        let put_away = path
            .parent()
            .and_then(|folder| std::fs::read_dir(folder).ok())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name();
                let stamp = name
                    .to_str()?
                    .strip_prefix(&stem)?
                    .strip_prefix('.')?
                    .strip_suffix(".jsonl")?;
                stamp.parse::<u64>().ok()
            })
            .max()
            .map_or(0, |seconds| seconds * 1000);
        // A session gone back to began when it was first spoken in.
        let first = said
            .items()
            .first()
            .map(|item| item.at)
            .filter(|at| *at > 0);
        Began(first.map_or(put_away, |first| first.min(put_away)))
    }
}

/// The conversation as a list of entries, with replies folded in as
/// they stream.
#[derive(Component, Default)]
pub struct Transcript {
    items: Vec<Item>,
    next: u64,
    /// True when text of the current turn already streamed in.
    streamed: bool,
    /// True when whoever it is with was given something to answer
    /// and has not finished answering it.
    answering: bool,
    /// The tool calls that another agent made while it was asked
    /// something from here.
    helped: Vec<u64>,
    shape: u64,
}

impl Transcript {
    /// A count that moves when an entry is added or a tool ends, and
    /// stays as a reply grows.
    pub fn shape(&self) -> u64 {
        self.shape
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// True when the turn that was last begun is still being
    /// answered.
    pub fn answering(&self) -> bool {
        self.answering
    }

    /// The entry `id` with its time.
    pub fn item(&self, id: u64) -> Option<&Item> {
        // Ids only grow, with a gap where a rewind took entries out.
        let at = self
            .items
            .binary_search_by_key(&id, |item| item.id)
            .ok()?;
        self.items.get(at)
    }

    /// The entry `id`.
    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.item(id).map(|item| &item.entry)
    }

    pub fn last(&self) -> Option<&Entry> {
        self.items.last().map(|i| &i.entry)
    }

    /// True when the entry `id` is another agent's own work.
    pub fn of_another(&self, id: u64) -> bool {
        matches!(self.get(id), Some(Entry::Agent { .. }))
            || self.helped.contains(&id)
    }

    /// The id of every entry, oldest first.
    pub fn ids(&self) -> Vec<u64> {
        self.items.iter().map(|i| i.id).collect()
    }

    pub(super) fn push(&mut self, entry: Entry) {
        self.push_at(entry, log::now_millis());
    }

    fn push_at(&mut self, entry: Entry, at: u64) {
        // Something to answer begins a turn.
        if matches!(entry, Entry::User { .. }) {
            self.answering = true;
        }
        self.items.push(Item {
            id: self.next,
            entry,
            at,
        });
        self.next += 1;
        self.shape += 1;
    }

    /// Drops what was said at `at` or later.
    pub(super) fn forget_since(&mut self, at: u64) {
        let keep = self
            .items
            .iter()
            .position(|item| item.at >= at)
            .unwrap_or(self.items.len());
        self.next -= (self.items.len() - keep) as u64;
        self.items.truncate(keep);
        self.shape += 1;
    }

    /// Drops the entry `id` and everything after it, and returns the
    /// entry. No later entry is given one of their ids.
    pub(super) fn rewind(&mut self, id: u64) -> Option<Item> {
        let keep = self
            .items
            .binary_search_by_key(&id, |item| item.id)
            .ok()?;
        let gone = self.items.drain(keep..).next();
        self.helped.retain(|helped| *helped < id);
        self.streamed = false;
        self.answering = false;
        self.shape += 1;
        gone
    }

    /// The line of the log `events` that holds the message `id`, or
    /// the next message after it that reached the log. `secrets` are
    /// what the log scrubs.
    pub(super) fn line_of(
        &self,
        id: u64,
        events: &[Value],
        secrets: &[String],
    ) -> Option<usize> {
        let out = log::taken_out(events);
        let mut logged = events
            .iter()
            .enumerate()
            .filter(|(at, line)| {
                !out[*at]
                    && line["type"] == "user"
                    && line["user"] != "hamlet"
            })
            .rev()
            .peekable();
        let mut found = None;
        // A message dropped before its turn began is in no log, and
        // nothing after the last message is, so the two are matched
        // from the end. A line is written after its message is said.
        for item in self.items.iter().rev() {
            if let Entry::User { user, text } = &item.entry
                && let Some((at, line)) = logged.peek()
                && line["user"] == user.as_str()
                && line["content"]
                    == log::scrubbed(text, secrets).as_str()
                && line["ts"].as_u64().unwrap_or(0) >= item.at
            {
                found = Some(*at);
                logged.next();
            }
            if item.id == id {
                return found;
            }
        }
        None
    }

    /// Ends the tool call that is open with `code`, and with the
    /// first line of its answer when it failed.
    fn close_tool(&mut self, code: i32, why: Option<&str>) {
        let open = self.items.iter_mut().rev().find_map(|i| {
            match &mut i.entry {
                Entry::Tool { exit, change, .. }
                    if exit.is_none() =>
                {
                    Some((exit, change))
                }
                _ => None,
            }
        });
        if let Some((exit, change)) = open {
            *exit = Some(code);
            if let Some(change) = change {
                change.failure = why.map(str::to_string);
            }
            self.shape += 1;
        }
    }

    /// Folds `reply` into the conversation.
    pub fn apply(&mut self, reply: &Reply) {
        match reply {
            Reply::Event(AgentEvent::Text(t)) => {
                self.streamed = true;
                match self.items.last_mut() {
                    Some(Item {
                        entry: Entry::Assistant(s),
                        ..
                    }) => s.push_str(t),
                    _ => self.push(Entry::Assistant(t.clone())),
                }
            }
            Reply::Event(AgentEvent::Reasoning(r)) => {
                match self.items.last_mut() {
                    Some(Item {
                        entry: Entry::Reasoning(s),
                        ..
                    }) => s.push_str(r),
                    _ => self.push(Entry::Reasoning(r.clone())),
                }
            }
            Reply::Event(AgentEvent::Tool {
                name,
                summary,
                change,
            }) => self.push(Entry::Tool {
                name: name.clone(),
                summary: summary.clone(),
                exit: None,
                change: change.clone(),
            }),
            Reply::Event(AgentEvent::ToolDone {
                exit_code,
                failure,
                ..
            }) => self.close_tool(*exit_code, failure.as_deref()),
            Reply::Event(AgentEvent::Answered(said)) => {
                self.push(Entry::Answered(said.clone()))
            }
            Reply::Event(
                AgentEvent::ModelCall(_) | AgentEvent::Tell { .. },
            ) => {}
            Reply::Event(AgentEvent::Consulted { by }) => {
                self.push(Entry::Notice(format!(
                    "{by} called on the librarian"
                )))
            }
            Reply::Event(AgentEvent::Question { from, text, at }) => {
                self.push_at(
                    Entry::User {
                        user: from.clone(),
                        text: text.clone(),
                    },
                    *at,
                )
            }
            Reply::Event(AgentEvent::AgentCreated {
                name,
                purpose,
            }) => self.push(Entry::Notice(format!(
                "The mayor created `{name}`: {purpose}"
            ))),
            Reply::Event(AgentEvent::Agent { name, event }) => {
                self.apply_agent(name, event)
            }
            Reply::Event(
                event @ (AgentEvent::Message { .. }
                | AgentEvent::RoadBuilt { .. }
                | AgentEvent::AgentRemoved { .. }
                | AgentEvent::AgentsMerged { .. }
                | AgentEvent::Civic(_)),
            ) => self.apply_hamlet(event),
            Reply::Notice(n)
            | Reply::Event(AgentEvent::Notice(n)) => {
                self.push(Entry::Notice(n.clone()))
            }
            Reply::Done(text) => {
                if !self.streamed && !text.trim().is_empty() {
                    self.push(Entry::Assistant(text.clone()));
                }
                self.streamed = false;
                self.answering = false;
                self.shape += 1;
            }
            Reply::Error(e) => {
                self.push(Entry::Error(e.clone()));
                self.streamed = false;
                self.answering = false;
            }
        }
    }

    /// Folds in an event of the agent `name`, which this conversation
    /// waits on.
    fn apply_agent(&mut self, name: &str, event: &AgentEvent) {
        match event {
            AgentEvent::Text(t) => match self.items.last_mut() {
                Some(Item {
                    entry: Entry::Agent { name: last, text },
                    ..
                }) if last == name => text.push_str(t),
                _ => self.push(Entry::Agent {
                    name: name.to_string(),
                    text: t.clone(),
                }),
            },
            AgentEvent::Tool {
                name: tool,
                summary,
                change,
            } => {
                self.helped.push(self.next);
                self.push(Entry::Tool {
                    name: format!("{name}: {tool}"),
                    summary: summary.clone(),
                    exit: None,
                    change: change.clone(),
                })
            }
            AgentEvent::ToolDone {
                exit_code, failure, ..
            } => self.close_tool(*exit_code, failure.as_deref()),
            AgentEvent::Agent { name, event } => {
                self.apply_agent(name, event)
            }
            AgentEvent::Message { .. }
            | AgentEvent::RoadBuilt { .. }
            | AgentEvent::AgentRemoved { .. }
            | AgentEvent::AgentsMerged { .. }
            | AgentEvent::Civic(_) => self.apply_hamlet(event),
            _ => {}
        }
    }

    /// A line for something that happened between agents.
    fn apply_hamlet(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::Message { from, to } => {
                self.push(Entry::Notice(format!(
                    "{from} sent a message to {to}"
                )))
            }
            AgentEvent::RoadBuilt { a, b } => {
                self.push(Entry::Notice(format!(
                    "A road now joins {a} and {b}"
                )))
            }
            AgentEvent::AgentRemoved { name } => self.push(
                Entry::Notice(format!("The mayor removed `{name}`")),
            ),
            AgentEvent::AgentsMerged { kept, gone } => {
                self.push(Entry::Notice(format!(
                    "The mayor merged `{gone}` into `{kept}`"
                )))
            }
            AgentEvent::Civic(civic) => {
                self.push(Entry::Notice(civic.notice()))
            }
            _ => {}
        }
    }

    /// The conversation of a session, rebuilt from its log.
    pub fn from_log(events: &[Value]) -> Self {
        let mut t = Transcript::default();
        for e in events {
            let at = e["ts"].as_u64().unwrap_or(0);
            match e["type"].as_str() {
                Some("user") if e["user"] == "hamlet" => {
                    t.push_at(Entry::Notice("reflection".into()), at)
                }
                Some("user") => t.push_at(
                    Entry::User {
                        user: e["user"]
                            .as_str()
                            .unwrap_or_default()
                            .into(),
                        text: e["content"]
                            .as_str()
                            .unwrap_or_default()
                            .into(),
                    },
                    at,
                ),
                Some("assistant") => {
                    let m = &e["message"];
                    if let Some(text) = m["content"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                    {
                        t.push_at(Entry::Assistant(text.into()), at);
                    }
                    for c in m["tool_calls"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        let function = &c["function"];
                        let name = function["name"]
                            .as_str()
                            .unwrap_or_default();
                        let arguments = function["arguments"]
                            .as_str()
                            .unwrap_or_default();
                        let tool = Entry::Tool {
                            name: name.into(),
                            summary: arguments
                                .chars()
                                .take(SUMMARY_CHARS)
                                .collect(),
                            exit: None,
                            change: Change::of(name, arguments),
                        };
                        t.push_at(tool, at);
                    }
                }
                Some("tool") => {
                    let code =
                        e["exit_code"].as_i64().unwrap_or(-1) as i32;
                    let output = e["output"].as_str();
                    let why =
                        output.filter(|_| code != 0).map(failure);
                    t.close_tool(code, why.as_deref());
                    if e["name"] == rulings::ASK_CHIEF
                        && let Some(said) =
                            output.and_then(asking::said)
                    {
                        t.push_at(Entry::Answered(said.into()), at);
                    }
                }
                Some("notice") => t.push_at(
                    Entry::Notice(
                        e["text"].as_str().unwrap_or_default().into(),
                    ),
                    at,
                ),
                Some("turn_aborted") => t.push_at(
                    Entry::Error(
                        e["reason"]
                            .as_str()
                            .unwrap_or("aborted")
                            .into(),
                    ),
                    at,
                ),
                _ => {}
            }
        }
        // A conversation read back from a log is over.
        t.answering = false;
        t
    }
}
