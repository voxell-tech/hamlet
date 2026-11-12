// This file was changed for Hamlet. See NOTICE in this crate.
use std::sync::LazyLock;

use anyhow::{Context, Result};
use parking_lot::Mutex;

use super::prometheus::{
    Encoder, IntGaugeVec, TextEncoder,
    register_int_gauge_vec_with_registry,
};

static HAMLET_ACTOR_INFO: LazyLock<IntGaugeVec> =
    LazyLock::new(|| {
        register_int_gauge_vec_with_registry!(
            "actor_info",
            "Static actor runtime build information.",
            &[
                "runtime",
                "version",
                "type",
                "envoy_version",
                "envoy_kind",
                "pool_name",
            ],
            *super::REGISTRY
        )
        .unwrap()
    });

static CURRENT_HAMLET_ACTOR_INFO: LazyLock<
    Mutex<Option<ActorRuntimeInfo>>,
> = LazyLock::new(|| Mutex::new(None));

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActorRuntimeInfo {
    runtime: String,
    version: String,
    runtime_type: String,
    envoy_version: String,
    envoy_kind: String,
    pool_name: String,
}

impl ActorRuntimeInfo {
    fn labels(&self) -> [&str; 6] {
        [
            &self.runtime,
            &self.version,
            &self.runtime_type,
            &self.envoy_version,
            &self.envoy_kind,
            &self.pool_name,
        ]
    }
}

pub struct RenderedMetrics {
    pub content_type: String,
    pub body: Vec<u8>,
}

pub fn runtime_type() -> &'static str {
    if std::env::var("NODE_ENV").as_deref() == Ok("production") {
        "deployed"
    } else {
        "local"
    }
}

pub fn record_actor_info(
    version: impl Into<String>,
    envoy_version: u32,
    envoy_kind: impl Into<String>,
    pool_name: impl Into<String>,
) {
    record_actor_info_inner(ActorRuntimeInfo {
        runtime: "hamlet_actor".to_owned(),
        version: version.into(),
        runtime_type: runtime_type().to_owned(),
        envoy_version: envoy_version.to_string(),
        envoy_kind: envoy_kind.into(),
        pool_name: pool_name.into(),
    });
}

pub fn render_prometheus_metrics() -> Result<RenderedMetrics> {
    ensure_actor_info_recorded();

    let encoder = TextEncoder::new();
    let metric_families = super::REGISTRY.gather();
    let mut body = Vec::new();
    encoder
        .encode(&metric_families, &mut body)
        .context("encode prometheus metrics")?;

    Ok(RenderedMetrics {
        content_type: encoder.format_type().to_owned(),
        body,
    })
}

fn ensure_actor_info_recorded() {
    if CURRENT_HAMLET_ACTOR_INFO.lock().is_some() {
        return;
    }

    // The legacy platform deployment still sets the old name.
    let envoy_version = std::env::var("HAMLET_ACTOR_NODE_VERSION")
        .or_else(|_| std::env::var("HAMLET_ACTOR_ENVOY_VERSION"))
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let pool_name = std::env::var("HAMLET_ACTOR_POOL")
        .unwrap_or_else(|_| "default".to_owned());
    record_actor_info(
        env!("CARGO_PKG_VERSION"),
        envoy_version,
        "unknown",
        pool_name,
    );
}

fn record_actor_info_inner(info: ActorRuntimeInfo) {
    let mut current = CURRENT_HAMLET_ACTOR_INFO.lock();
    if let Some(previous) = current.as_ref()
        && previous != &info
    {
        HAMLET_ACTOR_INFO
            .with_label_values(&previous.labels())
            .set(0);
    }

    HAMLET_ACTOR_INFO.with_label_values(&info.labels()).set(1);
    *current = Some(info);
}
