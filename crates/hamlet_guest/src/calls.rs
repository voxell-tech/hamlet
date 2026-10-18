//! Running the commands of other plugins, and answering the commands
//! other plugins run. What crosses is bytes, written here with
//! `postcard`, so both plugins use the same type for it.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::bindings::exports::hamlet::plugin::plugin::{
    CallAnswer, CallRequest, CommandCall, Incoming, Intent,
};

/// What a call came to: the bytes the command returned, or why there
/// are none.
pub type Outcome = Result<Vec<u8>, String>;

/// A call that was started and has no answer yet.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub struct Pending(pub u64);

/// The calls a plugin has waiting, each with a tag of the plugin's
/// own type that says what to do with the answer. It is kept in the
/// plugin's [`State`](crate::State), so that an answer that comes in
/// a later `step` finds its tag.
#[derive(Serialize, Deserialize)]
pub struct Calls<T> {
    next: u64,
    waiting: Vec<(u64, T)>,
}

impl<T> Default for Calls<T> {
    fn default() -> Self {
        Self {
            next: 0,
            waiting: Vec::new(),
        }
    }
}

impl<T> Calls<T> {
    /// Runs `command` of the plugin `target` with `args`. The intent
    /// is to be returned from `step`; the answer reaches a later
    /// `step` and [`Calls::route`] hands it back with `tag`.
    pub fn call<A: Serialize>(
        &mut self,
        target: &str,
        command: &str,
        args: &A,
        tag: T,
    ) -> Result<(Pending, Intent), String> {
        let args =
            postcard::to_allocvec(args).map_err(|e| e.to_string())?;
        self.next += 1;
        let id = self.next;
        self.waiting.push((id, tag));
        Ok((
            Pending(id),
            Intent::RunCommand(CallRequest {
                call_id: id,
                plugin: target.into(),
                command: command.into(),
                args,
            }),
        ))
    }

    /// The tag and the outcome of the call that `incoming` answers,
    /// and the call is no longer waiting. `None` for anything else.
    pub fn route(
        &mut self,
        incoming: &Incoming,
    ) -> Option<(T, Outcome)> {
        let Incoming::Reply(answer) = incoming else {
            return None;
        };
        let at = self
            .waiting
            .iter()
            .position(|(id, _)| *id == answer.call_id)?;
        let (_, tag) = self.waiting.remove(at);
        Some((tag, answer.outcome.clone()))
    }

    /// The calls waiting.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

/// The arguments of a command that was run, as a `A`.
pub fn args<A: DeserializeOwned>(
    call: &CommandCall,
) -> Result<A, String> {
    postcard::from_bytes(&call.args).map_err(|e| e.to_string())
}

/// The outcome of a call as a `R`.
pub fn returned<R: DeserializeOwned>(
    outcome: &Outcome,
) -> Result<R, String> {
    let bytes = outcome.as_ref().map_err(String::clone)?;
    postcard::from_bytes(bytes).map_err(|e| e.to_string())
}

/// The answer to the command `call`: what it returns, or why it
/// cannot. It is to be returned from `step`.
pub fn reply<R: Serialize>(
    call: &CommandCall,
    result: Result<&R, String>,
) -> Intent {
    let outcome = result.and_then(|value| {
        postcard::to_allocvec(value).map_err(|e| e.to_string())
    });
    Intent::Reply(CallAnswer {
        call_id: call.call_id,
        outcome,
    })
}
