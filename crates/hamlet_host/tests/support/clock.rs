//! The clock timers use.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch, the clock timers use.
pub fn now_ms() -> i64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(now.as_millis()).unwrap()
}
