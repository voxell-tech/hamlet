//! The words a chief reads that today's server writes inline from
//! its own state, written here from the realm's world.

use hamlet_protocol::client as protocol;

use crate::spend;

/// Who did what no agent or chief did.
pub(crate) const REALM: &str = "the realm";

/// The mode of a hamlet that works on no folder.
pub(crate) const NO_FOLDER: &str =
    "Hires work in their lodges; no folder is chosen.";

/// A day's budget period.
pub(crate) const TODAY: &str = "today";

/// One line of a payer's spend in its period: what it spent of the
/// cap it is nearest, or what it spent when it has none. `whose` is
/// `Hamlet` or `Realm`.
pub(crate) fn budget(
    whose: &str,
    spent: &protocol::Amount,
    caps: &protocol::ReportCaps,
) -> String {
    let caps = [
        (spent.micros, caps.micros, false),
        (spent.ms, caps.ms, true),
    ];
    let show = |amount: i64, time: bool| {
        if time {
            spend::duration(amount)
        } else {
            spend::money(amount)
        }
    };
    let nearest = caps
        .iter()
        .filter_map(|(spent, cap, time)| {
            cap.map(|cap| (*spent, cap, *time))
        })
        .max_by(|a, b| {
            (a.0 as f64 / a.1.max(1) as f64)
                .total_cmp(&(b.0 as f64 / b.1.max(1) as f64))
        });
    format!(
        "{whose} {TODAY}: {}",
        nearest
            .map(|(spent, cap, time)| {
                format!(
                    "{} of {}",
                    show(spent, time),
                    show(cap, time)
                )
            })
            .unwrap_or_else(|| spend::money(spent.micros))
    )
}
