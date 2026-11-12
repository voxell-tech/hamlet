// This file was changed for Hamlet. See NOTICE in this crate.
use crate::metrics::prometheus::{Encoder, TextEncoder};

pub(crate) fn render_global_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = crate::metrics::REGISTRY.gather();
    let mut encoded = Vec::new();
    encoder
        .encode(&metric_families, &mut encoded)
        .expect("encode metrics");
    String::from_utf8(encoded).expect("metrics should be utf-8")
}

pub(crate) fn metric_line_for_actor(
    line: &str,
    name: &str,
    actor_name: &str,
) -> bool {
    metric_name_matches(line, name)
        && line.contains(&format!("actor_name=\"{actor_name}\""))
}

pub(crate) fn metric_name_matches(line: &str, name: &str) -> bool {
    line.starts_with(name)
        || line
            .strip_prefix("hamlet_")
            .is_some_and(|unprefixed| unprefixed.starts_with(name))
}
