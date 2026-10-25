//! What one conversation's agent is doing and saying, folded from
//! what its runtime tells as it happens.

use std::collections::{BTreeMap, HashSet};

use hamlet_protocol::client::Doing;
use hamlet_protocol::runtime::{self, Block};
use serde_json::Value;

/// The most bytes of what an agent says that are shown.
const SAID: usize = 600;

/// The words of each thing an agent can be doing.
pub(crate) fn words(doing: &Doing) -> &'static str {
    match doing {
        Doing::Rest => "At rest",
        Doing::Think => "Thinking",
        Doing::ReadFile => "Reading a file",
        Doing::ReadNotes => "Reading its notes",
        Doing::ReadSkill => "Reading a skill",
        Doing::WriteFile => "Writing a file",
        Doing::WriteNotes => "Writing its notes",
        Doing::WriteSkill => "Writing a skill",
        Doing::Run => "Running a command",
        Doing::Fetch => "Fetching from the network",
        Doing::Custom => "Using a tool added by hand",
        Doing::Await => "Waiting for an agent's answer",
    }
}

/// The work and words of one conversation.
#[derive(Clone, Debug)]
pub(crate) struct Activity {
    pub doing: Doing,
    pub step: i64,
    pub failures: i64,
    pub saying: String,
    pub thought: bool,
    pub working: bool,
    pub pending: u64,
    /// The last answer ended in failure or was stopped.
    pub faulted: bool,
    blocks: BTreeMap<usize, String>,
    calls: HashSet<String>,
}

impl Activity {
    /// The activity of a conversation at rest, as its kept `items`
    /// leave it.
    pub fn of(items: &[runtime::Item]) -> Self {
        let mut activity = Self {
            doing: Doing::Rest,
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
            .find(|item| item.kind == runtime::ItemKind::Assistant)
        {
            activity.faulted = matches!(
                last.stop.as_deref(),
                Some("error" | "aborted")
            );
            for block in last.blocks.iter().flatten() {
                match block {
                    Block::Text { text } => {
                        activity.speak(text, false);
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
        pending: u64,
        partial: Option<&runtime::Partial>,
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
    pub fn kept(&mut self, item: &runtime::Item) {
        match item.kind {
            runtime::ItemKind::Assistant => {
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
            runtime::ItemKind::ToolResult => {
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
        self.doing = Doing::Rest;
        self.step = 0;
        self.failures = 0;
    }

    fn think(&mut self) {
        if self.doing == Doing::Think {
            return;
        }
        self.doing = Doing::Think;
        self.step += 1;
    }

    fn tool(
        &mut self,
        name: &str,
        arguments: &BTreeMap<String, Value>,
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
        self.doing = Doing::Think;
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
    arguments: &BTreeMap<String, Value>,
) -> Doing {
    let text = serde_json::to_string(arguments)
        .unwrap_or_default()
        .to_lowercase();
    let (read, write) = if text.contains("notes.md") {
        (Doing::ReadNotes, Doing::WriteNotes)
    } else if text.contains("skills/") || text.contains("skill.md") {
        (Doing::ReadSkill, Doing::WriteSkill)
    } else {
        (Doing::ReadFile, Doing::WriteFile)
    };
    match name {
        "read" | "search" | "find" => read,
        "edit" | "write" => write,
        "bash" | "powershell" => {
            if text.contains("skill.md") {
                Doing::ReadSkill
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
                Doing::Fetch
            } else if text.contains("notes.md") {
                Doing::ReadNotes
            } else {
                Doing::Run
            }
        }
        "create_agent" => Doing::Think,
        "ask_agent" | "request_road" | "agree_road"
        | "agree_move" | "ask_librarian" | "submit_skill" => {
            Doing::Await
        }
        _ => Doing::Custom,
    }
}
