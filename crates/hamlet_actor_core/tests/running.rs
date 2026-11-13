//! One actor started, called and stopped through [`RunningActor`] on
//! the in-memory test host.

use std::sync::Arc;

use super::{RunningActor, StartRequest};
use crate::testing::TestHost;
use crate::types::ActorKeySegment;
use crate::{
    ActorConfig, ActorEvent, ActorFactory, ActorStart, ShutdownKind,
};

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

async fn start(input: Option<Vec<u8>>) -> RunningActor {
    RunningActor::start(StartRequest {
        actor_id: "echo/a".to_owned(),
        name: "echo".to_owned(),
        key: vec![ActorKeySegment::String("a".to_owned())],
        generation: 1,
        factory: factory(),
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
        actor.action("input", Vec::new()).await.unwrap(),
        b"hello"
    );
    actor.stop(ShutdownKind::Sleep).await;
}

#[tokio::test]
async fn a_started_actor_has_an_inspector() {
    let actor = start(None).await;
    let reply =
        actor.action("connections", Vec::new()).await.unwrap();
    let revision = u64::from_le_bytes(reply.try_into().unwrap());
    assert!(revision > 0, "no inspector saw the action's connection");
    actor.stop(ShutdownKind::Sleep).await;
}
