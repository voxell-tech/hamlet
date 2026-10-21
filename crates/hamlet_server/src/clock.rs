//! The server's clock. A time is milliseconds since the Unix epoch.

/// Now, in milliseconds since the Unix epoch.
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// `count` minutes in milliseconds, saturating at the bounds of an
/// `i64`.
pub(crate) fn minutes(count: i64) -> i64 {
    count.saturating_mul(60_000)
}

/// `count` days in milliseconds, saturating at the bounds of an
/// `i64`.
pub(crate) fn days(count: i64) -> i64 {
    count.saturating_mul(86_400_000)
}
