// This file was changed for Hamlet. See NOTICE in this crate.
#[test]
fn render_prometheus_metrics_includes_actor_info() {
    hamlet_actor_core::metrics::record_actor_info(
        env!("CARGO_PKG_VERSION"),
        42,
        "serverful",
        "metrics-test-pool",
    );

    let metrics =
        hamlet_actor_core::metrics::render_prometheus_metrics()
            .expect("render metrics");
    let body = String::from_utf8(metrics.body)
        .expect("metrics should be utf-8");
    let expected = format!(
        concat!(
            "hamlet_actor_info{{envoy_kind=\"serverful\",",
            "envoy_version=\"42\",",
            "pool_name=\"metrics-test-pool\",",
            "runtime=\"hamlet_actor\",type=\"local\",",
            "version=\"{}\"}} 1",
        ),
        env!("CARGO_PKG_VERSION")
    );

    assert!(
        body.lines().any(|line| line == expected),
        "missing version metric in:\n{body}"
    );
}
