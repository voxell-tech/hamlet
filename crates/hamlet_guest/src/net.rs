//! Fetching over https. It needs the capability `net`, whose ask in
//! the manifest lists the hosts the plugin may reach in its `scope`.
//! A request is an intent to return from `step`, numbered by the
//! plugin; the answer reaches a later `step` as `Incoming::Fetched`,
//! whatever happened.

pub use crate::bindings::exports::hamlet::plugin::plugin::{
    FetchAnswer, FetchResponse,
};
use crate::bindings::exports::hamlet::plugin::plugin::{
    FetchRequest, HttpMethod, Incoming, Intent,
};

/// The intent that gets `url`, numbered `id`.
pub fn get(id: u32, url: &str) -> Intent {
    Intent::Fetch(FetchRequest {
        id,
        method: HttpMethod::Get,
        url: url.into(),
        content_type: String::new(),
        body: Vec::new(),
    })
}

/// The intent that posts `body`, of the type `content_type`, to
/// `url`, numbered `id`.
pub fn post(
    id: u32,
    url: &str,
    content_type: &str,
    body: &[u8],
) -> Intent {
    Intent::Fetch(FetchRequest {
        id,
        method: HttpMethod::Post,
        url: url.into(),
        content_type: content_type.into(),
        body: body.to_vec(),
    })
}

/// The number and the outcome of the fetch that `incoming` answers.
/// `None` for anything else.
pub fn answer(
    incoming: &Incoming,
) -> Option<(u32, Result<&FetchResponse, &str>)> {
    let Incoming::Fetched(answer) = incoming else {
        return None;
    };
    Some((answer.id, answer.outcome.as_ref().map_err(String::as_str)))
}
