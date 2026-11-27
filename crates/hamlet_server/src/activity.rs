//! What one conversation's agent is doing and saying, folded from
//! its chat as it happens.

use hamlet_protocol::client::Doing;

/// The most bytes of what an agent says that are shown.
const SAID: usize = 600;

/// The words of each thing an agent can be doing.
pub(crate) fn words(doing: &Doing) -> &'static str {
    match doing {
        Doing::Rest => "At rest",
        Doing::Think => "Thinking",
        Doing::ReadFile => "Reading a file",
        Doing::ReadMemory => "Reading its memory",
        Doing::ReadSkill => "Reading a skill",
        Doing::WriteFile => "Writing a file",
        Doing::WriteMemory => "Writing its memory",
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
}

impl Activity {
    /// The activity of a conversation at rest.
    pub fn new() -> Self {
        Self {
            doing: Doing::Rest,
            step: 0,
            failures: 0,
            saying: String::new(),
            thought: false,
            working: false,
            pending: 0,
            faulted: false,
        }
    }

    /// Takes in whether the conversation runs and how many inputs
    /// wait.
    pub fn live(&mut self, running: bool, pending: u64) {
        self.pending = pending;
        if running && !self.working {
            self.rest();
            self.working = true;
            self.faulted = false;
            self.saying.clear();
            self.thought = false;
        }
        if !running && self.working {
            self.working = false;
            self.rest();
        }
    }

    /// A model call of the turn begins.
    pub fn attempted(&mut self) {
        self.think();
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

    /// A call of the tool `name` begins, its arguments written as
    /// `arguments`.
    pub fn called(&mut self, name: &str, arguments: &str) {
        self.doing = doing_of(name, &arguments.to_lowercase());
        self.step += 1;
        self.saying.clear();
        self.thought = true;
    }

    /// The agent writes `text`, as the door folds it.
    pub fn said(&mut self, text: &str, thought: bool) {
        self.think();
        self.speak(text, thought);
    }

    pub fn ended(&mut self, failed: bool) {
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

/// The activity a call of `name` gives its agent, with `text` its
/// arguments written in lower case.
fn doing_of(name: &str, text: &str) -> Doing {
    let (read, write) = if text.contains("memory/") {
        (Doing::ReadMemory, Doing::WriteMemory)
    } else if text.contains("skills/") || text.contains("skill.md") {
        (Doing::ReadSkill, Doing::WriteSkill)
    } else {
        (Doing::ReadFile, Doing::WriteFile)
    };
    match name {
        "read" | "search" | "find" => read,
        "edit" | "write" => write,
        "bash" => {
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
            } else if text.contains("memory/") {
                Doing::ReadMemory
            } else {
                Doing::Run
            }
        }
        "hire" | "create_profile" => Doing::Think,
        "ask" | "request_road" | "agree_road" | "ask_librarian"
        | "submit_skill" => Doing::Await,
        _ => Doing::Custom,
    }
}
