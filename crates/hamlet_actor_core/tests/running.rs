//! One actor started, called and stopped through [`RunningActor`] on
//! the in-memory test host.

use std::sync::Arc;
use std::time::Duration;

use hamlet_actor_error::ActorError;

use super::{RunningActor, StartRequest};
use crate::testing::TestHost;
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
