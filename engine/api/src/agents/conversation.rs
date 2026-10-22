//! The window's focus, the conversation it has open with the sessions
//! shown beside it, an agent's papers, the library's skills and the
//! past sessions.

use serde::{Deserialize, Serialize};

use crate::agents::{Recipient, Session};

/// What the window looks at: the conversation open and the thing
/// selected. Every panel follows the same focus.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Focus {
    /// The chief at this window.
    pub chief: String,
    /// Who the open conversation is with.
    pub with: Recipient,
    pub open: Session,
    pub selected: Selected,
    /// The chief's clock: seconds east of UTC.
    pub offset: i32,
}

/// The thing selected.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Selected {
    Nothing,
    /// An agent by its name, living or gone.
    Agent(String),
    /// A hamlet by its name on disk.
    Hamlet(String),
    Library,
    /// The chief's own place in the realm.
    Hearth,
}

/// A session shown in the chat of the open conversation's agent.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Related {
    pub session: Session,
    pub part: Part,
    /// The address that writes in it, for a session between agents.
    pub from: Option<String>,
    /// It holds what an agent asked another and waited on.
    pub asked: bool,
}

/// The part a session has in an agent's chat.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Part {
    /// A conversation with the agent, shown whole.
    With,
    /// A conversation the agent wrote to another: its own messages,
    /// and the answers to what it asked.
    Wrote,
}

/// What an agent keeps in its house.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Papers {
    pub notes: String,
    pub character: String,
    /// Its own skills.
    pub skills: Vec<Skill>,
    /// The skills another agent of its hamlet has that it could be
    /// given.
    pub gifts: Vec<Gift>,
}

/// A skill an agent could be given, and whose it is.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Gift {
    pub skill: String,
    pub from: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    /// What it is for.
    pub description: String,
}

/// The past sessions found, of so many.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Pasts {
    pub all: u32,
    pub found: Vec<Past>,
}

/// A session kept in a house's log, newest first.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Past {
    /// The name its requests take.
    pub name: String,
    /// The first line said in it.
    pub title: String,
    /// Whoever said it.
    pub by: String,
    /// Milliseconds since 1970.
    pub modified: u64,
    /// The number it was archived under; none for the one in use.
    pub number: Option<String>,
    /// The open conversation can resume it.
    pub ours: bool,
    pub messages: u32,
    /// The tokens it used, in words; empty for none.
    pub tokens: String,
}

/// The change an edit asks for.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    /// The edit makes the file.
    pub created: bool,
    /// Its first lines.
    pub lines: Vec<ChangeLine>,
    /// The lines of the whole change.
    pub total: u32,
    pub added: u32,
    pub removed: u32,
    /// The answer to the edit, when it changed nothing.
    pub failure: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ChangeLine {
    pub mark: Mark,
    pub text: String,
}

/// The part a line has in a change.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Mark {
    Same,
    Out,
    In,
}

impl Change {
    /// What it adds and takes away, in words.
    pub fn count(&self) -> String {
        let lines = |n: u32| match n {
            1 => "1 line".to_string(),
            n => format!("{n} lines"),
        };
        match (self.created, self.added, self.removed) {
            (true, added, _) => format!("New file, {}", lines(added)),
            (false, 0, 0) => "No line changed".to_string(),
            (false, added, 0) => format!("{} added", lines(added)),
            (false, 0, removed) => {
                format!("{} removed", lines(removed))
            }
            (false, added, removed) => {
                format!("{} added, {removed} removed", lines(added))
            }
        }
    }
}
