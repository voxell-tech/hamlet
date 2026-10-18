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

/// The payload of `event` as text, as the events of the host have it.
pub fn text(event: &Published) -> String {
    String::from_utf8_lossy(&event.payload).into_owned()
}
