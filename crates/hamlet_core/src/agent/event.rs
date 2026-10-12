//! The events of an agent's turn.

use hamlet_model::provider::CallRecord;
use hamlet_rules::civic::Civic;

use crate::tools::Change;

/// The most of a tool call's arguments an event or a transcript
/// shows, in characters.
pub(crate) const SUMMARY_CHARS: usize = 200;

/// The listener a turn tells its events.
pub type Listener<'a> = dyn FnMut(Event) + Send + 'a;

/// An event of an agent's turn.
#[derive(Clone, Debug)]
pub enum Event {
    Text(String),
    Reasoning(String),
    Tool {
        name: String,
        summary: String,
        /// The change it asks for, when it is an edit.
        change: Option<Change>,
    },
    ToolDone {
        name: String,
        exit_code: i32,
        duration_ms: u64,
        /// The first line of its answer, when it failed.
        failure: Option<String>,
    },
    /// Words for whoever reads the conversation, which no model is
    /// shown.
    Notice(String),
    /// A model call finished; carries what it used.
    ModelCall(CallRecord),
    /// The mayor added an agent to the hamlet.
    AgentCreated {
        name: String,
        purpose: String,
    },
    /// The agent `by` called on the librarian.
    Consulted {
        by: String,
    },
    /// Something the agent `name` did while another waited on it.
    Agent {
        name: String,
        event: Box<Event>,
    },
    /// A message went along the road from one agent to another.
    Message {
        from: String,
        to: String,
    },
    /// The mayor laid a road between two agents.
    RoadBuilt {
        a: String,
        b: String,
    },
    /// The mayor removed an agent from the hamlet.
    AgentRemoved {
        name: String,
    },
    /// The mayor merged the agent `gone` into `kept`.
    AgentsMerged {
        kept: String,
        gone: String,
    },
    /// A message for another agent that this one does not wait on,
    /// sent as part of `task`.
    Tell {
        to: String,
        text: String,
        task: String,
    },
    /// A vote, a change of mayor, or something between hamlets.
    Civic(Civic),
    /// The agent at `from` asked this one something at `at`, in
    /// milliseconds since 1970, and waits for the answer.
    Question {
        from: String,
        text: String,
        at: u64,
    },
}
