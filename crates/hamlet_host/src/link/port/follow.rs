//! One session's half of a subscription.

use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use hamlet_actor_core::ActorLifecycle;
use tokio::sync::oneshot;

use super::{Answered, Forget, Pending, Session};
use crate::link::{Ask, Frame, Vetted, WireError};
use crate::local::feed::{Closed, Feed, Sink, feed};
use crate::{Address, Caller, Item};

/// One session's subscription, as the asking host holds it.
pub(crate) struct Routed {
    feed: Feed,
    _route: Route,
}

impl Routed {
    /// The feed's next item. [`Closed::Dropped`] is the session's
    /// loss, [`Closed::Ended`] the other host's end.
    pub(crate) async fn next(&mut self) -> Result<Item, Closed> {
        self.feed.next().await
    }
}

/// The reason a subscribe across the link gave no subscription.
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

/// The asking host's claim on one subscription of one session, from
/// before its request is queued. Dropped, it writes `Unsubscribe` if
/// the session still routes the subscription.
struct Route {
    session: Arc<Session>,
    sub: u64,
}

impl Route {
    /// Stops routing the subscription, and sends no `Unsubscribe`.
    fn forget(self) {
        self.session.pending.lock().following.remove(&self.sub);
    }
}

impl Drop for Route {
    fn drop(&mut self) {
        let routed =
            self.session.pending.lock().following.remove(&self.sub);
        if routed.is_none() {
            return;
        }
        let out = self.session.out.clone();
        let unsubscribe = Frame::Unsubscribe { sub: self.sub };
        // A drop cannot wait for the writer's queue, and may run
        // where no runtime is.
        self.session.host.runtime().spawn(async move {
            let _ = out.write(&unsubscribe).await;
        });
    }
}

impl Pending {
    /// The waiter for the reply to the subscribe `id`, registered at
    /// once with `sink`, where the subscription's frames go. Fails
    /// with `link.down` on a lost session.
    fn follow(
        &mut self,
        id: u64,
        sink: Sink,
    ) -> anyhow::Result<Answered> {
        let answered = self.expect(id)?;
        self.following.insert(id, sink);
        Ok(answered)
    }
}

impl Session {
    /// The asking side.
    pub(super) async fn subscribe(
        self: &Arc<Self>,
        caller: Option<&Caller>,
        to: &Address,
        event: &str,
        within: Duration,
    ) -> Result<Routed, Unanswered> {
        let subscribe = Ask::Subscribe {
            event: event.to_owned(),
        };
        let (id, bytes) = self
            .request(caller, to, subscribe)
            .map_err(|error| Unanswered::Refused(error.into()))?;
        let (sink, feed) = feed();
        let answered = self
            .pending
            .lock()
            .follow(id, sink)
            .map_err(Unanswered::Link)?;
        let route = Route {
            session: self.clone(),
            sub: id,
        };
        let _forget = Forget {
            pending: &self.pending,
            id,
        };
        let answer = tokio::time::timeout(
            within,
            self.exchange(bytes, answered),
        )
        .await;
        let unanswered = match answer {
            Ok(Ok(Ok(_))) => {
                return Ok(Routed {
                    feed,
                    _route: route,
                });
            }
            Err(_) => {
                return Err(Unanswered::Refused(
                    ActorLifecycle::NotReady.build().context(
                        format!(
                            "the subscribe to {to} was not answered \
                             within {within:?}"
                        ),
                    ),
                ));
            }
            Ok(Ok(Err(refused))) => {
                Unanswered::Refused(refused.into_error())
            }
            Ok(Err(link)) => Unanswered::Link(link),
        };
        route.forget();
        Err(unanswered)
    }

    /// `Frame::Event`. Never waits. An event for a subscription this
    /// session does not route is dropped.
    pub(super) fn deliver(&self, sub: u64, args: Vec<u8>) {
        if let Some(sink) = self.pending.lock().following.get(&sub) {
            sink.push(args);
        }
    }

    /// `Frame::Gap`. Never waits.
    pub(super) fn gap(&self, sub: u64) {
        if let Some(sink) = self.pending.lock().following.get(&sub) {
            sink.gap();
        }
    }

    /// `Frame::SubscriptionEnded`. Never waits.
    pub(super) fn end(&self, sub: u64, error: WireError) {
        let sink = self.pending.lock().following.remove(&sub);
        if let Some(sink) = sink {
            sink.close(&error.into_error());
        }
    }

    /// The answering side: serves the other host's subscribe `sub`
    /// until it ends or the other host unsubscribes.
    pub(super) fn serve(
        self: &Arc<Self>,
        sub: u64,
        from: Vetted,
        to: Address,
        event: String,
    ) {
        let (cancel, cancelled) = oneshot::channel::<()>();
        self.pending.lock().serving.insert(sub, cancel);
        let session = self.clone();
        self.host.runtime().spawn(async move {
            tokio::select! {
                biased;
                _ = cancelled => {}
                () = session.pump(sub, from, &to, &event) => {
                    session.pending.lock().serving.remove(&sub);
                }
            }
        });
    }

    /// Subscribes in this host, answers, then writes what the
    /// subscription gives until it ends.
    async fn pump(
        &self,
        sub: u64,
        from: Vetted,
        to: &Address,
        event: &str,
    ) {
        let mut followed =
            match self.host.subscribe(from, to, event).await {
                Ok(followed) => followed,
                Err(error) => {
                    return self.reply(sub, Err(error)).await;
                }
            };
        self.reply(sub, Ok(Vec::new())).await;
        let mut told = false;
        loop {
            let frame = match followed.next().await {
                Ok(Item::Event(args)) => Frame::Event { sub, args },
                Ok(Item::Gap) => Frame::Gap { sub },
                Err(end) => {
                    let error = WireError::of(&end);
                    let ended =
                        Frame::SubscriptionEnded { sub, error };
                    let _ = self.out.write(&ended).await;
                    return;
                }
            };
            match self.out.write(&frame).await {
                Ok(()) => {}
                Err(error)
                    if error.kind() == ErrorKind::InvalidInput =>
                {
                    if !std::mem::replace(&mut told, true) {
                        tracing::warn!(
                            actor_id = %to,
                            event,
                            %error,
                            "an event over the frame limit crosses the \
                             link as a gap"
                        );
                    }
                    if self
                        .out
                        .write(&Frame::Gap { sub })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }

    /// `Frame::Unsubscribe`. Never waits. An unknown `sub` is
    /// dropped: the subscription ended by itself, or was refused.
    pub(super) fn unsubscribe(&self, sub: u64) {
        drop(self.pending.lock().serving.remove(&sub));
    }
}
