//! A client's subscription to an actor of the host: what it gives,
//! what refuses it, what keeps the actor awake, and how it ends.

use std::time::{Duration, Instant};

use at::at;
use code::code;
use files::any_file_of;
#[cfg(target_os = "linux")]
use files::any_file_of_open;
use hamlet_actor_core::ShutdownKind;
use hamlet_host::route::{CallerKind, Operation, Routes};
use hamlet_host::{
    Address, Caller, Host, HostEvent, HostOptions, Item, Subscription,
};
use hamlet_testing::Scratch;
use logs::Logs;
use subscription::{end, next, refusal};
use ticker::{TICKED, WEIGHS, ask, number, tick, watchers};
use tracing_subscriber::filter::LevelFilter;

#[path = "support/at.rs"]
mod at;
#[path = "support/code.rs"]
mod code;
#[path = "support/files.rs"]
mod files;
#[path = "support/logs.rs"]
mod logs;
#[path = "support/subscription.rs"]
mod subscription;
#[path = "support/ticker.rs"]
mod ticker;

/// The time a ticker stays awake with nothing open.
const IDLE: Duration = Duration::from_secs(3600);

/// The time a subscription has to give what it holds or its end.
const PROMPT: Duration = Duration::from_secs(1);

fn open(dir: &Scratch, options: HostOptions) -> Host {
    open_idle(dir, IDLE, options)
}

fn open_idle(
    dir: &Scratch,
    idle: Duration,
    options: HostOptions,
) -> Host {
    Host::open_with(dir, ticker::factories(idle), options).unwrap()
}

/// Makes the ticker `ticker/a`.
async fn make(host: &Host) -> Address {
    let a = at("ticker", "a");
    host.create(&a, Vec::new()).await.unwrap();
    a
}

async fn subscribe(
    host: &Host,
    to: &Address,
    event: &str,
) -> Subscription {
    host.subscribe_for_client(to, event).await.unwrap()
}

async fn gives_nothing(subscription: &mut Subscription) {
    let next = tokio::time::timeout(
        Duration::from_millis(200),
        subscription.next(),
    )
    .await;
    assert!(
        next.is_err(),
        "the subscription gave {:?}",
        next.unwrap()
    );
}

/// Waits `within` at most for `holds`.
async fn eventually(
    what: &str,
    within: Duration,
    holds: impl AsyncFn() -> bool,
) {
    let deadline = Instant::now() + within;
    while !holds().await {
        assert!(
            Instant::now() < deadline,
            "after {within:?}, {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(target_os = "linux")]
async fn no_file_of_a_open(dir: &Scratch) {
    eventually(
        "a file of the stopped actor is open",
        Duration::from_secs(10),
        async || !any_file_of_open(dir, "ticker", "a"),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_follows_a_local_actors_event_in_order_stamped_client()
 {
    let dir = Scratch::new("subscribe-follow");
    let host = open(&dir, HostOptions::default());
    let a = make(&host).await;
    let mut ticks = subscribe(&host, &a, TICKED).await;
    for _ in 0..10 {
        tick(&host, &a, 0).await;
    }
    let mut given = Vec::new();
    for _ in 0..10 {
        given.push(number(&next(&mut ticks, PROMPT).await));
    }
    gives_nothing(&mut ticks).await;
    let watching = watchers(&host, &a).await;
    host.shutdown().await.unwrap();
    assert_eq!(given, (1..=10).map(Some).collect::<Vec<_>>());
    assert_eq!(watching, [Some(Caller::Client)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_to_a_key_with_no_file_fails_with_not_found_and_makes_no_file()
 {
    let dir = Scratch::new("subscribe-not-found");
    let host = open(&dir, HostOptions::default());
    let error = refusal(
        host.subscribe_for_client(&at("ticker", "none"), TICKED)
            .await,
    );
    host.shutdown().await.unwrap();
    assert!(code(&error) == "actor.not_found", "{error:#}");
    assert!(!any_file_of(&dir, "ticker", "none"));
}

#[tokio::test]
async fn a_subscribe_the_route_table_refuses_fails_with_actor_refused_and_is_reported()
 {
    let dir = Scratch::new("subscribe-refused");
    let host = open(
        &dir,
        HostOptions {
            routes: Routes::only(&[(CallerKind::Client, &["realm"])]),
            ..HostOptions::default()
        },
    );
    let a = make(&host).await;
    host.stop(&a, ShutdownKind::Sleep).await.unwrap();
    let mut events = host.events();
    let logs = Logs::default();
    let logging = logs.record(LevelFilter::WARN);
    let error = refusal(host.subscribe_for_client(&a, TICKED).await);
    drop(logging);
    let started = host.is_running(&a);
    host.shutdown().await.unwrap();
    assert!(code(&error) == "actor.refused", "{error:#}");
    assert_eq!(
        events.try_recv().unwrap(),
        HostEvent::Refused {
            from: Caller::Client,
            to: a,
            operation: Operation::Subscribe {
                event: TICKED.to_owned(),
            },
        }
    );
    let logged = logs.text();
    assert!(
        logged.lines().any(|line| {
            ["WARN", "ticker/a", "Subscribe"]
                .iter()
                .all(|word| line.contains(word))
        }),
        "{logged}"
    );
    assert!(!started, "the refused subscribe started the actor");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_the_actor_refuses_fails_with_its_error_and_leaves_no_connection()
 {
    let dir = Scratch::new("subscribe-forbidden");
    let host = open(&dir, HostOptions::default());
    let a = make(&host).await;
    let error =
        refusal(host.subscribe_for_client(&a, "secret").await);
    let watching = watchers(&host, &a).await;
    host.shutdown().await.unwrap();
    assert!(code(&error) == "auth.forbidden", "{error:#}");
    assert_eq!(watching, []);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribe_that_outlasts_the_start_deadline_fails_with_not_ready_and_leaves_no_connection()
 {
    let dir = Scratch::new("subscribe-not-ready");
    let host = open(
        &dir,
        HostOptions {
            start_deadline: Duration::from_secs(1),
            ..HostOptions::default()
        },
    );
    let a = make(&host).await;
    let begun = Instant::now();
    let error = refusal(host.subscribe_for_client(&a, "slow").await);
    let took = begun.elapsed();
    tokio::time::sleep_until((begun + WEIGHS).into()).await;
    let watching = watchers(&host, &a).await;
    host.shutdown().await.unwrap();
    assert!(code(&error) == "actor.not_ready", "{error:#}");
    assert!(took < Duration::from_secs(2), "{took:?}");
    assert_eq!(watching, []);
}

/// The stop lands while the actor's `onConnect` runs, and the
/// subscribe reaches the actor once its event loop has ended.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_that_overtakes_a_subscribe_while_its_connection_opens_fails_it_with_the_stops_end()
 {
    for destroy in [false, true] {
        let dir = Scratch::new("subscribe-overtaken");
        let host = open(&dir, HostOptions::default());
        let a = make(&host).await;
        ask(&host, &a, "slow_open").await.unwrap();
        let subscribing = tokio::spawn({
            let (host, a) = (host.clone(), a.clone());
            async move {
                refusal(host.subscribe_for_client(&a, TICKED).await)
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let how = if destroy {
            ShutdownKind::Destroy
        } else {
            ShutdownKind::Sleep
        };
        host.stop(&a, how).await.unwrap();
        let error = subscribing.await.unwrap();
        host.shutdown().await.unwrap();
        let stop = if destroy {
            "actor.destroyed"
        } else {
            "actor.stopping"
        };
        assert!(code(&error) == stop, "{error:#}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_open_subscription_keeps_its_actor_awake_and_a_dropped_one_lets_it_sleep()
 {
    let dir = Scratch::new("subscribe-awake");
    let host = open_idle(
        &dir,
        Duration::from_millis(200),
        HostOptions::default(),
    );
    let a = make(&host).await;
    let ticks = subscribe(&host, &a, TICKED).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let awake = host.is_running(&a);
    drop(ticks);
    eventually(
        "the actor is awake",
        Duration::from_secs(2),
        async || !host.is_running(&a),
    )
    .await;
    host.shutdown().await.unwrap();
    assert!(awake, "the actor slept under a subscription");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_with_stopping_when_the_actor_sleeps() {
    for by_host in [false, true] {
        let dir = Scratch::new("subscribe-sleep");
        let host = open(&dir, HostOptions::default());
        let a = make(&host).await;
        let mut ticks = subscribe(&host, &a, TICKED).await;
        tick(&host, &a, 0).await;
        if by_host {
            host.stop(&a, ShutdownKind::Sleep).await.unwrap();
        } else {
            ask(&host, &a, "sleep").await.unwrap();
        }
        let given = number(&next(&mut ticks, PROMPT).await);
        let ended = end(&mut ticks, PROMPT).await;
        #[cfg(target_os = "linux")]
        no_file_of_a_open(&dir).await;
        let again = tick(&host, &a, 0).await;
        host.shutdown().await.unwrap();
        assert_eq!(given, Some(1));
        assert_eq!(
            ended, "actor.stopping",
            "stopped by the host: {by_host}"
        );
        assert_eq!(again, 1);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_with_destroyed_when_the_actor_is_destroyed()
{
    for by_host in [false, true] {
        let dir = Scratch::new("subscribe-destroy");
        let host = open(&dir, HostOptions::default());
        let a = make(&host).await;
        let mut ticks = subscribe(&host, &a, TICKED).await;
        if by_host {
            host.stop(&a, ShutdownKind::Destroy).await.unwrap();
        } else {
            ask(&host, &a, "destroy").await.unwrap();
        }
        let ended = end(&mut ticks, PROMPT).await;
        eventually(
            "the destroyed actor's files are there",
            Duration::from_secs(10),
            async || !any_file_of(&dir, "ticker", "a"),
        )
        .await;
        #[cfg(target_os = "linux")]
        no_file_of_a_open(&dir).await;
        drop(host);
        let deadline = Instant::now() + Duration::from_secs(5);
        let second = loop {
            match Host::open_with(
                &dir,
                ticker::factories(IDLE),
                HostOptions::default(),
            ) {
                Ok(second) => break second,
                Err(error) => {
                    assert!(Instant::now() < deadline, "{error:#}");
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        make(&second).await;
        let again = tick(&second, &a, 0).await;
        second.shutdown().await.unwrap();
        drop(ticks);
        assert_eq!(
            ended, "actor.destroyed",
            "destroyed by the host: {by_host}"
        );
        assert_eq!(again, 1);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_with_stopping_when_the_actor_crashes() {
    let dir = Scratch::new("subscribe-crash");
    let host = open(&dir, HostOptions::default());
    let a = make(&host).await;
    let mut ticks = subscribe(&host, &a, TICKED).await;
    ask(&host, &a, "crash").await.unwrap();
    let ended = end(&mut ticks, PROMPT).await;
    host.shutdown().await.unwrap();
    assert_eq!(ended, "actor.stopping");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_with_stopping_when_the_actors_event_loop_fails()
 {
    let dir = Scratch::new("subscribe-fail");
    let host = open(&dir, HostOptions::default());
    let a = make(&host).await;
    let mut ticks = subscribe(&host, &a, TICKED).await;
    ask(&host, &a, "fail").await.unwrap();
    let ended = end(&mut ticks, PROMPT).await;
    host.shutdown().await.unwrap();
    assert_eq!(ended, "actor.stopping");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_when_a_stop_that_misses_its_deadline_begins()
{
    let dir = Scratch::new("subscribe-stuck");
    let host = open(
        &dir,
        HostOptions {
            stop_deadline: Duration::from_secs(1),
            ..HostOptions::default()
        },
    );
    let a = make(&host).await;
    let mut ticks = subscribe(&host, &a, TICKED).await;
    ask(&host, &a, "hang").await.unwrap();
    let begun = Instant::now();
    let stop = tokio::spawn({
        let (host, a) = (host.clone(), a.clone());
        async move { host.stop(&a, ShutdownKind::Sleep).await }
    });
    let ended = end(&mut ticks, PROMPT).await;
    let took = begun.elapsed();
    let stopped = stop.await.unwrap();
    let _ = host.shutdown().await;
    assert_eq!(ended, "actor.stopping");
    assert!(took < Duration::from_millis(200), "{took:?}");
    let error = stopped.expect_err("the hung stop succeeded");
    assert!(code(&error) == "actor.shutdown_timeout", "{error:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_with_stopping_when_the_host_shuts_down() {
    for shut_by_drop in [false, true] {
        let dir = Scratch::new("subscribe-shutdown");
        let host = open(&dir, HostOptions::default());
        let a = make(&host).await;
        let mut ticks = subscribe(&host, &a, TICKED).await;
        if shut_by_drop {
            drop(host);
        } else {
            host.shutdown().await.unwrap();
        }
        assert_eq!(
            end(&mut ticks, PROMPT).await,
            "actor.stopping",
            "shut by the host's drop: {shut_by_drop}"
        );
    }
}

/// Ticks of 32 KiB, so 2 MiB of them overfill the reader's queue.
#[tokio::test]
async fn a_full_queue_drops_events_gives_a_gap_and_logs_nothing() {
    const PADDING: u64 = 32 << 10;
    let dir = Scratch::new("subscribe-full");
    let host = open(&dir, HostOptions::default());
    let a = make(&host).await;
    let mut ticks = subscribe(&host, &a, TICKED).await;
    let logs = Logs::default();
    let logging = logs.record(LevelFilter::ERROR);
    let mut slowest = Duration::ZERO;
    for _ in 0..(2 << 20) / PADDING {
        let begun = Instant::now();
        tick(&host, &a, PADDING).await;
        slowest = slowest.max(begun.elapsed());
    }
    drop(logging);
    let mut given = Vec::new();
    loop {
        let item = next(&mut ticks, PROMPT).await;
        given.push(number(&item));
        if item == Item::Gap {
            break;
        }
    }
    let later = tick(&host, &a, 0).await;
    let after = number(&next(&mut ticks, PROMPT).await);
    host.shutdown().await.unwrap();
    assert!(slowest < Duration::from_secs(1), "{slowest:?}");
    assert_eq!(logs.text(), "");
    let (gap, events) = given.split_last().unwrap();
    assert_eq!(*gap, None);
    assert!(!events.is_empty());
    assert_eq!(
        events,
        (1..).map(Some).take(events.len()).collect::<Vec<_>>()
    );
    assert_eq!(after, Some(later));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscription_dropped_on_a_plain_thread_closes_its_connection()
 {
    let dir = Scratch::new("subscribe-drop-thread");
    let host = open(&dir, HostOptions::default());
    let a = make(&host).await;
    let ticks = subscribe(&host, &a, TICKED).await;
    std::thread::spawn(move || drop(ticks)).join().unwrap();
    eventually("a connection is open", PROMPT, async || {
        watchers(&host, &a).await.is_empty()
    })
    .await;
    host.shutdown().await.unwrap();
}

#[test]
fn a_subscription_dropped_after_its_runtime_is_gone_does_not_panic() {
    let dir = Scratch::new("subscribe-drop-late");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let ticks = runtime.block_on(async {
        let host = open(&dir, HostOptions::default());
        let a = make(&host).await;
        let ticks = subscribe(&host, &a, TICKED).await;
        host.shutdown().await.unwrap();
        ticks
    });
    drop(runtime);
    std::thread::spawn(move || drop(ticks)).join().unwrap();
}

#[test]
fn a_subscription_made_on_another_runtime_closes_its_connection_when_dropped()
 {
    let dir = Scratch::new("subscribe-other-runtime");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (host, a) = runtime.block_on(async {
        let host = open(&dir, HostOptions::default());
        let a = make(&host).await;
        (host, a)
    });
    let other = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let ticks = other.block_on(subscribe(&host, &a, TICKED));
    drop(other);
    drop(ticks);
    runtime.block_on(async {
        eventually("a connection is open", PROMPT, async || {
            watchers(&host, &a).await.is_empty()
        })
        .await;
        host.shutdown().await.unwrap();
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_that_ended_fails_the_same_way_again() {
    let dir = Scratch::new("subscribe-ended");
    let host = open(&dir, HostOptions::default());
    let a = make(&host).await;
    let mut ticks = subscribe(&host, &a, TICKED).await;
    host.stop(&a, ShutdownKind::Destroy).await.unwrap();
    let mut ends = Vec::new();
    for _ in 0..4 {
        ends.push(end(&mut ticks, PROMPT).await);
    }
    host.shutdown().await.unwrap();
    assert_eq!(ends, ["actor.destroyed"; 4]);
}

/// The close at the actor's later sleep finds its generation
/// stopping, and leaves the stream's end to the stop.
#[tokio::test(flavor = "multi_thread")]
async fn the_stream_ends_with_disconnected_when_the_actor_closes_the_connection()
 {
    let dir = Scratch::new("subscribe-closed");
    let host = open(&dir, HostOptions::default());
    let a = make(&host).await;
    let mut first = subscribe(&host, &a, TICKED).await;
    ask(&host, &a, "close").await.unwrap();
    let closed = end(&mut first, PROMPT).await;
    let running = host.is_running(&a);
    let mut second = subscribe(&host, &a, TICKED).await;
    ask(&host, &a, "sleep").await.unwrap();
    eventually("the actor is running", PROMPT, async || {
        !host.is_running(&a)
    })
    .await;
    let slept = end(&mut second, PROMPT).await;
    host.shutdown().await.unwrap();
    assert_eq!(closed, "actor.disconnected");
    assert!(running, "the actor stopped as it closed the connection");
    assert_eq!(slept, "actor.stopping");
}
