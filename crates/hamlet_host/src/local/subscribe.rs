//! A client's subscription to one event of an actor, in this host or
//! across the link.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hamlet_address::Address;
use tokio::sync::watch;
use tracing::Instrument;

use super::access::{Origin, Stamp};
use super::feed::Closed;
use super::{Failed, Host, HostInner, actor_id, host_shut_down};
use crate::directory::{Place, not_registered};
use crate::link::{LinkState, Port, Routed, Unanswered};
use crate::route::Operation;

impl Host {
    /// Follows the event `event` of the actor at `to` for a client of
    /// the program, stamped [`Caller::Client`](crate::Caller) as
    /// [`Host::call_for_client`] stamps a call. The actor reads the
    /// stamp in the connection's parameters and may refuse.
    ///
    /// Starts the actor from its file if it is not running, and never
    /// makes one. Returns once the actor has let the subscribe
    /// through: the actor's state read after this returns holds every
    /// change before the read, and the subscription every change
    /// after it. The subscription keeps the actor awake.
    ///
    /// Fails with nothing kept and nothing tried again:
    /// - `actor.refused` for a kind the route table does not let a
    ///   client reach;
    /// - `actor.not_registered` for a kind no host holds;
    /// - `actor.not_found` for a key with no actor;
    /// - the actor's own error when it refuses the subscriber;
    /// - `actor.not_ready` when the subscribe is not answered within
    ///   the start deadline, or the host shuts down first;
    /// - as a start fails, for an actor that is being destroyed or
    ///   whose stop missed its deadline.
    ///
    /// For a kind that runs in the host's Node child, it fails at
    /// once with `link.down`, and nothing is sent, while the link is
    /// not ready, and with `link.lost` when the link goes down before
    /// the answer. When the link goes down later, the subscription
    /// subscribes again once the next child is ready: see
    /// [`Item::Gap`].
    pub async fn subscribe_for_client(
        &self,
        to: &Address,
        event: &str,
    ) -> Result<Subscription> {
        self.inner().subscribe(Stamp::client(), to, event).await
    }
}

/// What a [`Subscription`] gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// One broadcast of the event: its arguments, as the actor's
    /// kind encodes them.
    Event(Vec<u8>),
    /// Events were lost here: read the actor's state now. Changes
    /// made between the loss and the read are in both.
    ///
    /// It is given where events were lost for a reader that fell
    /// behind, and, for an actor in the Node child, once each time
    /// the link went down and a new child took the subscription
    /// again. While the link is down the subscription gives nothing:
    /// that `Gap` comes once the new child has answered, so a read
    /// made then succeeds. A `Gap` for a slow reader can be read
    /// while the link is down; a read made then fails, and the
    /// drop's own `Gap` follows.
    Gap,
}

/// One reader's subscription to one event of one actor, from
/// [`Host::subscribe_for_client`]. Dropping it unsubscribes, on any
/// thread. It keeps no host open.
pub struct Subscription {
    source: Source,
    /// The host's `starts_end`.
    shut: watch::Receiver<Option<tokio::time::Instant>>,
    ended: Option<Failed>,
}

impl Subscription {
    /// The next item. Cancel-safe.
    ///
    /// An `Err` is the end, and every later call returns it again:
    /// - `actor.stopping` when the actor begins to stop, and when
    ///   this host shuts down or its last [`Host`] is dropped;
    /// - `actor.destroyed` when the actor begins to be destroyed;
    /// - `actor.disconnected` when the actor closed this
    ///   subscription's connection itself;
    /// - for an actor in the Node child, the error the new child gave
    ///   for the subscribe asked again after the link went down.
    pub async fn next(&mut self) -> Result<Item> {
        if let Some(why) = &self.ended {
            return Err(why.to_error());
        }
        let next = tokio::select! {
            biased;
            _ = self.shut.wait_for(Option::is_some) => {
                Err(Failed::new(&host_shut_down()))
            }
            next = self.source.next() => next,
        };
        next.map_err(|why| {
            let error = why.to_error();
            self.ended = Some(why);
            error
        })
    }
}

/// Where the actor runs.
enum Source {
    Local(Local),
    Linked(Across),
}

impl Source {
    async fn next(&mut self) -> Result<Item, Failed> {
        match self {
            Self::Local(local) => local.next().await,
            Self::Linked(across) => across.next().await,
        }
    }
}

/// A subscription to an actor of this host.
struct Local;

impl Local {
    async fn next(&mut self) -> Result<Item, Failed> {
        todo!("U8a: Local::next")
    }
}

/// A subscription to an actor of the other host, held by itself
/// across link sessions: the port keeps nothing of it while the link
/// is down.
struct Across {
    asked: Arc<Asked>,
    now: Now,
}

/// What was asked, to ask it again.
struct Asked {
    port: Port,
    stamp: Stamp,
    to: Address,
    event: String,
    /// How long each ask may take.
    within: Duration,
}

enum Now {
    Live(Routed),
    /// The session was lost. Kept here, not in `next`'s frame, so a
    /// dropped `next` does not drop an ask that is out.
    Again(Pin<Box<dyn Future<Output = Result<Routed>> + Send>>),
}

impl Across {
    async fn open(asked: Asked) -> Result<Self> {
        let routed =
            asked.ask().await.map_err(Unanswered::into_error)?;
        Ok(Self {
            asked: Arc::new(asked),
            now: Now::Live(routed),
        })
    }

    async fn next(&mut self) -> Result<Item, Failed> {
        loop {
            match &mut self.now {
                Now::Live(routed) => match routed.next().await {
                    Ok(item) => return Ok(item),
                    Err(Closed::Ended(why)) => return Err(why),
                    Err(Closed::Dropped) => {
                        self.now = Now::Again(Box::pin(again(
                            self.asked.clone(),
                        )));
                    }
                },
                Now::Again(asking) => match asking.as_mut().await {
                    Ok(routed) => {
                        self.now = Now::Live(routed);
                        return Ok(Item::Gap);
                    }
                    Err(why) => return Err(Failed::new(&why)),
                },
            }
        }
    }
}

impl Asked {
    async fn ask(&self) -> Result<Routed, Unanswered> {
        self.port
            .subscribe(
                &self.stamp,
                &self.to,
                &self.event,
                self.within,
            )
            .await
    }
}

/// Asks again on each session that gets ready, until the other host
/// answers.
async fn again(asked: Arc<Asked>) -> Result<Routed> {
    let mut link = asked.port.state();
    loop {
        link.wait_for(|state| *state == LinkState::Ready).await?;
        match asked.ask().await {
            Ok(routed) => return Ok(routed),
            // `wait_for` marked that Ready seen, so this waits for
            // the link to move rather than spin on a stale Ready.
            Err(Unanswered::Link(_)) => link.changed().await?,
            Err(Unanswered::Refused(why)) => return Err(why),
        }
    }
}

impl HostInner {
    /// Follows `event` of the actor at `to` for `stamp`, as
    /// [`Host::subscribe_for_client`] says.
    pub(super) async fn subscribe(
        self: &Arc<Self>,
        stamp: Stamp,
        to: &Address,
        event: &str,
    ) -> Result<Subscription> {
        self.admit(
            Operation::Subscribe {
                event: event.to_owned(),
            },
            stamp.caller(),
            to,
        )?;
        let place = self.locate(to)?;
        let span = tracing::debug_span!(
            "subscribe",
            actor_id = actor_id(to).as_str(),
            event,
            caller = stamp.caller().map(tracing::field::display)
        );
        let opened = async {
            match (place, stamp.origin()) {
                (Place::Local(_), _) => self
                    .follow(&stamp, to, event)
                    .await
                    .map(Source::Local),
                (Place::Linked(port), Origin::Here) => {
                    Across::open(Asked {
                        port: port.clone(),
                        stamp,
                        to: to.clone(),
                        event: event.to_owned(),
                        within: self.options.start_deadline,
                    })
                    .await
                    .map(Source::Linked)
                }
                (Place::Linked(_), Origin::Peer) => {
                    Err(not_registered(to.kind()))
                }
            }
        };
        let source = self
            .within_start_deadline(to, opened)
            .instrument(span)
            .await?;
        Ok(Subscription {
            source,
            shut: self.starts_end.subscribe(),
            ended: None,
        })
    }

    async fn follow(
        self: &Arc<Self>,
        _stamp: &Stamp,
        _to: &Address,
        _event: &str,
    ) -> Result<Local> {
        todo!("U8a: follow")
    }
}
