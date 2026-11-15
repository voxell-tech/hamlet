//! One session's half of a subscription.

use std::sync::Arc;
use std::time::Duration;

use super::Session;
use crate::local::feed::{Closed, Feed};
use crate::{Address, Caller, Item};

/// One session's subscription, as the asking host holds it.
pub(crate) struct Routed {
    feed: Feed,
}

impl Routed {
    /// The feed's next item. [`Closed::Dropped`] is the session's
    /// loss, [`Closed::Ended`] the other host's end.
    pub(crate) async fn next(&mut self) -> Result<Item, Closed> {
        self.feed.next().await
    }
}

/// Why a subscribe across the link gave no subscription.
pub(crate) enum Unanswered {
    /// `link.down` or `link.lost`: nothing of the subscribe is left
    /// in the other host, and the next session may be asked.
    Link(anyhow::Error),
    /// The other host's answer, or none in time.
    Refused(anyhow::Error),
}

impl Unanswered {
    pub(crate) fn into_error(self) -> anyhow::Error {
        match self {
            Self::Link(error) | Self::Refused(error) => error,
        }
    }
}

impl Session {
    pub(super) async fn subscribe(
        self: &Arc<Self>,
        _caller: Option<&Caller>,
        _to: &Address,
        _event: &str,
        _within: Duration,
    ) -> Result<Routed, Unanswered> {
        todo!("U8b: Session::subscribe")
    }
}
