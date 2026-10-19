//! Asking the model the chief runs. It needs the capability
//! `model.call`; each call is charged to the first hamlet's budget
//! under the plugin's name, and the host limits how many a minute and
//! how many tokens. A call is an intent to return from `step`,
//! numbered by the plugin; the answer reaches a later `step` as
//! `Incoming::ModelAnswered`, which [`answer`] reads.

use crate::bindings::exports::hamlet::plugin::plugin::{
    Incoming, Intent, ModelMessage, ModelRequest, ModelRole,
};

/// The intent that asks the model `prompt` with the system text
/// `system`, numbered `id`, for a reply of at most `max_tokens`
/// tokens, or as many as the host allows.
pub fn ask(
    id: u32,
    system: &str,
    prompt: &str,
    max_tokens: Option<u32>,
) -> Intent {
    talk(id, system, &[(true, prompt)], max_tokens)
}

/// The intent that asks the model to go on from `messages`, each
/// `true` where the plugin says it and `false` where the model said
/// it.
pub fn talk(
    id: u32,
    system: &str,
    messages: &[(bool, &str)],
    max_tokens: Option<u32>,
) -> Intent {
    Intent::ModelCall(ModelRequest {
        id,
        system: system.into(),
        messages: messages
            .iter()
            .map(|(user, text)| ModelMessage {
                role: match user {
                    true => ModelRole::User,
                    false => ModelRole::Assistant,
                },
                text: (*text).into(),
            })
            .collect(),
        max_tokens,
    })
}

/// The number and the outcome of the call that `incoming` answers:
/// the text of the reply, or why there is none. `None` for anything
/// else.
pub fn answer(
    incoming: &Incoming,
) -> Option<(u32, Result<&str, &str>)> {
    let Incoming::ModelAnswered(answer) = incoming else {
        return None;
    };
    let outcome = answer
        .outcome
        .as_ref()
        .map(String::as_str)
        .map_err(String::as_str);
    Some((answer.id, outcome))
}
