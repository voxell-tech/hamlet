use hamlet_actor_core::{ActorConfig, ActorStart};
use hamlet_testing::Scratch;

use super::*;

/// The kind `waker`, whose run handler sets the run wake deadline
/// `wake_at` and then answers only lifecycle events.
fn waker(wake_at: i64) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("waker".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    HashMap::from([(
        "waker".to_owned(),
        ActorFactory::new(config, move |start: ActorStart| {
            Box::pin(async move {
                let ActorStart {
                    ctx, mut events, ..
                } = start;
                ctx.set_run_wake_at(Some(wake_at)).await?;
                while let Some(event) = events.recv().await {
                    counter::answer(event);
                }
                Ok(())
            })
        }),
    )])
}

/// The deadline a stop stores comes from the file as well as from
/// the generation's last alarm: none from a file whose core made
/// no tables, and the run wake deadline with no alarm pushed.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_reads_its_deadline_from_the_file() {
    let dir = Scratch::new("host-stop-deadline");
    let mut bare =
        plain_sqlite::open(&dir.join("bare.sqlite"), true).unwrap();
    bump_generation(
        &mut bare,
        "bare",
        "a",
        &Open::OrCreate,
        &OpenClaim::default(),
    )
    .unwrap();
    assert_eq!(mark_stopped(&bare, None).unwrap(), None);
    assert_eq!(mark_stopped(&bare, Some(5)).unwrap(), Some(5));

    let wake_at = now_ms() + 600_000;
    let host = Host::open(&dir, waker(wake_at)).unwrap();
    let a = Address::new("waker", "a").unwrap();
    host.get_or_start(&a).await.unwrap();
    host.stop(&a, ShutdownKind::Sleep).await.unwrap();
    assert!(matches!(
        host.inner().actors.lock().lives.get(&a),
        Some(Life::Stopped { wake_at: stopped, .. })
            if *stopped == wake_at
    ));
    drop(host);
    let file =
        plain_sqlite::open(&actor_path(&dir, &a), false).unwrap();
    assert_eq!(mark_stopped(&file, None).unwrap(), Some(wake_at));
}

/// The core of an ended generation keeps its handle. Through it,
/// that generation can neither stop nor destroy the actor's
/// running generation, give it an alarm, nor reach its file. If
/// `made_again`, the actor is destroyed and its key made again
/// first: the new file counts its generations from the start
/// again.
async fn an_ended_generations_handle_reaches_nothing(
    made_again: bool,
) {
    let dir = Scratch::new("host-ended-handle");
    let host = Host::open(
        &dir,
        counter::factories(&["counter"], counter::Sleep::WhenIdle),
    )
    .unwrap();
    let counter_a = Address::new("counter", "a").unwrap();
    host.get_or_start(&counter_a).await.unwrap();
    let id = actor_id(&counter_a);
    let running = || {
        host.inner().actors.lock().lives[&counter_a]
            .loaded()
            .unwrap()
            .clone()
    };
    let ended = {
        let generation = running();
        LocalHandle {
            host: Arc::downgrade(host.inner()),
            at: counter_a.clone(),
            generation: generation.id,
            file: generation.file.clone(),
            alarm: generation.alarm.clone(),
        }
    };
    host.stop(&counter_a, ShutdownKind::Sleep).await.unwrap();
    host.get_or_start(&counter_a).await.unwrap();
    if made_again {
        host.stop(&counter_a, ShutdownKind::Destroy).await.unwrap();
        host.get_or_start(&counter_a).await.unwrap();
    }
    let next = running();
    let call = |name: &'static str| {
        host.call(
            &counter_a,
            name,
            Vec::new(),
            Duration::from_secs(600),
        )
    };
    call("increment").await.unwrap();

    ended.sleep_actor(id.clone(), Some(1), None);
    ended.sleep_actor(id.clone(), Some(1), Some("crash".into()));
    ended.stop_actor(id.clone(), Some(1));
    let (ack, acked) = oneshot::channel();
    ended.set_alarm_with_ack(
        id.clone(),
        Some(now_ms() + 60_000),
        Some(1),
        Some(ack),
    );
    acked
        .await
        .expect("the ended generation's alarm was not acked");
    assert!(
        ended
            .sqlite_execute(
                id.clone(),
                "UPDATE counter SET value = 100".to_owned(),
                None,
            )
            .await
            .is_err()
    );

    assert!(matches!(
        host.inner().actors.lock().lives.get(&counter_a),
        Some(Life::Running(generation))
            if Arc::ptr_eq(generation, &next)
                && generation.alarm.lock().is_none()
    ));
    let reply = call("get").await.unwrap();
    assert_eq!(counter::count(reply).unwrap(), 1);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generations_handle_reaches_nothing_of_the_next() {
    an_ended_generations_handle_reaches_nothing(false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generations_handle_reaches_nothing_of_its_key_made_again()
 {
    an_ended_generations_handle_reaches_nothing(true).await;
}
