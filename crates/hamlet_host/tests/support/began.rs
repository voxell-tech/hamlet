//! A test's wait for work it set going to report that it began.

use std::time::Duration;

/// Awaits `report`, the sign that `what` began, for ten seconds at
/// most, and fails the test past that: work that began reports it
/// well within a second, so only a defect reaches the bound.
pub async fn began<T>(
    what: &str,
    report: impl Future<Output = T>,
) -> T {
    tokio::time::timeout(Duration::from_secs(10), report)
        .await
        .unwrap_or_else(|_| panic!("{what} did not begin"))
}
