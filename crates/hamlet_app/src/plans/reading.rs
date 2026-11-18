//! A provider's limits as a chief reads them: each window's use, its
//! reset as a clock time, the pace it is used at, and one sentence
//! that says whether the agents can keep working, and until when.

use hamlet_protocol::client as protocol;
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Where ChatGPT shows a plan's use, and Hamlet's share of it.
pub(crate) const CHATGPT_USAGE: &str =
    "https://chatgpt.com/settings/usage";

/// How far into a window the pace is read from, and how much must be
/// used: before then a few early calls say little of the rest.
const PACE_FROM: f64 = 0.02;
const PACE_USED: f64 = 5.0;

/// Minutes after which what a provider told may have changed.
const STALE_MINUTES: i64 = 10;

/// How a plan, or one of its windows, stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mood {
    /// It answers, and lasts until it resets at the pace it is used.
    Fine,
    /// At the pace it is used, it runs out before it resets, or the
    /// provider says a limit is near.
    Short,
    /// The provider refused for a limit.
    Refused,
    /// The provider tells nothing of its limits.
    Unknown,
}

/// One window as a row shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub id: String,
    pub name: String,
    /// How much is used, from 0 to 1, where the provider says.
    pub used: Option<f32>,
    /// Where even use would stand now, from 0 to 1: past it, the
    /// window is used faster than it lasts.
    pub pace: Option<f32>,
    pub mood: Mood,
    /// What is used, as the bar fills: "46% used", or "$25.50 of
    /// $100".
    pub amount: String,
    /// What is left of a cap in money: "$74.50 left".
    pub left: Option<String>,
    /// When it resets: "resets 16:40 · in 2 h 26 m".
    pub reset: Option<String>,
    /// When it resets, in milliseconds since the epoch.
    pub resets: Option<i64>,
    /// When it runs out at this pace, where that is before it
    /// resets.
    pub runs_out: Option<i64>,
}

/// A provider's limits as the chief reads them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reading {
    pub mood: Mood,
    /// The answer to "can the agents keep working, and until when?".
    pub sentence: String,
    pub rows: Vec<Row>,
    /// The row that matters most: the one that runs out soonest at
    /// its pace, else the most used.
    pub nearest: Option<usize>,
    /// "checked 2 min ago", or "refused at 14:28".
    pub when: String,
    /// When it was told, in milliseconds since the epoch.
    pub checked: i64,
    /// What it told is old enough to have changed.
    pub stale: bool,
}

/// What `provider`'s `limits` say at `now`, in milliseconds since
/// the epoch, with times in `zone`.
pub(crate) fn read(
    provider: &str,
    limits: &protocol::ProviderLimits,
    now: i64,
    zone: &TimeZone,
) -> Reading {
    let rows = limits
        .windows
        .iter()
        .map(|window| row(window, limits.state, now, zone))
        .collect::<Vec<_>>();
    let nearest = nearest(&rows);
    let mood = match limits.state {
        protocol::LimitsState::Refused => Mood::Refused,
        protocol::LimitsState::Unknown if rows.is_empty() => {
            Mood::Unknown
        }
        protocol::LimitsState::Warning => Mood::Short,
        _ if rows.iter().any(|r| r.mood == Mood::Short) => {
            Mood::Short
        }
        _ => Mood::Fine,
    };
    let at = clock(limits.checked, now, zone);
    let when = match mood {
        Mood::Refused => format!("refused at {at}"),
        _ => format!("checked {}", ago(now - limits.checked)),
    };
    let stale = mood != Mood::Refused
        && now - limits.checked > STALE_MINUTES * 60_000;
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
    }
}

fn row(
    window: &protocol::LimitWindow,
    state: protocol::LimitsState,
    now: i64,
    zone: &TimeZone,
) -> Row {
    let used = window.used.map(|u| u.clamp(0.0, 100.0));
    let resets = window.resets.filter(|&at| at > now);
    // How far into the window now is, where its length and end are
    // known.
    let elapsed = match (window.length, resets) {
        (Some(length), Some(resets)) if length > 0 => {
            let span = length as f64 * 60_000.0;
            Some((1.0 - (resets - now) as f64 / span).clamp(0.0, 1.0))
        }
        _ => None,
    };
    let runs_out = match (used, elapsed, resets) {
        (Some(used), Some(elapsed), Some(resets))
            if elapsed >= PACE_FROM
                && (PACE_USED..100.0).contains(&used) =>
        {
            let span = window.length.unwrap_or(0) as f64 * 60_000.0;
            // At the pace so far, the rest of it lasts this long.
            let left = (100.0 - used) / used * elapsed * span;
            let at = now + left.round() as i64;
            (at < resets).then_some(at)
        }
        _ => None,
    };
    let refused = used.is_some_and(|u| u >= 100.0)
        || (state == protocol::LimitsState::Refused
            && window.money.as_ref().is_some_and(|m| m.left <= 0.0));
    let mood = if refused {
        Mood::Refused
    } else if runs_out.is_some() {
        Mood::Short
    } else {
        Mood::Fine
    };
    let amount = match (&window.money, used) {
        (Some(money), _) => format!(
            "{} of {}",
            dollars((money.cap - money.left).max(0.0)),
            dollars(money.cap)
        ),
        (None, Some(used)) => format!("{}% used", used.round()),
        (None, None) => "use not told".into(),
    };
    let left = window.money.as_ref().map(|money| {
        format!("{} left", dollars(money.left.max(0.0)))
    });
    Row {
        id: window.id.clone(),
        name: window.name.clone(),
        used: used.map(|u| (u / 100.0) as f32),
        pace: elapsed
            .filter(|_| used.is_some_and(|u| u > 0.0))
            .map(|e| e as f32),
        mood,
        amount,
        left,
        reset: resets.map(|at| {
            format!(
                "resets {} · in {}",
                clock(at, now, zone),
                span(at - now)
            )
        }),
        resets,
        runs_out,
    }
}

/// The row that runs out soonest at its pace, else a refused one,
/// else the most used.
fn nearest(rows: &[Row]) -> Option<usize> {
    let soonest = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.runs_out.map(|at| (at, i)))
        .min()
        .map(|(_, i)| i);
    soonest
        .or_else(|| rows.iter().position(|r| r.mood == Mood::Refused))
        .or_else(|| {
            rows.iter()
                .enumerate()
                .filter_map(|(i, r)| r.used.map(|u| (u, i)))
                .max_by(|a, b| a.0.total_cmp(&b.0))
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
        Mood::Unknown if provider == "chatgpt" => {
            "OpenAI does not tell how much of a ChatGPT plan is \
             used. ChatGPT's usage page shows it, and Hamlet's share."
                .into()
        }
        Mood::Unknown => {
            "This provider does not tell its limits.".into()
        }
        Mood::Refused if provider == "chatgpt" => {
            "OpenAI refused the last reply: the plan, or the share \
             of it you allow Hamlet, is used up. OpenAI does not say \
             which, or when it lifts."
                .into()
        }
        Mood::Refused => match near
            .filter(|row| row.mood == Mood::Refused)
        {
            Some(row) => match &row.reset {
                Some(reset) => format!(
                    "The {} is used up. It {reset}.",
                    limit(&row.name)
                ),
                None => {
                    format!("The {} is used up.", limit(&row.name))
                }
            },
            None => format!("It refused at {checked} for a limit."),
        },
        Mood::Short => {
            match near.and_then(|r| r.runs_out.map(|at| (r, at))) {
                Some((row, at)) => {
                    let around = clock(at, now, zone);
                    match row.resets {
                        Some(resets) => format!(
                            "At this pace the {} runs out around \
                             {around}, {} before it resets.",
                            limit(&row.name),
                            span(resets - at)
                        ),
                        None => format!(
                            "At this pace the {} runs out around \
                             {around}.",
                            limit(&row.name)
                        ),
                    }
                }
                None => "The provider says a limit is near.".into(),
            }
        }
        Mood::Fine => match near {
            Some(row) => {
                let used = match &row.left {
                    Some(left) => format!("has {left}"),
                    None => format!("is {}", row.amount),
                };
                match row.resets {
                    Some(at) => format!(
                        "Fine for now: the {} {used} and resets at \
                         {}.",
                        limit(&row.name),
                        clock(at, now, zone)
                    ),
                    None => format!(
                        "Fine for now: the {} {used}.",
                        limit(&row.name)
                    ),
                }
            }
            None if provider == "openrouter" => {
                "This key has no cap of its own.".into()
            }
            None => "Nothing caps it.".into(),
        },
    }
}

/// A window's name inside a sentence: "the weekly Opus limit" for
/// "Weekly · Opus".
fn prose(name: &str) -> String {
    let mut chars = name.chars();
    let first = chars.next().map(|c| c.to_lowercase().to_string());
    format!("{}{}", first.unwrap_or_default(), chars.as_str())
        .replace(" · ", " ")
}

/// A window as a sentence names it: "the weekly Fable limit" for
/// "Weekly · Fable", and "the monthly cap" for "Monthly cap".
fn limit(name: &str) -> String {
    let named = prose(name);
    if named.ends_with(" cap") {
        named
    } else {
        format!("{named} limit")
    }
}

/// `at` as a clock time in `zone`: "16:40" today, "Fri 09:10" within
/// the week, "4 Mar 09:10" further.
pub(crate) fn clock(at: i64, now: i64, zone: &TimeZone) -> String {
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
pub(crate) fn span(ms: i64) -> String {
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
pub(crate) fn ago(ms: i64) -> String {
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

    #[test]
    fn a_window_used_faster_than_it_lasts_runs_out_before_it_resets()
    {
        let (now, zone) = (10 * HOUR, TimeZone::UTC);
        // One hour in, 40% used: the rest lasts an hour and a half,
        // and the window resets in four.
        let read = read(
            "claude",
            &limits(
                protocol::LimitsState::Ok,
                vec![
                    five_hours(40.0, 4, now),
                    five_hours(10.0, 1, now),
                ],
            ),
            now,
            &zone,
        );
        assert_eq!(read.mood, Mood::Short);
        assert_eq!(read.nearest, Some(0));
        let row = &read.rows[0];
        assert_eq!(row.runs_out, Some(now + 3 * HOUR / 2));
        assert_eq!(row.pace, Some(0.2));
        assert_eq!(
            read.sentence,
            "At this pace the session limit runs out around 11:30, \
             2 h 30 m before it resets."
        );
        // Four hours in, 10% used: it lasts.
        assert_eq!(read.rows[1].runs_out, None);
        assert_eq!(read.rows[1].mood, Mood::Fine);
    }

    #[test]
    fn a_few_early_calls_say_nothing_of_the_pace() {
        let (now, zone) = (10 * HOUR, TimeZone::UTC);
        // A minute in, 3% used.
        let window = protocol::LimitWindow {
            resets: Some(now + 300 * 60_000 - 60_000),
            ..five_hours(3.0, 0, now)
        };
        let read = read(
            "claude",
            &limits(protocol::LimitsState::Ok, vec![window]),
            now,
            &zone,
        );
        assert_eq!(read.mood, Mood::Fine);
        assert_eq!(read.rows[0].runs_out, None);
    }

    #[test]
    fn a_refused_window_is_the_nearest_and_says_when_it_resets() {
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
        assert_eq!(read.mood, Mood::Refused);
        assert_eq!(read.nearest, Some(1));
        assert_eq!(
            read.sentence,
            "The session limit is used up. It resets 12:00 · in 2 h."
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
    }
}
