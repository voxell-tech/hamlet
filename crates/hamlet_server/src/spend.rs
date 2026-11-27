//! How the realm's spend is written (H8): money, millionths of a
//! dollar and durations.

use crate::society::limits::Micros;

/// `micros` in dollars to the cent, a half cent rounding up.
pub(crate) fn money(micros: Micros) -> String {
    if (1..10_000).contains(&micros) {
        return "<$0.01".into();
    }
    let cents = (i128::from(micros) + 5_000) / 10_000;
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// `dollars` in micros, to the nearest.
pub(crate) fn micros(dollars: f64) -> Micros {
    (dollars * 1_000_000.0).round() as i64
}

pub(crate) fn duration(ms: i64) -> String {
    let seconds = ms / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_rounds_to_the_cent() {
        assert_eq!(money(0), "$0.00");
        assert_eq!(money(5_000), "<$0.01");
        assert_eq!(money(1_234_999), "$1.23");
        assert_eq!(money(1_235_000), "$1.24");
    }
}
