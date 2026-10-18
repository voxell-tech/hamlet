//! What one conversation's agent is doing and saying, folded from
//! what its runtime tells as it happens.

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use crate::harness::link::{self, Block};

/// The most bytes of what an agent says that are shown.
const SAID: usize = 600;

/// The words of each thing an agent can be doing.
pub(crate) fn words(doing: &str) -> &'static str {
    match doing {
        "think" => "Thinking",
        "read-file" => "Reading a file",
        "read-notes" => "Reading its notes",
        "read-skill" => "Reading a skill",
        "write-file" => "Writing a file",
        "write-notes" => "Writing its notes",
        "write-skill" => "Writing a skill",
        "run" => "Running a command",
        "fetch" => "Fetching from the network",
        "custom" => "Using a tool added by hand",
        "await" => "Waiting for an agent's answer",
        _ => "At rest",
    }
}

/// The work and words of one conversation.
#[derive(Clone, Debug)]
pub(crate) struct Activity {
    pub doing: &'static str,
    pub step: i64,
    pub failures: i64,
    pub saying: String,
    pub thought: bool,
    pub working: bool,
    pub pending: i64,
    /// The last answer ended in failure or was stopped.
    pub faulted: bool,
    blocks: BTreeMap<usize, String>,
    calls: HashSet<String>,
}

impl Activity {
    /// The activity of a conversation at rest, as its kept `items`
    /// leave it.
    pub fn of(items: &[link::Item]) -> Self {
        let mut activity = Self {
            doing: "rest",
            step: 0,
            failures: 0,
            saying: String::new(),
            thought: false,
            working: false,
            pending: 0,
            faulted: false,
            blocks: BTreeMap::new(),
            calls: HashSet::new(),
        };
        if let Some(last) = items
            .iter()
            .rev()
            .find(|item| item.kind == link::ItemKind::Assistant)
        {
            activity.faulted = matches!(
                last.stop.as_deref(),
                Some("error" | "aborted")
            );
            for block in last.blocks.iter().flatten() {
                match block {
                    Block::Text { text } => {
                        activity.speak(text, false)
                    }
                    Block::Thinking { thinking } => {
                        activity.speak(thinking, true);
                    }
                    Block::Call { .. } => {}
                }
            }
        }
        activity
    }

    /// Takes in whether the conversation runs, how many inputs wait,
    /// and the answer being written.
    pub fn live(
        &mut self,
        running: bool,
        pending: i64,
        partial: Option<&link::Partial>,
    ) {
        self.pending = pending;
        if running && !self.working {
            self.calls.clear();
            self.blocks.clear();
            self.rest();
            self.working = true;
            self.faulted = false;
            self.saying.clear();
            self.thought = false;
        }
        if let Some(partial) = partial {
            for (index, block) in partial.blocks.iter().enumerate() {
                if !matches!(block, Block::Call { .. }) {
                    self.block(block, index);
                }
            }
        }
        if !running && self.working {
            self.working = false;
            self.rest();
        }
    }

    /// A model call of the turn begins.
    pub fn attempted(&mut self) {
        self.blocks.clear();
        self.think();
    }

    /// Takes in one kept entry.
    pub fn kept(&mut self, item: &link::Item) {
        match item.kind {
            link::ItemKind::Assistant => {
                for (index, block) in
                    item.blocks.iter().flatten().enumerate()
                {
                    self.block(block, index);
                }
                self.faulted = matches!(
                    item.stop.as_deref(),
                    Some("error" | "aborted")
                );
                self.blocks.clear();
            }
            link::ItemKind::ToolResult => {
                self.ended(item.error == Some(true));
            }
            _ => {}
        }
    }

    fn block(&mut self, block: &Block, index: usize) {
        let (text, thought) = match block {
            Block::Call {
                id,
                name,
                arguments,
            } => {
                if self.calls.insert(id.clone()) {
                    self.tool(name, arguments);
                }
                return;
            }
            Block::Text { text } => (text, false),
            Block::Thinking { thinking } => (thinking, true),
        };
        let previous = self.blocks.insert(index, text.clone());
        let previous = previous.unwrap_or_default();
        if *text == previous {
            return;
        }
        self.think();
        match text.strip_prefix(previous.as_str()) {
            Some(delta) => self.speak(delta, thought),
            None => self.speak(text, thought),
        }
    }

    fn rest(&mut self) {
        self.doing = "rest";
        self.step = 0;
        self.failures = 0;
    }

    fn think(&mut self) {
        if self.doing == "think" {
            return;
        }
        self.doing = "think";
        self.step += 1;
    }

    fn tool(
        &mut self,
        name: &str,
        arguments: &std::collections::BTreeMap<String, Value>,
    ) {
        self.doing = doing_of(name, arguments);
        self.step += 1;
        self.saying.clear();
        self.thought = true;
    }

    fn ended(&mut self, failed: bool) {
        if failed {
            self.failures += 1;
        }
        self.doing = "think";
        self.step += 1;
    }

    fn speak(&mut self, text: &str, thought: bool) {
        if thought != self.thought {
            self.saying.clear();
            self.thought = thought;
        }
        let said = format!("{}{text}", self.saying);
        self.saying = if thought {
            let mut end = 0;
            for (at, c) in said.char_indices() {
                if at + c.len_utf8() > SAID {
                    break;
                }
                end = at + c.len_utf8();
            }
            said[..end].to_owned()
        } else {
            let mut start = said.len();
            for (at, _) in said.char_indices().rev() {
                if said.len() - at > SAID {
                    break;
                }
                start = at;
            }
            said[start..].to_owned()
        };
    }
}

/// What a call of `name` with `arguments` has its agent doing.
fn doing_of(
    name: &str,
    arguments: &std::collections::BTreeMap<String, Value>,
) -> &'static str {
    let text = serde_json::to_string(arguments)
        .unwrap_or_default()
        .to_lowercase();
    let paper = if text.contains("notes.md") {
        "notes"
    } else if text.contains("skills/") || text.contains("skill.md") {
        "skill"
    } else {
        "file"
    };
    match name {
        "read" | "search" | "find" => match paper {
            "notes" => "read-notes",
            "skill" => "read-skill",
            _ => "read-file",
        },
        "edit" | "write" => match paper {
            "notes" => "write-notes",
            "skill" => "write-skill",
            _ => "write-file",
        },
        "bash" | "powershell" => {
            if text.contains("skill.md") {
                "read-skill"
            } else if [
                "curl ",
                "wget ",
                "http://",
                "https://",
                "web-search",
                "read-webpage",
                "weather",
                "call-api",
            ]
            .iter()
            .any(|word| text.contains(word))
            {
                "fetch"
            } else if text.contains("notes.md") {
                "read-notes"
            } else {
                "run"
            }
        }
        "create_agent" => "think",
        "ask_agent" | "request_road" | "agree_road"
        | "agree_move" | "ask_librarian" | "submit_skill" => "await",
        _ => "custom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_bytes_said_and_the_first_thought() {
        let mut activity = Activity::of(&[]);
        activity.speak(&("é".repeat(400) + "done"), false);
        assert_eq!(activity.saying, "é".repeat(298) + "done");
        activity.speak(&"é".repeat(400), true);
        assert_eq!(activity.saying, "é".repeat(300));
    }
}
