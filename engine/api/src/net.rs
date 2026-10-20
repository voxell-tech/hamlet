//! Fetches over the network, made by the engine for a plugin.

use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Method {
    Get,
    Post,
}

/// A request to an `https` address on a host the plugin asked for.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct FetchRequest {
    pub method: Method,
    pub url: String,
    /// The type of `body`, for a post.
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// What the host answered, whatever its status. A redirect is not
/// followed: it comes back as it is.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct FetchReply {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}
