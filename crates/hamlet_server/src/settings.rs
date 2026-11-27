//! The realm's link to its model server before it is tested, and
//! the provider a model's name gives.

use hamlet_protocol::client::{Link, LinkState};

pub(crate) fn untested() -> Link {
    Link {
        models: None,
        state: LinkState::Untested,
        words: None,
    }
}

/// The provider that serves the model named `model`.
pub(crate) fn provider_of(model: &str) -> Option<&str> {
    model.split_once("::").map(|(provider, _)| provider)
}
