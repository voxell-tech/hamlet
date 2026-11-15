//! The queue between a source of events and its one reader, at every
//! hop of a subscription: a local reader's, and each host's across
//! the link. A full queue drops the event and never makes its source
//! wait, and the reader gets one [`Item::Gap`] where the loss was.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::Notify;

use super::Failed;
use super::subscribe::Item;

/// The most a queue holds, each event counted as its arguments plus
/// [`OVERHEAD`]. An empty queue takes one event of any size.
const BUDGET: usize = 1 << 20;

/// The weight of an event with no arguments, or of the mark of a
/// loss, so the budget bounds the number of events too.
const OVERHEAD: usize = 64;

/// A queue for one reader: its writing end and its reading end.
pub(crate) fn feed() -> (Sink, Feed) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            queue: VecDeque::new(),
            weight: 0,
            lagged: false,
            closed: None,
        }),
        wake: Notify::new(),
    });
    (Sink(shared.clone()), Feed(shared))
}

/// The writing end. Its methods return at once. Dropped without
/// [`Sink::close`], it leaves the reader [`Closed::Dropped`].
pub(crate) struct Sink(Arc<Shared>);

/// The reading end.
pub(crate) struct Feed(Arc<Shared>);

struct Shared {
    /// Never held across an await.
    state: Mutex<State>,
    wake: Notify,
}

struct State {
    queue: VecDeque<Item>,
    /// The weight of `queue`, against [`BUDGET`].
    weight: usize,
    /// An event was dropped after the last item queued, and no gap
    /// is queued for it yet.
    lagged: bool,
    closed: Option<Closed>,
}

impl State {
    /// Queues `item` if it fits.
    fn take(&mut self, item: Item) -> bool {
        let cost = cost(&item);
        if self.weight != 0 && self.weight + cost > BUDGET {
            return false;
        }
        self.weight += cost;
        self.queue.push_back(item);
        true
    }
}

fn cost(item: &Item) -> usize {
    match item {
        Item::Event(args) => args.len() + OVERHEAD,
        Item::Gap => OVERHEAD,
    }
}

/// The reason a feed gives no more.
#[derive(Clone)]
pub(crate) enum Closed {
    /// The source closed it, and said why.
    Ended(Failed),
    /// The source dropped its sink: a link session that was lost.
    Dropped,
}

impl Sink {
    /// Queues one event, or drops it and marks the loss when the
    /// queue is full.
    pub(crate) fn push(&self, args: Vec<u8>) {
        self.put(Item::Event(args));
    }

    /// Marks that events were lost before they reached this queue.
    pub(crate) fn gap(&self) {
        self.put(Item::Gap);
    }

    fn put(&self, item: Item) {
        {
            let mut state = self.0.state.lock();
            if state.lagged {
                // The earlier loss goes in first, where it was. With
                // no room, this item is lost too.
                if !state.take(Item::Gap) {
                    return;
                }
                state.lagged = false;
                if item == Item::Gap {
                    return self.0.wake.notify_one();
                }
            }
            if !state.take(item) {
                // A gap at the tail already stands for this loss.
                // Otherwise the queue holds an item, so the reader
                // comes and finds the mark behind it.
                if state.queue.back() != Some(&Item::Gap) {
                    state.lagged = true;
                }
                return;
            }
        }
        self.0.wake.notify_one();
    }

    /// Closes the feed with `why`, unless it is closed already. The
    /// reader gets what is queued, then `why`.
    pub(crate) fn close(&self, why: &anyhow::Error) {
        self.0
            .state
            .lock()
            .closed
            .get_or_insert_with(|| Closed::Ended(Failed::new(why)));
        self.0.wake.notify_one();
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.0.state.lock().closed.get_or_insert(Closed::Dropped);
        self.0.wake.notify_one();
    }
}

impl Feed {
    /// The next item, or why there is none, which it then gives at
    /// every call. Cancel-safe.
    pub(crate) async fn next(&mut self) -> Result<Item, Closed> {
        loop {
            {
                let mut state = self.0.state.lock();
                if let Some(item) = state.queue.pop_front() {
                    state.weight -= cost(&item);
                    return Ok(item);
                }
                // A loss with no event after it to carry its mark.
                if std::mem::take(&mut state.lagged) {
                    return Ok(Item::Gap);
                }
                if let Some(closed) = &state.closed {
                    return Err(closed.clone());
                }
            }
            // `notify_one` keeps a permit, so a wake between the
            // unlock and this await is not lost.
            self.0.wake.notified().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use anyhow::anyhow;

    use super::*;

    const SOON: Duration = Duration::from_millis(200);

    fn event(n: u64) -> Vec<u8> {
        n.to_be_bytes().to_vec()
    }

    fn value(item: &Item) -> Option<u64> {
        match item {
            Item::Event(args) => Some(u64::from_be_bytes(
                args[..8].try_into().unwrap(),
            )),
            Item::Gap => None,
        }
    }

    fn why(next: Result<Item, Closed>) -> String {
        match next {
            Err(Closed::Ended(failed)) => {
                failed.to_error().to_string()
            }
            Err(Closed::Dropped) => "dropped".to_owned(),
            Ok(item) => panic!("expected the end, got {item:?}"),
        }
    }

    fn weight(feed: &Feed) -> usize {
        feed.0.state.lock().weight
    }

    async fn pending(feed: &mut Feed) -> bool {
        tokio::time::timeout(SOON, feed.next()).await.is_err()
    }

    async fn next(feed: &mut Feed) -> Item {
        feed.next().await.ok().unwrap()
    }

    #[tokio::test]
    async fn an_empty_queue_takes_one_event_of_any_size() {
        let (sink, mut feed) = feed();
        sink.push(vec![7; 3 * BUDGET]);
        sink.push(event(1));
        assert_eq!(
            next(&mut feed).await,
            Item::Event(vec![7; 3 * BUDGET])
        );
        assert_eq!(next(&mut feed).await, Item::Gap);
        assert_eq!(weight(&feed), 0);
        assert!(pending(&mut feed).await);
    }

    #[tokio::test]
    async fn a_full_queue_drops_events_and_tells_one_gap_where_they_were()
     {
        let (sink, mut feed) = feed();
        let fits = BUDGET / (8 + OVERHEAD);
        let begun = Instant::now();
        for n in 0..2 * fits as u64 {
            sink.push(event(n));
        }
        assert!(begun.elapsed() < Duration::from_secs(1));
        assert!(weight(&feed) <= BUDGET);
        let mut seen = Vec::new();
        for _ in 0..fits {
            seen.push(value(&next(&mut feed).await).unwrap());
        }
        assert_eq!(seen, (0..fits as u64).collect::<Vec<_>>());
        assert_eq!(next(&mut feed).await, Item::Gap);
        assert!(pending(&mut feed).await);
        sink.push(event(9_999_999));
        assert_eq!(
            next(&mut feed).await,
            Item::Event(event(9_999_999))
        );
        assert!(pending(&mut feed).await);
    }

    #[tokio::test]
    async fn a_loss_that_leaves_room_for_a_gap_tells_one_gap() {
        let (sink, mut feed) = feed();
        let half = vec![0; BUDGET / 2];
        for _ in 0..4 {
            sink.push(half.clone());
        }
        assert_eq!(next(&mut feed).await, Item::Event(half));
        assert_eq!(next(&mut feed).await, Item::Gap);
        assert!(pending(&mut feed).await);
    }

    #[tokio::test]
    async fn a_gap_sits_between_the_events_before_the_loss_and_those_after()
     {
        let (sink, mut feed) = feed();
        let big = BUDGET - 4 * (8 + OVERHEAD);
        sink.push(event(1));
        sink.push(vec![0; big]);
        sink.push(event(2));
        sink.push(event(3));
        sink.push(event(4));
        sink.push(event(5));
        assert_eq!(next(&mut feed).await, Item::Event(event(1)));
        assert_ne!(next(&mut feed).await, Item::Gap);
        sink.push(event(6));
        let mut rest = Vec::new();
        for _ in 0..4 {
            rest.push(next(&mut feed).await);
        }
        assert_eq!(
            rest,
            [
                Item::Event(event(2)),
                Item::Event(event(3)),
                Item::Gap,
                Item::Event(event(6)),
            ]
        );
        assert!(pending(&mut feed).await);
    }

    #[tokio::test]
    async fn a_gap_from_upstream_wakes_a_waiting_reader() {
        let (sink, mut feed) = feed();
        let reading =
            tokio::spawn(async move { next(&mut feed).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        sink.gap();
        let read = tokio::time::timeout(SOON, reading).await;
        assert_eq!(read.unwrap().unwrap(), Item::Gap);
    }

    #[tokio::test]
    async fn a_closed_feed_gives_what_is_queued_then_why() {
        let (sink, mut feed) = feed();
        sink.push(event(1));
        sink.push(event(2));
        sink.close(&anyhow!("destroyed"));
        assert_eq!(next(&mut feed).await, Item::Event(event(1)));
        assert_eq!(next(&mut feed).await, Item::Event(event(2)));
        assert_eq!(why(feed.next().await), "destroyed");
        assert_eq!(why(feed.next().await), "destroyed");
    }

    #[tokio::test]
    async fn the_first_close_wins_when_two_callbacks_share_the_sink()
    {
        let (sink, mut feed) = feed();
        let sink = Arc::new(sink);
        let pushed = sink.clone();
        let closer = sink.clone();
        pushed.push(event(1));
        closer.close(&anyhow!("disconnected"));
        pushed.push(event(2));
        closer.close(&anyhow!("stopping"));
        drop((pushed, closer, sink));
        assert_eq!(value(&next(&mut feed).await), Some(1));
        assert_eq!(value(&next(&mut feed).await), Some(2));
        assert_eq!(why(feed.next().await), "disconnected");
    }

    #[tokio::test]
    async fn a_close_wakes_a_reader_that_waits_while_the_sink_lives()
    {
        let (sink, mut feed) = feed();
        let reading = tokio::spawn(async move { feed.next().await });
        tokio::task::yield_now().await;
        sink.close(&anyhow!("disconnected"));
        let read = tokio::time::timeout(SOON, reading).await;
        assert_eq!(why(read.unwrap().unwrap()), "disconnected");
        drop(sink);
    }

    #[tokio::test]
    async fn a_dropped_sink_wakes_its_reader_with_dropped() {
        let (sink, mut feed) = feed();
        sink.push(event(1));
        let reading = tokio::spawn(async move {
            let first = next(&mut feed).await;
            (first, why(feed.next().await))
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(sink);
        let read = tokio::time::timeout(SOON, reading).await;
        assert_eq!(
            read.unwrap().unwrap(),
            (Item::Event(event(1)), "dropped".to_owned())
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_slow_reader_never_gets_an_event_twice_or_out_of_order()
    {
        let (sink, mut feed) = feed();
        let pushing = std::thread::spawn(move || {
            for n in 0..400_000u64 {
                sink.push(event(n));
            }
        });
        let mut last = None;
        let mut events = 0u64;
        let mut after_gap = false;
        loop {
            match tokio::time::timeout(SOON, feed.next()).await {
                Ok(Ok(item @ Item::Event(_))) => {
                    let n = value(&item).unwrap();
                    assert!(last < Some(n), "{last:?} then {n}");
                    if let Some(last) = last
                        && n != last + 1
                    {
                        assert!(after_gap, "{last} then {n}, no gap");
                    }
                    last = Some(n);
                    events += 1;
                    after_gap = false;
                }
                Ok(Ok(Item::Gap)) => after_gap = true,
                Ok(Err(Closed::Dropped)) => break,
                Ok(Err(Closed::Ended(_))) | Err(_) => {
                    panic!("the feed ended or stalled")
                }
            }
            if events.is_multiple_of(64) {
                tokio::time::sleep(Duration::from_micros(200)).await;
            }
        }
        pushing.join().unwrap();
        assert!(last == Some(399_999) || after_gap, "{last:?}");
    }
}
