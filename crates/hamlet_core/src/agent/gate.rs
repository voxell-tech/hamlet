//! The gate a tool call passes before it runs (C7), and whether an
//! instruction may still be argued against (H10).

use futures::future::BoxFuture;

use crate::tools::{Call, Leave, Reach};

/// A tool call about to run, as the gate sees it.
pub struct Request<'a> {
    /// The agent making it.
    pub who: &'a str,
    pub call: &'a Call,
    pub reach: Reach,
}

/// The gate's ruling on a tool call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ruling {
    Go(Leave),
    /// Blocked, with the reason the agent is told.
    Deny(String),
    /// Answered by a chief; the text is the call's result.
    Say(String),
}

/// The hook asked before every tool call, which may take as long as
/// it likes to answer (C7).
pub type Gate =
    Box<dyn FnMut(Request<'_>) -> BoxFuture<'static, Ruling> + Send>;

/// The standing of this turn's instruction: open to argument, or
/// closed (H10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Objection {
    /// The turn is not a chief's instruction.
    #[default]
    Closed,
    Open,
    Spent,
}
