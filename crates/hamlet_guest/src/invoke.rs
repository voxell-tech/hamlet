//! Calling the functions of typed interfaces other plugins export,
//! and answering the calls of the ones this plugin exports. What
//! crosses is bytes written here with `postcard`: the arguments as
//! the tuple of the parameters, such as `&(port, feet)`, and the
//! result as its value. The host checks both against the types the
//! manifests declare.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::bindings::exports::hamlet::plugin::plugin::{
    CallAnswer, Incoming, Intent, InvokeRequest,
};
pub use crate::bindings::exports::hamlet::plugin::plugin::{
    Invocation, InvokeError,
};

/// What a call came to: the bytes of its result, or why there are
/// none.
pub type Outcome = Result<Vec<u8>, InvokeError>;

/// The calls a plugin has waiting, each with a tag of the plugin's
/// own type that says what to do with the answer. It is kept in the
/// plugin's [`State`](crate::State), so that an answer that comes in
/// a later `step` finds its tag.
#[derive(Serialize, Deserialize)]
pub struct Invokes<T> {
    next: u64,
    waiting: Vec<(u64, T)>,
}

impl<T> Default for Invokes<T> {
    fn default() -> Self {
        Self {
            next: 0,
            waiting: Vec::new(),
        }
    }
}

impl<T> Invokes<T> {
    /// Calls `function` of `interface` in the plugin `plugin` with
    /// `args`. The intent is to be returned from `step`; the answer
    /// reaches a later `step` and [`Invokes::route`] hands it back
    /// with `tag`.
    pub fn call<A: Serialize>(
        &mut self,
        plugin: &str,
        (interface, function): (&str, &str),
        args: &A,
        tag: T,
    ) -> Result<Intent, String> {
        let args =
            postcard::to_allocvec(args).map_err(|e| e.to_string())?;
        self.next += 1;
        self.waiting.push((self.next, tag));
        Ok(Intent::Invoke(InvokeRequest {
            call_id: self.next,
            plugin: plugin.into(),
            function: format!("{interface}/{function}"),
            args,
        }))
    }

    /// The tag and the outcome of the call that `incoming` answers,
    /// and the call is no longer waiting. `None` for anything else.
    pub fn route(
        &mut self,
        incoming: &Incoming,
    ) -> Option<(T, Outcome)> {
        let Incoming::Returned(answer) = incoming else {
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

/// The plugins that implement `interface`, which the manifest lists
/// under `imports.interfaces`, in the order of their ids.
pub fn implementers(interface: &str) -> Result<Vec<String>, String> {
    crate::bindings::hamlet::plugin::host::implementers(interface)
}

/// The result of a call as a `R`, or why there is none in words.
pub fn returned<R: DeserializeOwned>(
    outcome: &Outcome,
) -> Result<R, String> {
    let bytes = outcome.as_ref().map_err(why)?;
    postcard::from_bytes(bytes).map_err(|e| e.to_string())
}

/// Why a call has no result, in words such as `refused: plugin.use
/// not granted`.
pub fn why(error: &InvokeError) -> String {
    let (kind, text) = match error {
        InvokeError::NoImplementer(text) => ("no implementer", text),
        InvokeError::Refused(text) => ("refused", text),
        InvokeError::Mismatch(text) => ("mismatch", text),
        InvokeError::OutOfBudget(text) => ("out of budget", text),
        InvokeError::Failed(text) => ("failed", text),
        InvokeError::Error(text) => ("error", text),
    };
    format!("{kind}: {text}")
}

/// The call of a function this plugin exports that `incoming` is, if
/// it is one.
pub fn invoked(incoming: &Incoming) -> Option<&Invocation> {
    match incoming {
        Incoming::Invoked(call) => Some(call),
        _ => None,
    }
}

/// The arguments of a call, as the tuple `A` of the parameters.
pub fn args<A: DeserializeOwned>(
    call: &Invocation,
) -> Result<A, String> {
    postcard::from_bytes(&call.args).map_err(|e| e.to_string())
}

/// The answer to `call`: its result, or why there is none. It is to
/// be returned from `step`.
pub fn give_back<R: Serialize>(
    call: &Invocation,
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
