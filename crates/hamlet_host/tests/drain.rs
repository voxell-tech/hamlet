//! The drain of an actor's event channel when its event loop panics,
//! against an event the core sends from another thread at that
//! moment.

use std::hint::spin_loop;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use hamlet_actor_core::actor::schedule::{
    ScheduleKind, ScheduledFireInfo,
};
use hamlet_actor_core::{ActorEvent, ActorEvents, Reply};
use hamlet_actor_error::ActorError;
use tokio::sync::{mpsc, oneshot};

/// The rounds of the race. Aimed as the test aims them, a send lands
/// inside the drain in one round of a few hundred, or more often.
const ROUNDS: u64 = 100_000;

/// One round's half for the sender thread.
struct Round {
    events: mpsc::UnboundedSender<ActorEvent>,
    reply: oneshot::Sender<anyhow::Result<Vec<u8>>>,
    /// How long the sender waits after the start of the round.
    spins: u64,
}

/// The core's dispatch of a one-shot that is due.
fn one_shot(reply: Reply<Vec<u8>>) -> ActorEvent {
    ActorEvent::Action {
        name: "inc".to_owned(),
        args: Vec::new(),
        conn: None,
        scheduled_fire: Some(ScheduledFireInfo {
            kind: ScheduleKind::At,
            id: "one-shot".to_owned(),
            name: None,
            scheduled_at: 0,
            fired_at: 0,
        }),
        invocation_telemetry: None,
        reply,
    }
}

/// Waits until `go` reads `round`, spinning, and yielding now and
/// then so that one core is enough.
fn wait_for(go: &AtomicU64, round: u64) {
    let mut spins = 0u32;
    while go.load(Ordering::Acquire) != round {
        spins = spins.wrapping_add(1);
        if spins.is_multiple_of(1024) {
            thread::yield_now();
        }
        spin_loop();
    }
}

/// A raw actor's event loop panics while the core sends it a one-shot
/// from another thread. A send that has begun when the channel closes
/// lands a moment later, so the drain waits for it. Its handler never
/// ran, so its reply answers `actor.dropped_reply`, which keeps the
/// one-shot to run at the next start, and answers before the drop
/// returns, so a stop does not wait on it. Each round aims its send
/// at the drop: later after a send that came before it, earlier after
/// one that came after.
#[test]
fn a_one_shot_sent_as_the_event_loop_panics_is_kept_and_answered_at_once()
 {
    let (rounds, rounds_rx) = std::sync::mpsc::channel::<Round>();
    let (sent_tx, sent) = std::sync::mpsc::channel::<bool>();
    let go = Arc::new(AtomicU64::new(0));
    let sender = thread::spawn({
        let go = go.clone();
        move || {
            for (number, round) in (1..).zip(rounds_rx) {
                wait_for(&go, number);
                for _ in 0..round.spins {
                    spin_loop();
                }
                let landed = round
                    .events
                    .send(one_shot(round.reply.into()))
                    .is_ok();
                sent_tx.send(landed).unwrap();
            }
        }
    });

    // How many spins the sender waits more than the test.
    let mut lead: i64 = 0;
    let (mut before, mut after, mut panicked, mut pending) =
        (0u64, 0u64, 0u64, 0u64);
    for number in 1..=ROUNDS {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (reply, mut answer) = oneshot::channel();
        let events = ActorEvents::from(events_rx);
        // Held until the answer is read. An event left in the channel
        // would otherwise drop with its last sender and answer as if
        // the drain had reached it.
        let held = events_tx.clone();
        rounds
            .send(Round {
                events: events_tx,
                reply,
                spins: lead.max(0).unsigned_abs(),
            })
            .unwrap();
        go.store(number, Ordering::Release);
        for _ in 0..lead.min(0).unsigned_abs() {
            spin_loop();
        }
        let _ = catch_unwind(AssertUnwindSafe(move || {
            let _events = events;
            resume_unwind(Box::new("the event loop panics"));
        }));
        if sent.recv().unwrap() {
            before += 1;
            lead += 1;
        } else {
            after += 1;
            lead -= 1;
        }
        match answer.try_recv() {
            Ok(Err(error)) => {
                let error = ActorError::extract(&error);
                match (error.group(), error.code()) {
                    ("actor", "dropped_reply") => {}
                    ("actor", "panicked") => panicked += 1,
                    other => panic!("answered {other:?}"),
                }
            }
            Ok(Ok(_)) => panic!("answered as handled"),
            Err(oneshot::error::TryRecvError::Empty) => pending += 1,
            Err(oneshot::error::TryRecvError::Closed) => {
                panic!("dropped unanswered")
            }
        }
        drop(held);
    }
    drop(rounds);
    sender.join().unwrap();

    assert!(
        before.min(after) > ROUNDS / 4,
        "the sends were not aimed at the drop: {before} came before \
         it and {after} after"
    );
    assert_eq!(
        (panicked, pending),
        (0, 0),
        "of {before} one-shots sent before the drop, some answered \
         as panicked, which deletes a one-shot that never ran, or \
         were still unanswered, which holds a stop for its grace"
    );
}
