//! An agent's work and words as its turn goes, guessed from the
//! turn's events.

use bevy_ecs::prelude::*;

use crate::agent::Event as AgentEvent;

/// The paper an agent reads or writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paper {
    File,
    Notes,
    Skill,
}

/// The step of its loop an agent is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Doing {
    /// Busy, with nothing more known.
    #[default]
    Unknown,
    /// Waiting for the model's reply.
    Think,
    Read(Paper),
    Write(Paper),
    /// Running a shell command.
    Run,
    /// Running a command that reaches the network.
    Fetch,
    /// Using a tool added by hand.
    CustomTool,
    /// Waiting for the answer of an agent it asked.
    Await,
}

/// An agent's words or thoughts as its turn goes.
#[derive(Component, Clone, PartialEq, Eq, Debug, Default)]
pub struct Saying {
    pub text: String,
    /// The text is a thought.
    pub thought: bool,
}

impl Saying {
    /// The most of it that is kept, in bytes: the end of what is
    /// said, which is what shows, and the start of a thought, which
    /// says what it is about.
    const KEPT: usize = 600;

    /// Takes in one event of the agent's turn.
    pub fn note(&mut self, event: &AgentEvent) {
        let (more, thought) = match event {
            AgentEvent::Text(more) => (more, false),
            AgentEvent::Reasoning(more) => (more, true),
            // A tool call ends what was being said.
            AgentEvent::Tool { .. } => {
                self.text.clear();
                self.thought = true;
                return;
            }
            _ => return,
        };
        if self.thought != thought {
            self.text.clear();
            self.thought = thought;
        }
        if thought && self.text.len() >= Self::KEPT {
            return;
        }
        self.text.push_str(more);
        if !thought && self.text.len() > Self::KEPT {
            let mut cut = self.text.len() - Self::KEPT;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
        }
    }
}

/// An agent's step in its work.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Activity {
    pub doing: Doing,
    /// Model calls and tool calls begun since it took up its work.
    pub step: u32,
    /// Tool calls that ended in an error since then.
    pub failures: u32,
}

impl Activity {
    /// Takes in one event of the agent's turn.
    pub fn note(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::Text(_) | AgentEvent::Reasoning(_) => {
                if self.doing != Doing::Think {
                    self.doing = Doing::Think;
                    self.step += 1;
                }
            }
            AgentEvent::Tool { name, summary, .. } => {
                self.doing = doing(name, summary);
                self.step += 1;
            }
            AgentEvent::ToolDone { exit_code, .. } => {
                if *exit_code != 0 {
                    self.failures += 1;
                }
                self.doing = Doing::Think;
                self.step += 1;
            }
            _ => {}
        }
    }
}

/// The step a tool call amounts to, judged from its name and the
/// start of its arguments.
fn doing(tool: &str, arguments: &str) -> Doing {
    let args = arguments.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| args.contains(w));
    match tool {
        "bash" | "powershell" => {
            // Reading a skill comes first: a skill's own name can
            // be one of the network words.
            if has(&["skill.md"]) {
                Doing::Read(Paper::Skill)
            } else if has(&[
                "curl ",
                "wget ",
                "http://",
                "https://",
                "web-search",
                "read-webpage",
                "weather",
                "call-api",
            ]) {
                Doing::Fetch
            } else if has(&["notes.md"]) {
                Doing::Read(Paper::Notes)
            } else {
                Doing::Run
            }
        }
        "edit" => {
            if has(&["notes.md"]) {
                Doing::Write(Paper::Notes)
            } else if has(&["skills/"]) {
                Doing::Write(Paper::Skill)
            } else {
                Doing::Write(Paper::File)
            }
        }
        "create_agent" => Doing::Think,
        "ask_agent" => Doing::Await,
        _ => Doing::CustomTool,
    }
}
