//! The server's clock. A time is milliseconds since the Unix epoch.

/// Now, in milliseconds since the Unix epoch.
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
