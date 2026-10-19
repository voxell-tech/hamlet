//! Steering the agents' turns. A plugin that holds the capability
//! `agent.steer` is given `Incoming::Steer` at each model call and
//! each tool call of the agents of the hamlets its ask lists, and
//! answers in the same `step`, which has a small budget of its own:
//! with [`note`] before a model call, with [`hold`] before a tool
//! call, or with nothing to change nothing.

use crate::bindings::exports::hamlet::plugin::plugin::{
    Incoming, Intent, SteerChange, SteerReply,
};
pub use crate::bindings::exports::hamlet::plugin::plugin::{
    SteerPoint, SteerTool,
};

/// The point of a turn that `incoming` puts to the plugin. `None` for
/// anything else.
pub fn point(incoming: &Incoming) -> Option<&SteerPoint> {
    match incoming {
        Incoming::Steer(point) => Some(point),
        _ => None,
    }
}

/// The intent that gives the agent a short note before the model
/// call of `point`, marked as the plugin's.
pub fn note(point: &SteerPoint, text: &str) -> Intent {
    answer(point, SteerChange::Note(text.into()))
}

/// The intent that holds back the tool call of `point`, with a reason
/// the agent is told.
pub fn hold(point: &SteerPoint, why: &str) -> Intent {
    answer(point, SteerChange::Hold(why.into()))
}

fn answer(point: &SteerPoint, change: SteerChange) -> Intent {
    Intent::SteerAnswer(SteerReply {
        id: point.id,
        change,
    })
}
