// This file was changed for Hamlet. See NOTICE in this crate.
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use parking_lot::Mutex;

use crate::actor::task_types::{
    ShutdownKind, StateMutationReason, UserTaskKind,
};
use crate::metrics::prometheus::{
    CounterVec, HistogramOpts, HistogramVec, IntCounterVec,
    IntGaugeVec, Opts, Registry,
};
use crate::time::Instant;

const ACTOR_LABELS: &[&str] = &["actor_name"];
const INBOX_LABELS: &[&str] = &["actor_name", "inbox"];
const USER_TASK_LABELS: &[&str] = &["actor_name", "kind"];
const WORK_LABELS: &[&str] = &["actor_name", "kind"];
const SHUTDOWN_LABELS: &[&str] = &["actor_name", "reason"];
const STATE_MUTATION_LABELS: &[&str] = &["actor_name", "reason"];
const DIRECT_SHUTDOWN_LABELS: &[&str] =
    &["actor_name", "subsystem", "operation"];
const RUNTIME_CALLBACK_LABELS: &[&str] = &["actor_name", "callback"];
const STARTUP_PHASE_LABELS: &[&str] =
    &["actor_name", "phase", "is_new", "result"];
const STARTUP_KIND_UNKNOWN: u8 = 0;
const STARTUP_KIND_NEW: u8 = 1;
const STARTUP_KIND_EXISTING: u8 = 2;

pub(crate) mod startup_phase {
    #[derive(Clone, Copy, Debug)]
    #[repr(u8)]
    pub(crate) enum StartupPhase {
        Unknown = 0,
        LoadPersisted = 1,
        CoreInit = 2,
        RuntimePreamble = 3,
        PostReady = 4,
        Total = 5,
        /// `ensure_startup_storage`, before persisted state loads.
        StartupStorage = 6,
        /// Probing for the internal SQLite schema.
        SchemaCheck = 7,
        /// Probing legacy KV before bootstrapping a fresh actor.
        KvProbe = 8,
        /// The fresh actor's inspector token KV mirror write. It runs
        /// in the background once the startup commit is durable.
        InspectorToken = 10,
        /// Opening a fresh actor's database. Its bootstrap commits
        /// later, with its first transaction.
        DatabaseOpen = 11,
        /// From the runtime receiving the start to its first storage
        /// request, the start of `ensure_startup_storage`: context
        /// build, task spawn and the start handoff.
        BeforeStorage = 12,
        /// Making a new actor's startup writes durable as one commit:
        /// its bootstrap, schema migrations and initial state.
        StartupCommit = 13,
        /// The startup commit's round trip from the VFS to the engine
        /// and back, the part of `StartupCommit` spent outside this
        /// process.
        StartupCommitTransport = 14,
    }

    impl StartupPhase {
        pub(crate) fn as_label(self) -> &'static str {
            match self {
                StartupPhase::Unknown => "unknown",
                StartupPhase::LoadPersisted => "load_persisted",
                StartupPhase::CoreInit => "core_init",
                StartupPhase::RuntimePreamble => "runtime_preamble",
                StartupPhase::PostReady => "post_ready",
                StartupPhase::Total => "total",
                StartupPhase::StartupStorage => "startup_storage",
                StartupPhase::SchemaCheck => "schema_check",
                StartupPhase::KvProbe => "kv_probe",
                StartupPhase::InspectorToken => "inspector_token",
                StartupPhase::DatabaseOpen => "database_open",
                StartupPhase::BeforeStorage => "before_storage",
                StartupPhase::StartupCommit => "startup_commit",
                StartupPhase::StartupCommitTransport => {
                    "startup_commit_transport"
                }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct ActorMetrics {
    inner: Arc<ActorMetricInner>,
}

/// Records the total startup metric when the startup attempt leaves
/// scope.
///
/// Startup phases record their own durations at the phase boundary.
/// This guard owns the total duration so early returns cannot forget
/// the total error metric.
pub(crate) struct StartupTimer {
    metrics: ActorMetrics,
    started_at: Instant,
    is_new: Option<bool>,
    finished: bool,
}

#[derive(Debug)]
struct ActorMetricInner {
    labels: ActorMetricLabels,
    state: Mutex<ActorMetricState>,
    /// Counts this actor on `actor_active_count` until it stops.
    active: Mutex<Option<crate::metrics::IntGaugeGuard>>,
    startup_is_new: AtomicU8,
    startup_complete: AtomicBool,
    current_startup_phase: AtomicU8,
    ready_at: Mutex<Option<Instant>>,
}

#[derive(Debug)]
struct ActorMetricLabels {
    actor_name: String,
}

#[derive(Debug, Default)]
struct ActorMetricState {
    queue_depth: i64,
    active_connections: i64,
    lifecycle_inbox_depth: i64,
    dispatch_inbox_depth: i64,
    lifecycle_event_inbox_depth: i64,
    user_tasks_active: BTreeMap<&'static str, i64>,
    http_requests_active: i64,
    keep_awake_active: i64,
    internal_keep_awake_active: i64,
    shutdown_tasks_active: i64,
}

struct ActorMetricCollectors {
    actor_active_count: IntGaugeVec,
    actor_started_total: IntCounterVec,
    actor_stopped_total: IntCounterVec,
    startup_phase_duration_seconds: HistogramVec,
    create_state_duration_seconds: HistogramVec,
    create_vars_duration_seconds: HistogramVec,
    runtime_callback_duration_seconds: HistogramVec,
    queue_depth: IntGaugeVec,
    queue_messages_sent_total: IntCounterVec,
    queue_messages_received_total: IntCounterVec,
    active_connections: IntGaugeVec,
    connections_total: IntCounterVec,
    inbox_depth: IntGaugeVec,
    user_tasks_active: IntGaugeVec,
    user_task_duration_seconds: HistogramVec,
    http_requests_active: IntGaugeVec,
    keep_awake_active: IntGaugeVec,
    shutdown_tasks_active: IntGaugeVec,
    shutdown_wait_seconds: HistogramVec,
    shutdown_timeout_total: CounterVec,
    state_mutation_total: CounterVec,
    direct_subsystem_shutdown_warning_total: CounterVec,
}

static METRICS: LazyLock<ActorMetricCollectors> =
    LazyLock::new(ActorMetricCollectors::new);

impl ActorMetricCollectors {
    fn new() -> Self {
        let actor_active_count = IntGaugeVec::new(
            Opts::new(
                "actor_actor_active_count",
                "current active actors in this process",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_active_count gauge");
        let actor_started_total = IntCounterVec::new(
            Opts::new(
                "actor_actor_started_total",
                "total actors started in this process",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_started_total counter");
        let actor_stopped_total = IntCounterVec::new(
            Opts::new(
                "actor_actor_stopped_total",
                "total actors stopped in this process",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_stopped_total counter");
        let startup_phase_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "actor_actor_startup_phase_duration_seconds",
                "actor startup phase duration in seconds",
            )
            .buckets(startup_duration_buckets()),
            STARTUP_PHASE_LABELS,
        )
        .expect(
            "create actor_startup_phase_duration_seconds histogram",
        );
        let create_state_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "actor_actor_create_state_duration_seconds",
                "typed actor state creation time during startup in \
                 seconds",
            ),
            ACTOR_LABELS,
        )
        .expect(
            "create actor_create_state_duration_seconds histogram",
        );
        let create_vars_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "actor_actor_create_vars_duration_seconds",
                "typed actor vars creation time during startup in \
                 seconds",
            ),
            ACTOR_LABELS,
        )
        .expect(
            "create actor_create_vars_duration_seconds histogram",
        );
        let runtime_callback_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "actor_actor_runtime_callback_duration_seconds",
                "foreign runtime lifecycle callback time during the \
                 startup preamble in seconds",
            )
            .buckets(startup_duration_buckets()),
            RUNTIME_CALLBACK_LABELS,
        )
        .expect(
            "create actor_runtime_callback_duration_seconds \
             histogram",
        );
        let queue_depth = IntGaugeVec::new(
            Opts::new(
                "actor_actor_queue_depth",
                "current actor queue depth",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_queue_depth gauge");
        let queue_messages_sent_total = IntCounterVec::new(
            Opts::new(
                "actor_actor_queue_messages_sent_total",
                "total queue messages sent",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_queue_messages_sent_total counter");
        let queue_messages_received_total = IntCounterVec::new(
            Opts::new(
                "actor_actor_queue_messages_received_total",
                "total queue messages received",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_queue_messages_received_total counter");
        let active_connections = IntGaugeVec::new(
            Opts::new(
                "actor_actor_connections_active",
                "current active actor connections",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_connections_active gauge");
        let connections_total = IntCounterVec::new(
            Opts::new(
                "actor_actor_connections_total",
                "total successfully established actor connections",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_connections_total counter");
        let inbox_depth = IntGaugeVec::new(
            Opts::new(
                "actor_actor_inbox_depth",
                "current actor inbox depth",
            ),
            INBOX_LABELS,
        )
        .expect("create actor_inbox_depth gauge");
        let user_tasks_active = IntGaugeVec::new(
            Opts::new(
                "actor_actor_user_tasks_active",
                "current active actor user tasks",
            ),
            USER_TASK_LABELS,
        )
        .expect("create actor_user_tasks_active gauge");
        let user_task_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "actor_actor_user_task_duration_seconds",
                "actor user task execution time in seconds",
            ),
            USER_TASK_LABELS,
        )
        .expect("create actor_user_task_duration_seconds histogram");
        let http_requests_active = IntGaugeVec::new(
            Opts::new(
                "actor_actor_http_requests_active",
                "current actor-scoped HTTP requests",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_http_requests_active gauge");
        let keep_awake_active = IntGaugeVec::new(
            Opts::new(
                "actor_actor_keep_awake_active",
                "current actor keep-awake work",
            ),
            WORK_LABELS,
        )
        .expect("create actor_keep_awake_active gauge");
        let shutdown_tasks_active = IntGaugeVec::new(
            Opts::new(
                "actor_actor_shutdown_tasks_active",
                "current actor work draining during shutdown",
            ),
            ACTOR_LABELS,
        )
        .expect("create actor_shutdown_tasks_active gauge");
        let shutdown_wait_seconds = HistogramVec::new(
            HistogramOpts::new(
                "actor_actor_shutdown_wait_seconds",
                "actor shutdown wait time in seconds",
            ),
            SHUTDOWN_LABELS,
        )
        .expect("create actor_shutdown_wait_seconds histogram");
        let shutdown_timeout_total = CounterVec::new(
            Opts::new(
                "actor_actor_shutdown_timeout_total",
                "total actor shutdown timeout events",
            ),
            SHUTDOWN_LABELS,
        )
        .expect("create actor_shutdown_timeout_total counter");
        let state_mutation_total = CounterVec::new(
            Opts::new(
                "actor_actor_state_mutation_total",
                "total actor state mutations",
            ),
            STATE_MUTATION_LABELS,
        )
        .expect("create actor_state_mutation_total counter");
        let direct_subsystem_shutdown_warning_total = CounterVec::new(
			Opts::new(
				"actor_actor_direct_subsystem_shutdown_warning_total",
				"total actor shutdown warnings emitted by direct subsystem drains",
			),
			DIRECT_SHUTDOWN_LABELS,
		)
		.expect("create actor_direct_subsystem_shutdown_warning_total counter");

        register_metric(
            &crate::metrics::REGISTRY,
            actor_active_count.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            actor_started_total.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            actor_stopped_total.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            create_state_duration_seconds.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            startup_phase_duration_seconds.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            create_vars_duration_seconds.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            runtime_callback_duration_seconds.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            queue_depth.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            queue_messages_sent_total.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            queue_messages_received_total.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            active_connections.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            connections_total.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            inbox_depth.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            user_tasks_active.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            user_task_duration_seconds.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            http_requests_active.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            keep_awake_active.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            shutdown_tasks_active.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            shutdown_wait_seconds.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            shutdown_timeout_total.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            state_mutation_total.clone(),
        );
        register_metric(
            &crate::metrics::REGISTRY,
            direct_subsystem_shutdown_warning_total.clone(),
        );

        Self {
            actor_active_count,
            actor_started_total,
            actor_stopped_total,
            startup_phase_duration_seconds,
            create_state_duration_seconds,
            create_vars_duration_seconds,
            runtime_callback_duration_seconds,
            queue_depth,
            queue_messages_sent_total,
            queue_messages_received_total,
            active_connections,
            connections_total,
            inbox_depth,
            user_tasks_active,
            user_task_duration_seconds,
            http_requests_active,
            keep_awake_active,
            shutdown_tasks_active,
            shutdown_wait_seconds,
            shutdown_timeout_total,
            state_mutation_total,
            direct_subsystem_shutdown_warning_total,
        }
    }
}

impl ActorMetrics {
    pub(crate) fn new(actor_name: impl Into<String>) -> Self {
        Self::new_with_sqlite_profiling(
            actor_name,
            crate::SqliteProfilingConfig::default(),
        )
    }

    pub(crate) fn new_with_sqlite_profiling(
        actor_name: impl Into<String>,
        _sqlite_profiling: crate::SqliteProfilingConfig,
    ) -> Self {
        let mut _sqlite_profiling =
            _sqlite_profiling.with_process_override();
        let labels = ActorMetricLabels {
            actor_name: actor_name.into(),
        };
        let metrics = &*METRICS;
        let active = crate::metrics::GaugeGuardExt::inc_guard(
            &metrics
                .actor_active_count
                .with_label_values(&labels.as_label_values()),
        );
        metrics
            .actor_started_total
            .with_label_values(&labels.as_label_values())
            .inc();
        Self {
            inner: Arc::new(ActorMetricInner {
                labels,
                state: Mutex::new(ActorMetricState::default()),
                active: Mutex::new(Some(active)),
                startup_is_new: AtomicU8::new(STARTUP_KIND_UNKNOWN),
                startup_complete: AtomicBool::new(false),
                current_startup_phase: AtomicU8::new(
                    startup_phase::StartupPhase::Unknown as u8,
                ),
                ready_at: Mutex::new(None),
            }),
        }
    }

    fn labels(&self) -> &ActorMetricLabels {
        &self.inner.labels
    }

    fn actor_labels(&self) -> [&str; 1] {
        self.labels().as_label_values()
    }

    pub(crate) fn begin_startup(&self) {
        self.inner
            .startup_is_new
            .store(STARTUP_KIND_UNKNOWN, Ordering::Release);
        self.inner.current_startup_phase.store(
            startup_phase::StartupPhase::LoadPersisted as u8,
            Ordering::Release,
        );
        *self.inner.ready_at.lock() = None;
        self.inner.startup_complete.store(false, Ordering::Release);
    }

    /// Begins a timed startup attempt and records `total,error`
    /// unless it succeeds.
    pub(crate) fn begin_startup_timer(&self) -> StartupTimer {
        self.begin_startup();
        StartupTimer {
            metrics: self.clone(),
            started_at: Instant::now(),
            is_new: None,
            finished: false,
        }
    }

    pub(crate) fn set_startup_phase(
        &self,
        phase: startup_phase::StartupPhase,
    ) {
        self.inner
            .current_startup_phase
            .store(phase as u8, Ordering::Release);
    }

    pub(crate) fn set_startup_is_new(&self, is_new: bool) {
        let kind = if is_new {
            STARTUP_KIND_NEW
        } else {
            STARTUP_KIND_EXISTING
        };
        self.inner.startup_is_new.store(kind, Ordering::Release);
    }

    pub(crate) fn finish_startup(&self) {
        *self.inner.ready_at.lock() =
            Some(crate::time::Instant::now());
        self.inner.startup_complete.store(true, Ordering::Release);
    }

    pub(crate) fn observe_startup_phase(
        &self,
        phase: startup_phase::StartupPhase,
        is_new: Option<bool>,
        outcome: &'static str,
        duration: Duration,
    ) {
        let labels = self.actor_labels();
        METRICS
            .startup_phase_duration_seconds
            .with_label_values(&[
                labels[0],
                phase.as_label(),
                optional_is_new_label(is_new),
                outcome,
            ])
            .observe(duration.as_secs_f64());
    }

    /// Records the storage bootstrap, its steps and the
    /// persisted-state load once the start knows whether the actor is
    /// new. They run before it does, so they are held until then.
    pub(crate) fn observe_startup_storage(
        &self,
        is_new: bool,
        storage: Duration,
        storage_phases: &[(startup_phase::StartupPhase, Duration)],
        load_persisted: Duration,
    ) {
        let is_new = Some(is_new);
        self.observe_startup_phase(
            startup_phase::StartupPhase::StartupStorage,
            is_new,
            "success",
            storage,
        );
        for (phase, duration) in storage_phases {
            self.observe_startup_phase(
                *phase, is_new, "success", *duration,
            );
        }
        self.observe_startup_phase(
            startup_phase::StartupPhase::LoadPersisted,
            is_new,
            "success",
            load_persisted,
        );
    }

    pub(crate) fn observe_startup_phase_result<T, E>(
        &self,
        phase: startup_phase::StartupPhase,
        is_new: Option<bool>,
        started_at: Instant,
        result: std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let outcome =
            if result.is_ok() { "success" } else { "error" };
        self.observe_startup_phase(
            phase,
            is_new,
            outcome,
            started_at.elapsed(),
        );
        result
    }

    pub(crate) fn observe_create_state(&self, duration: Duration) {
        METRICS
            .create_state_duration_seconds
            .with_label_values(&self.actor_labels())
            .observe(duration.as_secs_f64());
    }

    pub(crate) fn observe_create_vars(&self, duration: Duration) {
        METRICS
            .create_vars_duration_seconds
            .with_label_values(&self.actor_labels())
            .observe(duration.as_secs_f64());
    }

    pub(crate) fn observe_runtime_callback(
        &self,
        callback: &'static str,
        duration: Duration,
    ) {
        METRICS
            .runtime_callback_duration_seconds
            .with_label_values(&[self.actor_labels()[0], callback])
            .observe(duration.as_secs_f64());
    }

    pub(crate) fn set_queue_depth(&self, depth: u32) {
        let labels = self.actor_labels();
        let mut state = self.inner.state.lock();
        set_aggregated_gauge(
            &mut state.queue_depth,
            i64::from(depth),
            &METRICS.queue_depth,
            &labels,
        );
    }

    pub(crate) fn add_queue_messages_sent(&self, count: u64) {
        METRICS
            .queue_messages_sent_total
            .with_label_values(&self.actor_labels())
            .inc_by(count);
    }

    pub(crate) fn add_queue_messages_received(&self, count: u64) {
        METRICS
            .queue_messages_received_total
            .with_label_values(&self.actor_labels())
            .inc_by(count);
    }

    pub(crate) fn set_active_connections(&self, count: usize) {
        let labels = self.actor_labels();
        let mut state = self.inner.state.lock();
        set_aggregated_gauge(
            &mut state.active_connections,
            usize_to_i64(count),
            &METRICS.active_connections,
            &labels,
        );
    }

    pub(crate) fn inc_connections_total(&self) {
        METRICS
            .connections_total
            .with_label_values(&self.actor_labels())
            .inc();
    }

    pub(crate) fn set_lifecycle_inbox_depth(&self, depth: usize) {
        self.set_inbox_depth("lifecycle", depth);
    }

    pub(crate) fn set_dispatch_inbox_depth(&self, depth: usize) {
        self.set_inbox_depth("dispatch", depth);
    }

    pub(crate) fn set_lifecycle_event_inbox_depth(
        &self,
        depth: usize,
    ) {
        self.set_inbox_depth("lifecycle_event", depth);
    }

    fn set_inbox_depth(&self, inbox: &'static str, depth: usize) {
        let labels = self.actor_labels();
        let mut state = self.inner.state.lock();
        let current = match inbox {
            "lifecycle" => &mut state.lifecycle_inbox_depth,
            "dispatch" => &mut state.dispatch_inbox_depth,
            "lifecycle_event" => {
                &mut state.lifecycle_event_inbox_depth
            }
            _ => unreachable!("unknown inbox metric label"),
        };
        set_aggregated_gauge(
            current,
            usize_to_i64(depth),
            &METRICS.inbox_depth,
            &[labels[0], inbox],
        );
    }

    pub(crate) fn begin_user_task(&self, kind: UserTaskKind) {
        let labels = self.actor_labels();
        let kind = kind.as_metric_label();
        let mut state = self.inner.state.lock();
        let current =
            state.user_tasks_active.entry(kind).or_default();
        let next = (*current).saturating_add(1);
        set_aggregated_gauge(
            current,
            next,
            &METRICS.user_tasks_active,
            &[labels[0], kind],
        );
    }

    pub(crate) fn end_user_task(
        &self,
        kind: UserTaskKind,
        duration: Duration,
    ) {
        let labels = self.actor_labels();
        let kind = kind.as_metric_label();
        {
            let mut state = self.inner.state.lock();
            let current =
                state.user_tasks_active.entry(kind).or_default();
            let next = (*current).saturating_sub(1);
            set_aggregated_gauge(
                current,
                next,
                &METRICS.user_tasks_active,
                &[labels[0], kind],
            );
        }
        METRICS
            .user_task_duration_seconds
            .with_label_values(&[labels[0], kind])
            .observe(duration.as_secs_f64());
    }

    pub(crate) fn set_http_requests_active(&self, count: usize) {
        let labels = self.actor_labels();
        let mut state = self.inner.state.lock();
        set_aggregated_gauge(
            &mut state.http_requests_active,
            usize_to_i64(count),
            &METRICS.http_requests_active,
            &labels,
        );
    }

    pub(crate) fn set_keep_awake_active(&self, count: usize) {
        let labels = self.actor_labels();
        let mut state = self.inner.state.lock();
        set_aggregated_gauge(
            &mut state.keep_awake_active,
            usize_to_i64(count),
            &METRICS.keep_awake_active,
            &[labels[0], "keep_awake"],
        );
    }

    pub(crate) fn set_internal_keep_awake_active(
        &self,
        count: usize,
    ) {
        let labels = self.actor_labels();
        let mut state = self.inner.state.lock();
        set_aggregated_gauge(
            &mut state.internal_keep_awake_active,
            usize_to_i64(count),
            &METRICS.keep_awake_active,
            &[labels[0], "internal_keep_awake"],
        );
    }

    pub(crate) fn set_shutdown_tasks_active(&self, count: usize) {
        let labels = self.actor_labels();
        let mut state = self.inner.state.lock();
        set_aggregated_gauge(
            &mut state.shutdown_tasks_active,
            usize_to_i64(count),
            &METRICS.shutdown_tasks_active,
            &labels,
        );
    }

    pub(crate) fn observe_shutdown_wait(
        &self,
        reason: ShutdownKind,
        duration: Duration,
    ) {
        let labels = self.actor_labels();
        METRICS
            .shutdown_wait_seconds
            .with_label_values(&[labels[0], reason.as_metric_label()])
            .observe(duration.as_secs_f64());
    }

    pub(crate) fn inc_shutdown_timeout(&self, reason: ShutdownKind) {
        let labels = self.actor_labels();
        METRICS
            .shutdown_timeout_total
            .with_label_values(&[labels[0], reason.as_metric_label()])
            .inc();
    }

    pub(crate) fn inc_state_mutation(
        &self,
        reason: StateMutationReason,
    ) {
        let labels = self.actor_labels();
        METRICS
            .state_mutation_total
            .with_label_values(&[labels[0], reason.as_metric_label()])
            .inc();
    }

    pub(crate) fn inc_direct_subsystem_shutdown_warning(
        &self,
        subsystem: &str,
        operation: &str,
    ) {
        let labels = self.actor_labels();
        METRICS
            .direct_subsystem_shutdown_warning_total
            .with_label_values(&[labels[0], subsystem, operation])
            .inc();
    }

    pub(crate) fn record_actor_stopped(&self) {
        self.inner.record_actor_stopped();
    }
}

impl Drop for ActorMetricInner {
    fn drop(&mut self) {
        self.record_actor_stopped();
    }
}

impl ActorMetricInner {
    fn record_actor_stopped(&self) {
        let Some(active) = self.active.lock().take() else {
            return;
        };
        drop(active);

        self.clear_aggregated_gauges();
        let metrics = &*METRICS;
        metrics
            .actor_stopped_total
            .with_label_values(&self.labels.as_label_values())
            .inc();
    }

    fn clear_aggregated_gauges(&self) {
        let labels = self.labels.as_label_values();
        let mut state = self.state.lock();
        set_aggregated_gauge(
            &mut state.queue_depth,
            0,
            &METRICS.queue_depth,
            &labels,
        );
        set_aggregated_gauge(
            &mut state.active_connections,
            0,
            &METRICS.active_connections,
            &labels,
        );
        set_aggregated_gauge(
            &mut state.lifecycle_inbox_depth,
            0,
            &METRICS.inbox_depth,
            &[labels[0], "lifecycle"],
        );
        set_aggregated_gauge(
            &mut state.dispatch_inbox_depth,
            0,
            &METRICS.inbox_depth,
            &[labels[0], "dispatch"],
        );
        set_aggregated_gauge(
            &mut state.lifecycle_event_inbox_depth,
            0,
            &METRICS.inbox_depth,
            &[labels[0], "lifecycle_event"],
        );
        for (kind, current) in state.user_tasks_active.iter_mut() {
            set_aggregated_gauge(
                current,
                0,
                &METRICS.user_tasks_active,
                &[labels[0], *kind],
            );
        }
        set_aggregated_gauge(
            &mut state.http_requests_active,
            0,
            &METRICS.http_requests_active,
            &labels,
        );
        set_aggregated_gauge(
            &mut state.keep_awake_active,
            0,
            &METRICS.keep_awake_active,
            &[labels[0], "keep_awake"],
        );
        set_aggregated_gauge(
            &mut state.internal_keep_awake_active,
            0,
            &METRICS.keep_awake_active,
            &[labels[0], "internal_keep_awake"],
        );
        set_aggregated_gauge(
            &mut state.shutdown_tasks_active,
            0,
            &METRICS.shutdown_tasks_active,
            &labels,
        );
    }
}

impl Default for ActorMetrics {
    fn default() -> Self {
        Self::new("")
    }
}

impl fmt::Debug for ActorMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActorMetrics").finish()
    }
}

fn set_aggregated_gauge(
    current: &mut i64,
    next: i64,
    gauge: &IntGaugeVec,
    labels: &[&str],
) {
    let delta = next.saturating_sub(*current);
    if delta != 0 {
        gauge.with_label_values(labels).add(delta);
        *current = next;
    }
}

impl StartupTimer {
    pub(crate) fn set_is_new(&mut self, is_new: bool) {
        self.is_new = Some(is_new);
        self.metrics.set_startup_is_new(is_new);
    }

    pub(crate) fn finish_success(mut self) -> Duration {
        let duration = self.started_at.elapsed();
        self.metrics.finish_startup();
        self.metrics.observe_startup_phase(
            startup_phase::StartupPhase::Total,
            self.is_new,
            "success",
            duration,
        );
        self.finished = true;
        duration
    }
}

impl Drop for StartupTimer {
    fn drop(&mut self) {
        if self.finished {
            return;
        }

        self.metrics.observe_startup_phase(
            startup_phase::StartupPhase::Total,
            self.is_new,
            "error",
            self.started_at.elapsed(),
        );
    }
}

fn optional_is_new_label(is_new: Option<bool>) -> &'static str {
    match is_new {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

fn startup_duration_buckets() -> Vec<f64> {
    vec![
        0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
        2.5, 5.0, 10.0, 30.0, 60.0,
    ]
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl ActorMetricLabels {
    fn as_label_values(&self) -> [&str; 1] {
        [self.actor_name.as_str()]
    }
}

fn register_metric<M>(registry: &Registry, metric: M)
where
    M: crate::metrics::prometheus::core::Collector
        + Clone
        + Send
        + Sync
        + 'static,
{
    if let Err(error) = registry.register(Box::new(metric)) {
        tracing::warn!(
            ?error,
            "actor metric registration failed, using existing \
             collector"
        );
    }
}

// Test shim keeps moved tests in crate-root tests/ with
// private-module access.
