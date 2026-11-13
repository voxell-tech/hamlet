//! One actor started, called and stopped through [`RunningActor`] on
//! the in-memory test host.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use hamlet_actor_error::ActorError;
use tokio::sync::oneshot;

use super::{RunningActor, StartRequest};
use crate::actor::internal_storage::queries::TAKE_DUE_SCHEDULES_SQL;
use crate::testing::{TestHost, TestSqliteWriteGate};
use crate::types::ActorKeySegment;
use crate::{
    ActorConfig, ActorEvent, ActorFactory, ActorStart, ShutdownKind,
};

/// A call's deadline where a test does not test it: the kind's action
/// timeout decides.
const NO_DEADLINE: Duration = Duration::MAX;

/// An actor whose `input` action replies with its start input, and
/// whose `connections` action replies with the inspector's count of
/// connection changes.
fn factory() -> Arc<ActorFactory> {
    let config = ActorConfig {
        name: Some("echo".to_owned()),
        ..ActorConfig::default()
    };
    Arc::new(ActorFactory::new(config, |start: ActorStart| {
        Box::pin(async move {
            let ActorStart {
                ctx,
                input,
                mut events,
                ..
            } = start;
            while let Some(event) = events.recv().await {
                match event {
                    ActorEvent::Action { name, reply, .. } => {
                        let output = match name.as_str() {
                            "input" => {
                                input.clone().unwrap_or_default()
                            }
                            _ => ctx
                                .inspector_snapshot()
                                .connections_revision
                                .to_le_bytes()
                                .to_vec(),
                        };
                        reply.send(Ok(output));
                    }
                    ActorEvent::SerializeState { reply, .. } => {
                        reply.send(Ok(Vec::new()));
                    }
                    ActorEvent::RunGracefulCleanup {
                        reply, ..
                    }
                    | ActorEvent::ConnectionPreflight {
                        reply, ..
                    }
                    | ActorEvent::ConnectionOpen { reply, .. }
                    | ActorEvent::DisconnectConn { reply, .. } => {
                        reply.send(Ok(()));
                    }
                    _ => {}
                }
            }
            Ok(())
        })
    }))
}

/// An actor that never answers an action, with the given action
/// timeout.
fn mute_factory(action_timeout: Duration) -> Arc<ActorFactory> {
    let config = ActorConfig {
        name: Some("mute".to_owned()),
        action_timeout,
        ..ActorConfig::default()
    };
    Arc::new(ActorFactory::new(config, |start: ActorStart| {
        Box::pin(async move {
            let mut events = start.events;
            let mut held = Vec::new();
            while let Some(event) = events.recv().await {
                match event {
                    ActorEvent::Action { reply, .. } => {
                        held.push(reply);
                    }
                    ActorEvent::SerializeState { reply, .. } => {
                        reply.send(Ok(Vec::new()));
                    }
                    ActorEvent::RunGracefulCleanup {
                        reply, ..
                    }
                    | ActorEvent::ConnectionPreflight {
                        reply, ..
                    }
                    | ActorEvent::ConnectionOpen { reply, .. }
                    | ActorEvent::DisconnectConn { reply, .. } => {
                        reply.send(Ok(()));
                    }
                    _ => {}
                }
            }
            Ok(())
        })
    }))
}

async fn start(input: Option<Vec<u8>>) -> RunningActor {
    start_with(factory(), input).await
}

async fn start_with(
    factory: Arc<ActorFactory>,
    input: Option<Vec<u8>>,
) -> RunningActor {
    RunningActor::start(StartRequest {
        actor_id: "echo/a".to_owned(),
        name: "echo".to_owned(),
        key: vec![ActorKeySegment::String("a".to_owned())],
        generation: 1,
        factory,
        handle: TestHost::new_for_tests(),
        new_storage: false,
        input,
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn the_first_start_hands_the_actor_its_input() {
    let actor = start(Some(b"hello".to_vec())).await;
    assert_eq!(
        actor
            .action("input", Vec::new(), NO_DEADLINE)
            .await
            .unwrap(),
        b"hello"
    );
    actor.stop(ShutdownKind::Sleep).await;
}

#[tokio::test]
async fn a_started_actor_has_an_inspector() {
    let actor = start(None).await;
    let reply = actor
        .action("connections", Vec::new(), NO_DEADLINE)
        .await
        .unwrap();
    let revision = u64::from_le_bytes(reply.try_into().unwrap());
    assert!(revision > 0, "no inspector saw the action's connection");
    actor.stop(ShutdownKind::Sleep).await;
}

#[tokio::test]
async fn a_dropped_action_call_closes_its_connection() {
    let actor =
        start_with(mute_factory(Duration::from_secs(60)), None).await;
    tokio::time::timeout(
        Duration::from_millis(100),
        actor.action("wait", Vec::new(), NO_DEADLINE),
    )
    .await
    .expect_err("the action should still be waiting");
    tokio::time::timeout(Duration::from_secs(5), async {
        while actor.ctx.active_connection_count() > 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the dropped call's connection is still registered");
}

#[tokio::test]
async fn an_action_that_times_out_closes_its_connection() {
    let actor =
        start_with(mute_factory(Duration::from_millis(100)), None)
            .await;
    let error = actor
        .action("wait", Vec::new(), NO_DEADLINE)
        .await
        .unwrap_err();
    assert_eq!(
        ActorError::extract(&error).code(),
        "action_timed_out"
    );
    assert_eq!(actor.ctx.active_connection_count(), 0);
}

#[tokio::test]
async fn a_deadline_shorter_than_the_action_timeout_ends_the_action()
{
    let actor =
        start_with(mute_factory(Duration::from_secs(60)), None).await;
    let begun = tokio::time::Instant::now();
    let error = actor
        .action("wait", Vec::new(), Duration::from_millis(100))
        .await
        .unwrap_err();
    assert_eq!(
        ActorError::extract(&error).code(),
        "action_timed_out"
    );
    assert!(begun.elapsed() < Duration::from_secs(5));
    assert_eq!(actor.ctx.active_connection_count(), 0);
}

/// The threads that panicked, by name.
static PANICKED: std::sync::Mutex<Vec<String>> =
    std::sync::Mutex::new(Vec::new());

/// Records the name of each thread that panics, from the first call
/// on.
fn record_panics() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let name = std::thread::current()
                .name()
                .unwrap_or_default()
                .to_owned();
            PANICKED.lock().unwrap().push(name);
            previous(info);
        }));
    });
}

/// Whether this thread panicked since [`record_panics`]. A
/// current-thread runtime runs its tasks on the test's own thread.
fn this_thread_panicked() -> bool {
    let name = std::thread::current().name().map(str::to_owned);
    PANICKED
        .lock()
        .unwrap()
        .iter()
        .any(|panicked| Some(panicked) == name.as_ref())
}

/// Sets its flag when dropped.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn stuck_config(name: &str) -> ActorConfig {
    ActorConfig {
        name: Some(name.to_owned()),
        has_database: true,
        ..ActorConfig::default()
    }
}

/// Starts an actor of `factory` on `host`, and gives up on the start
/// once `parked` resolves, as a host does at its deadline: drops the
/// start's future, then closes the database. Checks that within a
/// second the actor's task ends, dropping everything that holds
/// `host`, that the run handler's future is dropped, setting
/// `dropped`, and that nothing on this thread panicked.
async fn give_up_on_the_start(
    host: Arc<TestHost>,
    factory: Arc<ActorFactory>,
    dropped: &AtomicBool,
    parked: impl Future<Output = ()>,
) {
    record_panics();
    let mut start = Box::pin(RunningActor::start(StartRequest {
        actor_id: "stuck/a".to_owned(),
        name: "stuck".to_owned(),
        key: vec![ActorKeySegment::String("a".to_owned())],
        generation: 1,
        factory,
        handle: host.clone(),
        new_storage: false,
        input: None,
    }));
    tokio::select! {
        started = &mut start => {
            panic!("the start ended before the test gave up: {:?}", started.err());
        }
        () = parked => {}
    }
    drop(start);
    host.close();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Arc::strong_count(&host) > 1
        || !dropped.load(Ordering::SeqCst)
    {
        assert!(
            Instant::now() < deadline,
            "after a second, {} holders of the host, run future \
             dropped: {}",
            Arc::strong_count(&host) - 1,
            dropped.load(Ordering::SeqCst)
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!this_thread_panicked(), "a panic while the start ended");
}

/// A start the host gives up on while the run handler's preamble has
/// not signaled ready.
#[tokio::test]
async fn a_start_given_up_at_the_ready_wait_ends() {
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let factory = ActorFactory::new_with_manual_startup_ready(
        stuck_config("stuck"),
        move |start: ActorStart| {
            let flag = DropFlag(flag.clone());
            Box::pin(async move {
                let _held = (flag, start);
                std::future::pending::<()>().await;
                Ok(())
            })
        },
    );
    give_up_on_the_start(
        TestHost::new_for_tests(),
        Arc::new(factory),
        &dropped,
        tokio::time::sleep(Duration::from_millis(100)),
    )
    .await;
}

/// A start the host gives up on while one of its statements waits
/// behind a transaction the run handler's preamble left open.
#[tokio::test]
async fn a_start_given_up_behind_an_open_transaction_ends() {
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let (ready_tx, ready_rx) = oneshot::channel();
    let ready_tx = std::sync::Mutex::new(Some(ready_tx));
    let factory = ActorFactory::new_with_manual_startup_ready(
        stuck_config("stuck"),
        move |start: ActorStart| {
            let flag = DropFlag(flag.clone());
            let ready_tx = ready_tx.lock().unwrap().take();
            Box::pin(async move {
                let _flag = flag;
                let ActorStart {
                    ctx,
                    events,
                    startup_ready,
                    ..
                } = start;
                let _open = ctx.sql().begin_transaction(None).await?;
                if let Some(ready) = startup_ready {
                    let _ = ready.send(Ok(()));
                }
                if let Some(ready_tx) = ready_tx {
                    let _ = ready_tx.send(());
                }
                let _held = events;
                std::future::pending::<()>().await;
                Ok(())
            })
        },
    );
    give_up_on_the_start(
        TestHost::new_for_tests(),
        Arc::new(factory),
        &dropped,
        async {
            ready_rx.await.unwrap();
            // Long enough for the start to reach its next statement.
            tokio::time::sleep(Duration::from_millis(100)).await;
        },
    )
    .await;
}

/// A start the host gives up on while its drain of overdue schedules
/// waits for the database.
#[tokio::test]
async fn a_start_given_up_during_the_drain_ends() {
    let (entered_tx, mut entered) =
        tokio::sync::mpsc::unbounded_channel();
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let host = TestHost::with_write_gate(TestSqliteWriteGate {
        sql_prefix: TAKE_DUE_SCHEDULES_SQL,
        entered_tx,
        release: release.clone(),
    });
    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let factory = ActorFactory::new(
        stuck_config("stuck"),
        move |start: ActorStart| {
            let flag = DropFlag(flag.clone());
            Box::pin(async move {
                let _flag = flag;
                let mut events = start.events;
                while events.recv().await.is_some() {}
                Ok(())
            })
        },
    );
    give_up_on_the_start(host, Arc::new(factory), &dropped, async {
        entered.recv().await.unwrap();
    })
    .await;
    release.add_permits(1);
}
