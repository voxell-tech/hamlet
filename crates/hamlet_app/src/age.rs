//! How long ago something happened, as a person reads it.

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

/// Days from which an age is given as the date it began.
const DATED: u64 = 7;

/// How long ago a moment was: minutes, hours, days, then its date.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Age {
    Now,
    Minutes(u64),
    Hours(u64),
    Days(u64),
    /// The day it began, and whether that is in the year of now.
    On(Date, bool),
}

impl Age {
    /// The age of `then` at `now`, both in milliseconds since 1970.
    pub(crate) fn of(now: u64, then: u64) -> Self {
        let seconds = now.saturating_sub(then) / 1000;
        match seconds {
            0..60 => Self::Now,
            60..3600 => Self::Minutes(seconds / 60),
            3600..86_400 => Self::Hours(seconds / 3600),
            _ if seconds / 86_400 < DATED => {
                Self::Days(seconds / 86_400)
            }
            _ => {
                let day = |ms: u64| {
                    Timestamp::from_millisecond(ms as i64)
                        .unwrap_or_default()
                        .to_zoned(TimeZone::system())
                        .date()
                };
                let (began, today) = (day(then), day(now));
                Self::On(began, began.year() == today.year())
            }
        }
    }

    /// For a column: `now`, `5m`, `3h`, `2d`, `4 Mar`.
    pub(crate) fn short(self) -> String {
        match self {
            Self::Now => "now".into(),
            Self::Minutes(n) => format!("{n}m"),
            Self::Hours(n) => format!("{n}h"),
            Self::Days(n) => format!("{n}d"),
            Self::On(day, true) => day.strftime("%-d %b").to_string(),
            Self::On(day, false) => {
                day.strftime("%-d %b %Y").to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_age_past_a_week_is_a_date_not_a_count() {
        let now = 1_750_000_000_000;
        let day = 86_400_000;
        assert_eq!(Age::of(now, now - 6 * day), Age::Days(6));
        assert!(matches!(
            Age::of(now, now - 7 * day),
            Age::On(_, true)
        ));
        assert!(matches!(Age::of(now, 1000), Age::On(_, false)));
        assert_eq!(Age::of(now, now + day), Age::Now);
    }
}
