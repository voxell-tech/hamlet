//! Numbers and prices told shortly, as the stalls show them.

use crate::shelf::now;

/// Dollars for a million tokens, to the cent or past it for less.
pub(crate) fn dollars(price: f64) -> String {
    if price > 0.0 && price < 0.01 {
        format!("${price:.4}")
    } else {
        format!("${price:.2}")
    }
}

/// A price in and out, in a few words.
pub(crate) fn price_words(
    input: Option<f64>,
    output: Option<f64>,
) -> String {
    match input.zip(output) {
        None => "Price varies".into(),
        Some((0.0, 0.0)) => "Free".into(),
        Some((input, output)) => {
            format!("{} in, {} out", dollars(input), dollars(output))
        }
    }
}

/// A count of tokens, short: `8k`, `1M`.
pub(crate) fn tokens(count: u64) -> String {
    if count >= 1_000_000 && count.is_multiple_of(100_000) {
        let millions = count as f64 / 1e6;
        format!("{}M", (millions * 10.0).round() / 10.0)
    } else if count >= 1000 {
        format!("{}k", count / 1000)
    } else {
        count.to_string()
    }
}

/// A count, short: `950`, `57.2K`, `3.9M`.
pub(crate) fn count(n: u64) -> String {
    if n >= 999_950 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// A count in full, its thousands apart: `1,094,811`.
pub(crate) fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut grouped = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// The time since `then`, in milliseconds since 1970, in words.
pub(crate) fn ago(then: u64) -> String {
    let minutes = now().saturating_sub(then) / 60_000;
    match minutes {
        0 => "just now".into(),
        1 => "a minute ago".into(),
        2..60 => format!("{minutes} minutes ago"),
        60..120 => "an hour ago".into(),
        _ => format!("{} hours ago", minutes / 60),
    }
}
