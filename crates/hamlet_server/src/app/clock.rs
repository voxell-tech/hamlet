//! The server's clock. A time is milliseconds since the Unix epoch.

use anyhow::{Context, Result};
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};

/// Now, in milliseconds since the Unix epoch.
pub(crate) fn now() -> i64 {
    Timestamp::now().as_millisecond()
}

/// `count` minutes in milliseconds, saturating.
pub(crate) fn minutes(count: i64) -> i64 {
    count.saturating_mul(60_000)
}

/// `count` days in milliseconds, saturating.
pub(crate) fn days(count: i64) -> i64 {
    count.saturating_mul(86_400_000)
}

/// The local wall-clock time at `at`.
pub(crate) fn local(at: i64) -> Result<Zoned> {
    let at =
        Timestamp::from_millisecond(at).context("the local time")?;
    Ok(at.to_zoned(TimeZone::system()))
}
