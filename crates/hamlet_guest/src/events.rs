//! Publishing events, and reading the ones a plugin is given. A
//! payload is bytes, written here with `postcard`; the events of the
//! host carry text, read with [`text`].

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::bindings::exports::hamlet::plugin::plugin::{
    Intent, Publication, Published,
};

/// The intent that publishes the event `topic`, which the manifest
/// lists under `exports.events`, with `payload`. It is to be returned
/// from `step`.
pub fn publish<P: Serialize>(
    topic: &str,
    payload: &P,
) -> Result<Intent, String> {
    let payload =
        postcard::to_allocvec(payload).map_err(|e| e.to_string())?;
    Ok(Intent::Publish(Publication {
        topic: topic.into(),
        payload,
    }))
}

/// The payload of `event` as a `P`.
pub fn payload<P: DeserializeOwned>(
    event: &Published,
) -> Result<P, String> {
    postcard::from_bytes(&event.payload).map_err(|e| e.to_string())
}

/// A turn of an agent, as `host/before-turn` and `host/after-turn`
/// tell of it. They need the capability `agent.hook`, and only tell:
/// a plugin cannot change or stop the turn.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Turn {
    pub agent: String,
    pub hamlet: String,
    /// How many turns the agent's session has had, this one
    /// included.
    pub number: u64,
    /// The names of the tools the turn used, in order. None before
    /// it ends. Never what they were given or returned.
    pub tools: Vec<String>,
}

/// The turn that a `host/before-turn` or `host/after-turn` event
/// tells of.
pub fn turn(event: &Published) -> Option<Turn> {
    if event.sender != "host"
        || !matches!(
            event.topic.as_str(),
            "before-turn" | "after-turn"
        )
    {
        return None;
    }
    let text = text(event);
    let field = |key: &str| {
        text.lines().find_map(|line| {
            line.strip_prefix(key)?.strip_prefix('=')
        })
    };
    Some(Turn {
        agent: field("agent")?.into(),
        hamlet: field("hamlet")?.into(),
        number: field("turn")?.parse().ok()?,
        tools: field("tools")?
            .split(',')
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect(),
    })
}

/// The payload of `event` as text, as the events of the host have it.
pub fn text(event: &Published) -> String {
    String::from_utf8_lossy(&event.payload).into_owned()
}
