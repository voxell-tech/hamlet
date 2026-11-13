// This file was changed for Hamlet. See NOTICE in this crate.
use super::*;
use crate::testing::ActorContextHarness;

#[test]
fn build_applies_actor_config_to_owned_subsystems() {
    let config = ActorConfig {
        max_queue_size: 7,
        max_queue_message_size: 11,
        create_conn_state_timeout: Duration::from_millis(123),
        connection_liveness_timeout: Duration::from_millis(456),
        sleep_timeout: Duration::from_millis(789),
        no_sleep: true,
        ..ActorConfig::default()
    };

    let ctx = ActorContextHarness::new().context_with_kv(
        "configured-actor".to_owned(),
        "configured".to_owned(),
        Vec::new(),
        "local".to_owned(),
        crate::kv::tests::new_in_memory(),
        config.clone(),
    );

    let queue_config = ctx.queue_config_for_tests();
    assert_eq!(queue_config.max_queue_size, config.max_queue_size);
    assert_eq!(
        queue_config.max_queue_message_size,
        config.max_queue_message_size
    );

    let connection_config = ctx.connection_config_for_tests();
    assert_eq!(
        connection_config.create_conn_state_timeout,
        config.create_conn_state_timeout
    );
    assert_eq!(
        connection_config.connection_liveness_timeout,
        config.connection_liveness_timeout
    );

    let sleep_config = ctx.sleep_config();
    assert_eq!(sleep_config.sleep_timeout, config.sleep_timeout);
    assert_eq!(sleep_config.no_sleep, config.no_sleep);
}

#[tokio::test]
async fn inspector_attach_guard_notifies_on_threshold_edges() {
    let ctx = ActorContext::new(
        "inspector-actor",
        "actor",
        Vec::new(),
        "local",
    );
    let attach_count = Arc::new(AtomicU32::new(0));
    let (overlay_tx, _) = broadcast::channel(4);
    ctx.configure_inspector_runtime(
        Arc::clone(&attach_count),
        overlay_tx,
    );
    let (lifecycle_tx, mut lifecycle_rx) = mpsc::unbounded_channel();
    ctx.configure_lifecycle_events(Some(lifecycle_tx));

    let first_guard = ctx
        .inspector_attach()
        .expect("inspector runtime should be configured");
    assert_eq!(ctx.inspector_attach_count(), 1);
    assert!(matches!(
        lifecycle_rx.try_recv(),
        Ok(LifecycleEvent::InspectorAttachmentsChanged)
    ));

    let second_guard = ctx
        .inspector_attach()
        .expect("inspector runtime should be configured");
    assert_eq!(ctx.inspector_attach_count(), 2);
    assert!(matches!(
        lifecycle_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    drop(second_guard);
    assert_eq!(ctx.inspector_attach_count(), 1);
    assert!(matches!(
        lifecycle_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    drop(first_guard);
    assert_eq!(ctx.inspector_attach_count(), 0);
    assert!(matches!(
        lifecycle_rx.try_recv(),
        Ok(LifecycleEvent::InspectorAttachmentsChanged)
    ));
}

#[tokio::test]
async fn disconnect_callback_guard_blocks_sleep_until_drop() {
    let ctx = ActorContext::new(
        "actor-disconnect",
        "actor",
        Vec::new(),
        "local",
    );
    ctx.set_started(true);

    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let task = tokio::spawn({
        let ctx = ctx.clone();
        async move {
            ctx.with_disconnect_callback(|| async move {
                let _ = started_tx.send(());
                let _ = release_rx.await;
            })
            .await;
        }
    });

    started_rx.await.expect("disconnect callback should start");
    assert_eq!(ctx.pending_disconnect_count(), 1);
    assert_eq!(
        ctx.can_sleep().await,
        CanSleep::ActiveDisconnectCallbacks
    );

    release_tx
        .send(())
        .expect("disconnect callback should still be waiting");
    task.await.expect("disconnect callback task should join");

    assert_eq!(ctx.pending_disconnect_count(), 0);
    assert_eq!(ctx.can_sleep().await, CanSleep::Yes);
}

#[tokio::test(start_paused = true)]
async fn disconnect_callback_completion_resets_sleep_timer() {
    let ctx = ActorContext::new(
        "actor-disconnect-timer",
        "actor",
        Vec::new(),
        "local",
    );
    ctx.configure_sleep(ActorConfig {
        sleep_timeout: Duration::from_secs(5),
        ..ActorConfig::default()
    });
    ctx.set_started(true);

    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let task = tokio::spawn({
        let ctx = ctx.clone();
        async move {
            ctx.with_disconnect_callback(|| async move {
                let _ = started_tx.send(());
                let _ = release_rx.await;
            })
            .await;
        }
    });
    started_rx.await.expect("disconnect callback should start");

    tokio::time::advance(Duration::from_secs(10)).await;
    tokio::task::yield_now().await;
    assert_eq!(ctx.sleep_request_count(), 0);

    release_tx
        .send(())
        .expect("disconnect callback should still be waiting");
    task.await.expect("disconnect callback task should join");

    tokio::time::advance(Duration::from_secs(5)).await;
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert_eq!(ctx.sleep_request_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn active_run_handler_blocks_sleep_until_cleared() {
    let ctx = ActorContext::new(
        "actor-run-active",
        "actor",
        Vec::new(),
        "local",
    );
    ctx.configure_sleep(ActorConfig {
        sleep_timeout: Duration::from_secs(5),
        ..ActorConfig::default()
    });
    ctx.set_started(true);

    ctx.begin_run_handler();
    assert_eq!(ctx.can_sleep().await, CanSleep::ActiveRunHandler);

    tokio::time::advance(Duration::from_secs(10)).await;
    tokio::task::yield_now().await;
    assert_eq!(ctx.sleep_request_count(), 0);

    ctx.end_run_handler();
    assert_eq!(ctx.can_sleep().await, CanSleep::Yes);
    tokio::task::yield_now().await;

    tokio::time::advance(Duration::from_secs(5)).await;
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert_eq!(ctx.sleep_request_count(), 1);
}

mod moved_tests {
    use std::collections::{BTreeSet, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use anyhow::{Result, anyhow};
    use tokio::sync::{mpsc, oneshot, watch};
    use tokio::time::sleep;

    use super::ActorContext;
    use crate::actor::connection::ConnHandle;
    use crate::actor::messages::ActorEvent;
    use crate::host::{ActorHost, HostHandle};
    use crate::runtime::RuntimeBoxFuture;
    use crate::sqlite::{
        BindParam, ExecuteResult, QueryResult, SqliteBatchStatement,
    };
    use crate::types::ListOpts;
    use crate::{ActorConfig, SqliteDb};

    fn now_timestamp_ms() -> i64 {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
    }

    /// A host that serves no SQL: it gives its client settings and
    /// says which hibernated connections of one actor are live, as
    /// the engine's live tunnel request registry did.
    struct StubHost {
        endpoint: &'static str,
        token: Option<&'static str>,
        namespace: &'static str,
        pool_name: &'static str,
        actor_id: String,
        live_connections: HashSet<[u8; 8]>,
        session: watch::Sender<u64>,
    }

    impl StubHost {
        fn new(
            endpoint: &'static str,
            token: Option<&'static str>,
            namespace: &'static str,
            pool_name: &'static str,
        ) -> Self {
            Self {
                endpoint,
                token,
                namespace,
                pool_name,
                actor_id: String::new(),
                live_connections: HashSet::new(),
                session: watch::channel(0).0,
            }
        }
    }

    impl ActorHost for StubHost {
        fn endpoint(&self) -> &str {
            self.endpoint
        }

        fn token(&self) -> Option<&str> {
            self.token
        }

        fn namespace(&self) -> &str {
            self.namespace
        }

        fn pool_name(&self) -> &str {
            self.pool_name
        }

        fn connection_session(&self) -> Option<u64> {
            Some(*self.session.borrow())
        }

        fn subscribe_connection_session(
            &self,
        ) -> watch::Receiver<u64> {
            self.session.subscribe()
        }

        fn sleep_actor(
            &self,
            _actor_id: String,
            _generation: Option<u32>,
            _error: Option<String>,
        ) {
        }

        fn stop_actor(
            &self,
            _actor_id: String,
            _generation: Option<u32>,
        ) {
        }

        fn set_alarm_with_ack(
            &self,
            _actor_id: String,
            _alarm_ts: Option<i64>,
            _generation: Option<u32>,
            _ack_tx: Option<oneshot::Sender<()>>,
        ) {
        }

        fn sqlite_exec(
            &self,
            _actor_id: String,
            _sql: String,
        ) -> RuntimeBoxFuture<Result<QueryResult>> {
            Box::pin(async {
                Err(anyhow!("stub host serves no sql"))
            })
        }

        fn sqlite_execute(
            &self,
            _actor_id: String,
            _sql: String,
            _params: Option<Vec<BindParam>>,
        ) -> RuntimeBoxFuture<Result<ExecuteResult>> {
            Box::pin(async {
                Err(anyhow!("stub host serves no sql"))
            })
        }

        fn sqlite_execute_batch(
            &self,
            _actor_id: String,
            _statements: Vec<SqliteBatchStatement>,
        ) -> RuntimeBoxFuture<Result<Vec<ExecuteResult>>> {
            Box::pin(async {
                Err(anyhow!("stub host serves no sql"))
            })
        }

        fn hibernatable_connection_is_live(
            &self,
            actor_id: &str,
            _generation: Option<u32>,
            gateway_id: [u8; 4],
            request_id: [u8; 4],
        ) -> bool {
            let mut key = [0; 8];
            key[..4].copy_from_slice(&gateway_id);
            key[4..].copy_from_slice(&request_id);
            actor_id == self.actor_id
                && self.live_connections.contains(&key)
        }
    }

    fn build_host_with_live_connections(
        actor_id: &str,
        live_connections: HashSet<[u8; 8]>,
    ) -> HostHandle {
        Arc::new(StubHost {
            actor_id: actor_id.to_owned(),
            live_connections,
            ..StubHost::new(
                "http://127.0.0.1:1",
                None,
                "test",
                "test",
            )
        })
    }

    fn build_client_host() -> HostHandle {
        Arc::new(StubHost::new(
            "http://127.0.0.1:7777",
            Some("secret"),
            "test-ns",
            "test-pool",
        ))
    }

    #[tokio::test]
    #[expect(deprecated, reason = "exercises the actor KV helpers")]
    async fn kv_helpers_delegate_to_kv_wrapper() {
        let ctx = crate::testing::new_with_kv(
            "actor-1",
            "actor",
            Vec::new(),
            "local",
            crate::kv::tests::new_in_memory(),
        );

        ctx.kv_batch_put(&[(b"alpha".as_slice(), b"1".as_slice())])
            .await
            .expect("kv batch put should succeed");

        let values = ctx
            .kv_batch_get(&[b"alpha".as_slice()])
            .await
            .expect("kv batch get should succeed");
        assert_eq!(values, vec![Some(b"1".to_vec())]);

        let listed = ctx
            .kv_list_prefix(b"alp", ListOpts::default())
            .await
            .expect("kv list prefix should succeed");
        assert_eq!(listed, vec![(b"alpha".to_vec(), b"1".to_vec())]);

        ctx.kv_batch_delete(&[b"alpha".as_slice()])
            .await
            .expect("kv batch delete should succeed");
        let values = ctx
            .kv_batch_get(&[b"alpha".as_slice()])
            .await
            .expect("kv batch get after delete should succeed");
        assert_eq!(values, vec![None]);
    }

    #[test]
    fn client_accessors_read_config_from_wired_envoy_handle() {
        let handle = build_client_host();
        let ctx = ActorContext::build(
            "client-actor".to_owned(),
            "actor".to_owned(),
            Vec::new(),
            "local".to_owned(),
            handle.namespace().to_owned(),
            Some(1),
            handle.pool_name().to_owned(),
            ActorConfig::default(),
            crate::kv::Kv::new(handle.clone(), "client-actor"),
            SqliteDb::new_with_remote_sqlite(
                handle.clone(),
                "client-actor",
                None,
                Some(1),
                false,
                true,
            )
            .expect("test remote sqlite should be configured"),
        );
        ctx.configure_envoy(handle, Some(1));

        assert_eq!(
            ctx.client_endpoint(),
            Some("http://127.0.0.1:7777")
        );
        assert_eq!(ctx.client_token(), Some("secret"));
        assert_eq!(ctx.client_namespace(), Some("test-ns"));
        assert_eq!(ctx.client_pool_name(), Some("test-pool"));
    }

    #[tokio::test]
    async fn connection_helpers_iterate_and_disconnect_without_managed_callback()
     {
        let ctx = crate::testing::new_with_kv(
            "actor-conns",
            "actor",
            Vec::new(),
            "local",
            crate::kv::tests::new_in_memory(),
        );
        let managed_disconnects =
            Arc::new(Mutex::new(Vec::<String>::new()));
        let transport_disconnects =
            Arc::new(Mutex::new(Vec::<String>::new()));

        let conn_a =
            ConnHandle::new("conn-a", vec![1], vec![2], false);
        conn_a.configure_disconnect_handler(Some(Arc::new({
            let managed_disconnects = managed_disconnects.clone();
            move |_reason| {
                let managed_disconnects = managed_disconnects.clone();
                Box::pin(async move {
                    managed_disconnects
                        .lock()
                        .expect(
                            "managed disconnect log lock poisoned",
                        )
                        .push("conn-a".to_owned());
                    Ok(())
                })
            }
        })));
        conn_a.configure_transport_disconnect_handler(Some(
            Arc::new({
                let transport_disconnects =
                    transport_disconnects.clone();
                move |_reason| {
                    let transport_disconnects =
                        transport_disconnects.clone();
                    Box::pin(async move {
                        transport_disconnects
                            .lock()
                            .expect(
                                "transport disconnect log lock \
                                 poisoned",
                            )
                            .push("conn-a".to_owned());
                        Ok(())
                    })
                }
            }),
        ));

        let conn_b =
            ConnHandle::new("conn-b", vec![3], vec![4], false);
        ctx.add_conn(conn_a);
        ctx.add_conn(conn_b);

        assert_eq!(
            ctx.conns()
                .map(|conn| conn.id().to_owned())
                .collect::<Vec<_>>(),
            vec!["conn-a".to_owned(), "conn-b".to_owned()]
        );
        assert_eq!(ctx.conns().len(), 2);

        ctx.disconnect_conn("conn-a".into())
            .await
            .expect("targeted disconnect should succeed");

        assert_eq!(
            transport_disconnects
                .lock()
                .expect("transport disconnect log lock poisoned")
                .as_slice(),
            ["conn-a"]
        );
        assert!(
            managed_disconnects
                .lock()
                .expect("managed disconnect log lock poisoned")
                .is_empty()
        );
        assert_eq!(
            ctx.conns()
                .map(|conn| conn.id().to_owned())
                .collect::<Vec<_>>(),
            vec!["conn-b".to_owned()]
        );
    }

    #[tokio::test]
    async fn take_pending_hibernation_changes_snapshots_removals_without_draining_core_state()
     {
        let ctx = crate::testing::new_with_kv(
            "actor-hibernation-pending",
            "actor",
            Vec::new(),
            "local",
            crate::kv::tests::new_in_memory(),
        );

        ctx.request_hibernation_transport_save("conn-updated");
        ctx.request_hibernation_transport_removal("conn-removed");

        assert_eq!(
            ctx.take_pending_hibernation_changes(),
            vec!["conn-removed".to_owned()]
        );

        let pending = ctx.take_pending_hibernation_changes_inner();
        assert_eq!(
            pending.updated,
            BTreeSet::from(["conn-updated".to_owned()])
        );
        assert_eq!(
            pending.removed,
            BTreeSet::from(["conn-removed".to_owned()])
        );
    }

    #[tokio::test]
    async fn hibernated_connection_is_live_checks_specific_live_registry_entry()
     {
        let ctx = crate::testing::new_with_kv(
            "actor-live-conn",
            "actor",
            Vec::new(),
            "local",
            crate::kv::tests::new_in_memory(),
        );
        ctx.configure_envoy(
            build_host_with_live_connections(
                "actor-live-conn",
                HashSet::from([[1, 2, 3, 4, 5, 6, 7, 8]]),
            ),
            Some(7),
        );

        assert!(
            ctx.hibernated_connection_is_live(
                &[1, 2, 3, 4],
                &[5, 6, 7, 8]
            )
            .expect("matching live connection should be found")
        );
        assert!(
            !ctx.hibernated_connection_is_live(
                &[1, 2, 3, 4],
                &[9, 9, 9, 9]
            )
            .expect("missing live connection should return false")
        );
    }

    #[tokio::test]
    async fn disconnect_conns_continues_past_per_conn_errors() {
        let ctx = crate::testing::new_with_kv(
            "actor-disconnect-conns",
            "actor",
            Vec::new(),
            "local",
            crate::kv::tests::new_in_memory(),
        );

        let conn_a =
            ConnHandle::new("conn-a", vec![1], vec![2], false);
        conn_a.configure_transport_disconnect_handler(Some(
            Arc::new(move |_reason| {
                Box::pin(async move { Err(anyhow!("boom-a")) })
            }),
        ));

        let conn_b =
            ConnHandle::new("conn-b", vec![3], vec![4], false);
        let transport_disconnects =
            Arc::new(Mutex::new(Vec::<String>::new()));
        conn_b.configure_transport_disconnect_handler(Some(
            Arc::new({
                let transport_disconnects =
                    transport_disconnects.clone();
                move |_reason| {
                    let transport_disconnects =
                        transport_disconnects.clone();
                    Box::pin(async move {
                        transport_disconnects
                            .lock()
                            .expect(
                                "transport disconnect log lock \
                                 poisoned",
                            )
                            .push("conn-b".to_owned());
                        Ok(())
                    })
                }
            }),
        ));

        ctx.add_conn(conn_a.clone());
        ctx.add_conn(conn_b);

        let error = ctx
            .disconnect_conns(|conn| conn.id().starts_with("conn-"))
            .await
            .expect_err(
                "bulk disconnect should surface transport failures",
            );
        let error_text = format!("{error:#}");
        assert!(error_text.contains("conn-a"));
        assert!(
            transport_disconnects
                .lock()
                .expect("transport disconnect log lock poisoned")
                .iter()
                .any(|conn_id| conn_id == "conn-b")
        );
        assert!(ctx.conns().any(|conn| conn.id() == "conn-a"));
        assert!(!ctx.conns().any(|conn| conn.id() == "conn-b"));

        let conn_c =
            ConnHandle::new("conn-c", vec![5], vec![6], false);
        conn_c.configure_transport_disconnect_handler(Some(
            Arc::new(move |_reason| {
                Box::pin(async move { Err(anyhow!("boom-c")) })
            }),
        ));
        ctx.add_conn(conn_c);

        let error = ctx
            .disconnect_conns(|conn| {
                conn.id() == "conn-a" || conn.id() == "conn-c"
            })
            .await
            .expect_err(
                "bulk disconnect should aggregate multiple failures",
            );
        let error_text = format!("{error:#}");
        assert!(error_text.contains("conn-a"));
        assert!(error_text.contains("conn-c"));
        assert!(ctx.conns().any(|conn| conn.id() == "conn-a"));
        assert!(ctx.conns().any(|conn| conn.id() == "conn-c"));
    }

    #[tokio::test(start_paused = true)]
    async fn init_alarms_arms_local_alarm_for_persisted_schedule_state()
     {
        let ctx = crate::testing::new_with_kv(
            "actor-init-alarms",
            "actor",
            Vec::new(),
            "local",
            crate::kv::Kv::new_in_memory(),
        );
        let now_ms = 1_700_000_000_000;
        ctx.set_schedule_time_for_tests(now_ms);
        let fired = Arc::new(AtomicUsize::new(0));
        ctx.set_local_alarm_callback(Some(Arc::new({
            let fired = fired.clone();
            move || {
                let fired = fired.clone();
                Box::pin(async move {
                    fired.fetch_add(1, Ordering::SeqCst);
                })
            }
        })));
        ctx.at(now_ms + 20, "tick", &[1])
            .await
            .expect("persist future schedule");
        ctx.cancel_local_alarm_timeouts();
        ctx.init_alarms().await;

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(20)).await;
        tokio::task::yield_now().await;

        assert_eq!(fired.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn drain_overdue_scheduled_events_dispatches_actions_via_actor_inbox()
     {
        let ctx = crate::testing::new_with_kv(
            "actor-overdue-events",
            "actor",
            Vec::new(),
            "local",
            crate::kv::Kv::new_in_memory(),
        );
        let (events_tx, mut events_rx) = mpsc::unbounded_channel();
        ctx.configure_actor_events(Some(events_tx));
        ctx.at(now_timestamp_ms() - 1_000, "tick", &[1, 2, 3])
            .await
            .expect("persist overdue schedule");

        let recv = tokio::spawn(async move {
            match events_rx
                .recv()
                .await
                .expect("scheduled action event should arrive")
            {
                ActorEvent::Action {
                    name,
                    args,
                    conn,
                    scheduled_fire,
                    invocation_telemetry: _,
                    reply,
                } => {
                    assert_eq!(name, "tick");
                    assert_eq!(args, vec![1, 2, 3]);
                    assert!(conn.is_none());
                    let fire = scheduled_fire
                        .expect("scheduled fire metadata");
                    assert_eq!(
                        fire.kind,
                        crate::actor::schedule::ScheduleKind::At
                    );
                    assert!(fire.name.is_none());
                    reply.send(Ok(Vec::new()));
                }
                event => panic!("unexpected event: {event:?}"),
            }
        });

        ctx.drain_overdue_scheduled_events().await.expect(
            "draining overdue scheduled events should succeed",
        );
        recv.await.expect("scheduled action receiver should join");

        assert!(
            ctx.list_scheduled_events().await.unwrap().is_empty()
        );
    }

    #[tokio::test]
    async fn keep_awake_region_blocks_sleep_idle_until_guard_drops() {
        let ctx = crate::testing::new_with_kv(
            "actor-keep-awake",
            "actor",
            Vec::new(),
            "local",
            crate::kv::Kv::new_in_memory(),
        );

        let keep_awake = tokio::spawn({
            let ctx = ctx.clone();
            async move {
                ctx.keep_awake(async {
                    sleep(Duration::from_millis(30)).await;
                })
                .await;
            }
        });

        for _ in 0..20 {
            if ctx.keep_awake_count() > 0 {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }

        assert_eq!(ctx.keep_awake_count(), 1);
        assert!(
            !ctx.wait_for_sleep_idle_window(
                Instant::now() + Duration::from_millis(5)
            )
            .await
        );
        assert!(
            ctx.wait_for_sleep_idle_window(
                Instant::now() + Duration::from_millis(100)
            )
            .await
        );

        keep_awake.await.expect("keep_awake task should complete");
        assert_eq!(ctx.keep_awake_count(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_requests_envoy_on_next_scheduler_tick_without_wall_clock_delay()
     {
        let ctx = crate::testing::new_with_kv(
            "actor-sleep-request",
            "actor",
            Vec::new(),
            "local",
            crate::kv::Kv::new_in_memory(),
        );

        ctx.set_started(true);

        assert_eq!(ctx.sleep_request_count(), 0);

        ctx.sleep().expect("sleep should be accepted after startup");
        tokio::task::yield_now().await;

        assert_eq!(ctx.sleep_request_count(), 1);
    }
}
