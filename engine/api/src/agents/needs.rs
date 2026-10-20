//! What waits for the chief: approvals, objections, questions and
//! faults, the answers each takes, and the questions an agent asks.

use serde::{Deserialize, Serialize};

use super::{Change, SessionName};

/// Something that waits for the chief, oldest first.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Need {
    /// The id its answers take.
    pub id: u64,
    pub kind: NeedKind,
    /// The agent that raised it.
    pub by: String,
    /// The exact text the chief has to read.
    pub about: String,
    /// One line for it.
    pub line: String,
    /// The time it has waited, in words.
    pub waited: String,
    /// Times it was asked while it waited.
    pub asked: u32,
    /// The answers it takes, in order.
    pub answers: Vec<Choice>,
    pub questions: Option<Questions>,
    /// The change an edit that waits asks for.
    pub change: Option<Change>,
    /// The session whose turn it holds.
    pub session: Option<SessionName>,
    /// The answers picked so far for its questions.
    pub sheet: Sheet,
}

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum NeedKind {
    Approval(Danger),
    Objection,
    Question,
    Fault,
    /// It waited when the agents last stopped and was never
    /// answered.
    Lapsed,
}

/// What makes a tool call wait.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Danger {
    /// It needs the network and its agent has none.
    Network,
    /// Nothing confines it and its agent may not run unconfined.
    Unconfined,
    /// Nothing confines it and it looks destructive.
    Destructive,
    /// The chief reserved it.
    Reserved,
    /// An edit, in a hamlet that asks before each.
    Edit,
    /// A command, in a hamlet that asks before each.
    Command,
}

impl Danger {
    /// The chief can allow the like for the rest of the session.
    pub fn for_session(self) -> bool {
        matches!(self, Danger::Edit | Danger::Command)
    }
}

/// An answer to something that waits.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Choice {
    /// Run it this once.
    Allow,
    /// Run it, and let the agent do the like without asking.
    Always,
    Deny,
    GoAhead,
    /// Take the instruction back.
    Withdraw,
    /// Answer in words.
    Reply,
    Dismiss,
}

/// The questions one call asks.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Questions {
    /// A line over the questions; empty for none.
    pub message: String,
    pub questions: Vec<Question>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    /// Empty for none.
    pub description: String,
    pub options: Vec<Offer>,
    /// The chief may answer in words of their own.
    pub text_input: bool,
}

/// An answer a question offers.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Offer {
    pub title: String,
    /// Empty for none.
    pub description: String,
}

/// The chief's answer to one question.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Pick {
    /// The option at this place.
    Offer(u32),
    Words(String),
}

/// The question on show and the answers picked so far.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Sheet {
    pub at: u32,
    /// One for each question.
    pub picks: Vec<Option<Pick>>,
}
