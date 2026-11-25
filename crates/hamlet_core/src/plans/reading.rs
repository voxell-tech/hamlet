//! A provider's limits as a chief reads them: how much of each window
//! is left and how that stands, when it resets, where the pace so far
//! takes it, and a sentence when something needs the chief.

use hamlet_protocol::client as protocol;
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Where ChatGPT shows a plan's use, and Hamlet's share of it.
pub const CHATGPT_USAGE: &str = "https://chatgpt.com/settings/usage";

/// How far into a window the pace is read from, and how much must be
/// used: before then a few early calls say little of the rest.
const PACE_FROM: f64 = 0.02;
const PACE_USED: f64 = 5.0;

/// Minutes after which what a provider told may have changed.
const STALE_MINUTES: i64 = 10;

/// The percent left above which a window has plenty, and above which
/// it has some.
const PLENTY: f64 = 50.0;
const SOME: f64 = 25.0;

/// How a plan, or one of its windows, stands, from least to most in
/// need of the chief.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mood {
    /// The provider does not say how much is used.
    Unknown,
    /// More than half is left, and it lasts at this pace.
    Plenty,
    /// A quarter to a half is left.
    Fair,
    /// Under a quarter is left, at this pace it runs out before it
    /// resets, or the provider says a limit is near.
    Low,
    /// Used up: the provider refuses.
    Out,
}

/// One window as a row shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub name: String,
    /// How much is left, from 0 to 1, where the provider says.
    pub left: Option<f32>,
    /// Where the bar would end now at an even pace, from 0 to 1: the
    /// share of the window's time still to come.
    pub pace: Option<f32>,
    pub mood: Mood,
    /// What is left beside the bar: "73% left", "$6.80 left", or
    /// "Not reported".
    pub amount: String,
    /// What is used, for the amount's tooltip: "27% used", "$3.20 of
    /// $10 used".
    pub used: Option<String>,
    /// When it resets: "Resets in 2 h 26 m (16:40)".
    pub reset: Option<String>,
    /// Where the pace so far takes it: "At this pace, about 60% is
    /// left when it resets".
    pub forecast: Option<String>,
    /// When it resets, in milliseconds since the epoch.
    pub resets: Option<i64>,
    /// When it runs out at this pace, where that is before it
    /// resets.
    pub runs_out: Option<i64>,
}

/// A provider's limits as the chief reads them.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub mood: Mood,
    /// What needs the chief, or what stands in for windows the
    /// provider does not tell; empty while every window is fine.
    pub sentence: String,
    pub rows: Vec<Row>,
    /// The row that matters most: a used up one, else the one that
    /// runs out soonest at its pace, else the one with least left.
    pub nearest: Option<usize>,
    /// "Checked 2 min ago", or "Refused at 14:28".
    pub when: String,
    /// When it was told, in milliseconds since the epoch.
    pub checked: i64,
    /// What it told is old enough to have changed.
    pub stale: bool,
    /// A window reset since it was told, so it is read again.
    pub due: bool,
}

/// What `provider`'s `limits` say at `now`, in milliseconds since
/// the epoch, with times in `zone`.
pub fn read(
    provider: &str,
    limits: &protocol::ProviderLimits,
    now: i64,
    zone: &TimeZone,
) -> Reading {
    let rows = limits
        .windows
        .iter()
        .map(|window| {
            row(window, limits.state, limits.checked, now, zone)
        })
        .collect::<Vec<_>>();
    let nearest = nearest(&rows);
    let worst = rows.iter().map(|r| r.mood).max();
    let mood = match limits.state {
        protocol::LimitsState::Refused => Mood::Out,
        protocol::LimitsState::Warning => {
            worst.unwrap_or(Mood::Low).max(Mood::Low)
        }
        protocol::LimitsState::Unknown if rows.is_empty() => {
            Mood::Unknown
        }
        _ => worst.unwrap_or(Mood::Plenty),
    };
    let at = clock(limits.checked, now, zone);
    let when = match mood {
        Mood::Out => format!("Refused at {at}"),
        _ => format!("Checked {}", ago(now - limits.checked)),
    };
    let stale = mood != Mood::Out
        && now - limits.checked > STALE_MINUTES * 60_000;
    let due = limits.windows.iter().any(|w| {
        w.resets.is_some_and(|at| limits.checked < at && at <= now)
    });
    Reading {
        sentence: sentence(
            provider, mood, &rows, nearest, &at, now, zone,
        ),
        mood,
        rows,
        nearest,
        when,
        checked: limits.checked,
        stale,
        due,
    }
}

fn row(
    window: &protocol::LimitWindow,
    state: protocol::LimitsState,
    checked: i64,
    now: i64,
    zone: &TimeZone,
) -> Row {
    let money = window.money.as_ref();
    let used = window
        .used
        .or_else(|| {
            money
                .filter(|m| m.cap > 0.0)
                .map(|m| (m.cap - m.left.max(0.0)) / m.cap * 100.0)
        })
        .map(|u| u.clamp(0.0, 100.0));
    let resets = window.resets.filter(|&at| at > now);
    let span_ms = window
        .length
        .filter(|&length| length > 0)
        .map(|length| length as f64 * 60_000.0);
    // How far into the window `at` is, where its length and end are
    // known.
    let into = |at: i64| match (span_ms, resets) {
        (Some(span), Some(resets)) => {
            Some((1.0 - (resets - at) as f64 / span).clamp(0.0, 1.0))
        }
        _ => None,
    };
    // The pace is the use told over the time gone when it was told,
    // so it holds until the next check.
    let elapsed = into(checked.min(now));
    // What would be used by the reset at the pace so far.
    let projected = match (used, elapsed) {
        (Some(used), Some(elapsed))
            if elapsed >= PACE_FROM && used >= PACE_USED =>
        {
            Some(used / elapsed)
        }
        _ => None,
    };
    let runs_out = match (used, elapsed, span_ms, resets) {
        (Some(used), Some(elapsed), Some(span), Some(resets))
            if projected.is_some_and(|p| p > 100.0)
                && used < 100.0 =>
        {
            // At the pace so far, the rest of it lasts this long.
            let left = (100.0 - used) / used * elapsed * span;
            let at = checked.min(now) + left.round() as i64;
            (at < resets).then_some(at)
        }
        _ => None,
    };
    let out = used.is_some_and(|u| u >= 100.0)
        || (state == protocol::LimitsState::Refused
            && money.is_some_and(|m| m.left <= 0.0));
    let mood = match used {
        _ if out => Mood::Out,
        None => Mood::Unknown,
        Some(_) if runs_out.is_some() => Mood::Low,
        Some(used) if 100.0 - used > PLENTY => Mood::Plenty,
        Some(used) if 100.0 - used > SOME => Mood::Fair,
        Some(_) => Mood::Low,
    };
    let amount = match (money, used) {
        (Some(money), _) => {
            format!("{} left", dollars(money.left.max(0.0)))
        }
        (None, Some(used)) => {
            format!("{}% left", 100.0 - used.round())
        }
        (None, None) => "Not reported".into(),
    };
    let used_words = match (money, used) {
        (Some(money), _) => Some(format!(
            "{} of {} used",
            dollars((money.cap - money.left).max(0.0)),
            dollars(money.cap)
        )),
        (None, Some(used)) => Some(format!("{}% used", used.round())),
        (None, None) => None,
    };
    let forecast = match (runs_out, projected) {
        _ if out => None,
        (Some(at), _) => Some(format!(
            "At this pace, it runs out around {}",
            clock(at, now, zone)
        )),
        (None, Some(projected)) if projected < 100.0 => {
            // In steps of five, since a pace is a guess.
            let left = ((100.0 - projected) / 5.0).floor() * 5.0;
            Some(if left < 5.0 {
                "At this pace, it just lasts until it resets".into()
            } else {
                format!(
                    "At this pace, about {left}% is left when it \
                     resets"
                )
            })
        }
        _ => None,
    };
    Row {
        id: window.id.clone(),
        name: window.name.clone(),
        left: used.map(|u| ((100.0 - u) / 100.0) as f32),
        pace: into(now)
            .filter(|_| used.is_some_and(|u| u > 0.0))
            .map(|e| (1.0 - e) as f32),
        mood,
        amount,
        used: used_words,
        reset: resets
            .map(|at| format!("Resets {}", until(at, now, zone))),
        forecast,
        resets,
        runs_out,
    }
}

/// A used up row, else the one that runs out soonest at its pace,
/// else the one with least left.
fn nearest(rows: &[Row]) -> Option<usize> {
    let soonest = || {
        rows.iter()
            .enumerate()
            .filter_map(|(i, r)| r.runs_out.map(|at| (at, i)))
            .min()
            .map(|(_, i)| i)
    };
    rows.iter()
        .position(|r| r.mood == Mood::Out)
        .or_else(soonest)
        .or_else(|| {
            rows.iter()
                .enumerate()
                .filter_map(|(i, r)| r.left.map(|l| (l, i)))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, i)| i)
        })
}

fn sentence(
    provider: &str,
    mood: Mood,
    rows: &[Row],
    nearest: Option<usize>,
    checked: &str,
    now: i64,
    zone: &TimeZone,
) -> String {
    let near = nearest.and_then(|i| rows.get(i));
    match mood {
        Mood::Unknown if rows.is_empty() && provider == "chatgpt" => {
            "ChatGPT didn't share this plan's usage just now. Check \
             again, or see it on ChatGPT's usage page."
                .into()
        }
        Mood::Unknown if rows.is_empty() => {
            "This provider doesn't report its usage limits.".into()
        }
        Mood::Out => match near.filter(|row| row.mood == Mood::Out) {
            Some(row) => match row.resets {
                Some(at) => format!(
                    "{} reached. It resets {}.",
                    capital(&limit(&row.name)),
                    until(at, now, zone)
                ),
                None => {
                    format!("{} reached.", capital(&limit(&row.name)))
                }
            },
            // ChatGPT's refusal names no limit; the read after it
            // does.
            None if provider == "chatgpt" => format!(
                "ChatGPT stopped answering at {checked}: the plan's \
                 usage limit, or the share of it you allow Hamlet, \
                 is reached."
            ),
            None => {
                format!("A usage limit was reached at {checked}.")
            }
        },
        Mood::Low => match near {
            Some(row) => match (row.runs_out, row.resets) {
                (Some(at), Some(resets)) => format!(
                    "At this pace, the {} runs out around {}, {} \
                     before it resets.",
                    limit(&row.name),
                    clock(at, now, zone),
                    span(resets - at)
                ),
                (Some(at), None) => format!(
                    "At this pace, the {} runs out around {}.",
                    limit(&row.name),
                    clock(at, now, zone)
                ),
                (None, Some(resets)) if row.left.is_some() => {
                    format!(
                        "Only {} of the {} is left. It resets {}.",
                        row.amount.trim_end_matches(" left"),
                        limit(&row.name),
                        until(resets, now, zone)
                    )
                }
                _ => "Close to a usage limit.".into(),
            },
            None => "Close to a usage limit.".into(),
        },
        _ if rows.is_empty() && provider == "openrouter" => {
            "This key has no spending limit.".into()
        }
        _ if rows.is_empty() => "No usage limits reported.".into(),
        _ => String::new(),
    }
}

/// A window's name inside a sentence: "weekly Opus" for "Weekly ·
/// Opus".
fn prose(name: &str) -> String {
    let mut chars = name.chars();
    let first = chars.next().map(|c| c.to_lowercase().to_string());
    format!("{}{}", first.unwrap_or_default(), chars.as_str())
        .replace(" · ", " ")
}

/// A window as a sentence names it: "weekly Fable limit" for "Weekly
/// · Fable", and "monthly cap" for "Monthly cap".
fn limit(name: &str) -> String {
    let named = prose(name);
    if named.ends_with(" cap") {
        named
    } else {
        format!("{named} limit")
    }
}

/// `words` with its first letter upper case, to start a sentence.
fn capital(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// When `at` comes, nearest first: "in 2 h 26 m (16:40)" within a
/// day, else "Thu 20:00 (in 4 d 7 h)".
pub fn until(at: i64, now: i64, zone: &TimeZone) -> String {
    let (when, wait) = (clock(at, now, zone), span(at - now));
    if at - now < 86_400_000 {
        format!("in {wait} ({when})")
    } else {
        format!("{when} (in {wait})")
    }
}

/// `at` as a clock time in `zone`: "16:40" today, "Fri 09:10" within
/// the week, "4 Mar 09:10" further.
pub fn clock(at: i64, now: i64, zone: &TimeZone) -> String {
    let local = |ms: i64| {
        Timestamp::from_millisecond(ms)
            .unwrap_or_default()
            .to_zoned(zone.clone())
    };
    let (then, today) = (local(at), local(now));
    let days = (then.date() - today.date()).get_days().abs();
    if then.date() == today.date() {
        then.strftime("%H:%M").to_string()
    } else if days < 7 {
        then.strftime("%a %H:%M").to_string()
    } else {
        then.strftime("%-d %b %H:%M").to_string()
    }
}

/// A span of time as a person says it: "2 h 26 m", "2 d 8 h",
/// "under a minute".
pub fn span(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    let (days, hours, minutes) =
        (minutes / 1_440, minutes / 60 % 24, minutes % 60);
    match (days, hours, minutes) {
        (0, 0, 0) => "under a minute".into(),
        (0, 0, m) => format!("{m} m"),
        (0, h, 0) => format!("{h} h"),
        (0, h, m) => format!("{h} h {m} m"),
        (d, 0, _) => format!("{d} d"),
        (d, h, _) => format!("{d} d {h} h"),
    }
}

/// How long ago `ms` was: "just now", "2 min ago".
pub fn ago(ms: i64) -> String {
    match ms / 60_000 {
        ..1 => "just now".into(),
        m if m < 60 => format!("{m} min ago"),
        m => format!("{} ago", span(m * 60_000)),
    }
}

fn dollars(amount: f64) -> String {
    if amount.fract() == 0.0 {
        format!("${amount:.0}")
    } else {
        format!("${amount:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000;

    fn limits(
        state: protocol::LimitsState,
        windows: Vec<protocol::LimitWindow>,
    ) -> protocol::ProviderLimits {
        protocol::ProviderLimits {
            checked: 0,
            plan: None,
            state,
            windows,
        }
    }

    /// A five-hour window `used` percent used, `left` hours before it
    /// resets.
    fn five_hours(
        used: f64,
        left: i64,
        now: i64,
    ) -> protocol::LimitWindow {
        protocol::LimitWindow {
            id: "five_hour".into(),
            length: Some(300),
            money: None,
            name: "Session".into(),
            resets: Some(now + left * HOUR),
            used: Some(used),
        }
    }

    fn read_ok(
        windows: Vec<protocol::LimitWindow>,
        now: i64,
    ) -> Reading {
        read(
            "claude",
            &protocol::ProviderLimits {
                checked: now,
                ..limits(protocol::LimitsState::Ok, windows)
            },
            now,
            &TimeZone::UTC,
        )
    }

    #[test]
    fn a_window_used_faster_than_it_lasts_runs_out_before_it_resets()
    {
        let now = 10 * HOUR;
        // One hour in, 40% used: the rest lasts an hour and a half,
        // and the window resets in four. Four hours in, 10% used: it
        // lasts, with about 85% left.
        let read = read_ok(
            vec![five_hours(40.0, 4, now), five_hours(10.0, 1, now)],
            now,
        );
        assert_eq!(read.mood, Mood::Low);
        assert_eq!(read.nearest, Some(0));
        let row = &read.rows[0];
        assert_eq!(row.amount, "60% left");
        assert_eq!(row.runs_out, Some(now + 3 * HOUR / 2));
        assert_eq!(row.pace, Some(0.8));
        assert_eq!(
            read.sentence,
            "At this pace, the session limit runs out around 11:30, \
             2 h 30 m before it resets."
        );
        assert_eq!(read.rows[1].mood, Mood::Plenty);
        assert_eq!(
            read.rows[1].forecast.as_deref(),
            Some("At this pace, about 85% is left when it resets")
        );
    }

    #[test]
    fn what_is_left_colours_a_window_and_a_fine_plan_says_nothing() {
        let now = 10 * HOUR;
        // A minute in, so the pace says nothing yet.
        let window = |used: f64| protocol::LimitWindow {
            resets: Some(now + 300 * 60_000 - 60_000),
            ..five_hours(used, 0, now)
        };
        let moods = [3.0, 60.0, 80.0].map(|used| {
            read_ok(vec![window(used)], now).rows[0].mood
        });
        assert_eq!(moods, [Mood::Plenty, Mood::Fair, Mood::Low]);
        let fine = read_ok(vec![window(3.0)], now);
        assert_eq!(fine.rows[0].runs_out, None);
        assert_eq!(fine.sentence, "");
    }

    #[test]
    fn a_used_up_window_is_the_nearest_and_says_when_it_resets() {
        let (now, zone) = (10 * HOUR, TimeZone::UTC);
        let read = read(
            "claude",
            &limits(
                protocol::LimitsState::Refused,
                vec![
                    five_hours(30.0, 3, now),
                    five_hours(100.0, 2, now),
                ],
            ),
            now,
            &zone,
        );
        assert_eq!(read.mood, Mood::Out);
        assert_eq!(read.nearest, Some(1));
        assert_eq!(
            read.sentence,
            "Session limit reached. It resets in 2 h (12:00)."
        );
    }

    #[test]
    fn spans_and_clock_times_read_as_a_person_says_them() {
        let zone = TimeZone::UTC;
        assert_eq!(span(30_000), "under a minute");
        assert_eq!(span(146 * 60_000), "2 h 26 m");
        assert_eq!(span(56 * HOUR + 59 * 60_000), "2 d 8 h");
        // 1 January 1970 was a Thursday.
        assert_eq!(clock(16 * HOUR + 2_400_000, 0, &zone), "16:40");
        assert_eq!(clock(33 * HOUR, 0, &zone), "Fri 09:00");
        assert_eq!(clock(240 * HOUR, 0, &zone), "11 Jan 00:00");
        assert_eq!(
            until(33 * HOUR, 0, &zone),
            "Fri 09:00 (in 1 d 9 h)"
        );
    }
}
