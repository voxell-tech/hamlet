//! An agent's brief on the running of its hamlet (H17): the late
//! decisions, each with its reason, and the ones that touched the
//! agent itself.

use hamlet_home::limits::Mayor;
use hamlet_model::log;
use serde_json::{Value, json};

use crate::hamlet::{Hamlet, MAYOR};
use crate::record::text;
use crate::words::clip;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// The most decisions it is told of, and the most of its own road
/// requests that were refused.
const SHOWN: usize = 6;

/// The most of a reason that is shown, in characters.
const REASON: usize = 160;

/// Notes that the mayor did not build the road `from` asked for to
/// `to`, with what it said.
pub fn refuse_road(
    hamlet: &Hamlet,
    from: &str,
    to: &str,
    reason: &str,
) -> anyhow::Result<()> {
    hamlet.append(json!({
        "v": 1,
        "type": "road_refused",
        "ts": log::now_millis(),
        "from": from,
        "to": to,
        "reason": clip(reason, REASON),
    }))
}

/// The decider, as an agent is told it.
fn who(by: &str) -> &'static str {
    if by.is_empty() || by == MAYOR {
        "The mayor"
    } else if by.starts_with("vote:") {
        "A vote"
    } else {
        "A chief"
    }
}

/// The brief of the agent `name` on the running of its hamlet, as a
/// part of its prompt. Empty when nothing was decided lately.
pub fn note(hamlet: &Hamlet, name: &str) -> String {
    // The reach of the tally back in time, which is the chief's to
    // set.
    let days = hamlet.limits().map_or_else(
        |_| Mayor::default().told_days,
        |l| l.mayor.told_days,
    );
    told(&hamlet.events(), name, log::now_millis(), days)
}

fn told(events: &[Value], name: &str, now: u64, days: u64) -> String {
    let since = now.saturating_sub(days.saturating_mul(DAY_MS));
    let reason = |e: &Value| match clip(&text(e, "reason"), REASON) {
        reason if reason.is_empty() => "no reason given".to_string(),
        reason => reason,
    };
    let mut decided = Vec::new();
    let mut mine = Vec::new();
    for e in events {
        let late = e["ts"].as_u64().unwrap_or(0) >= since;
        let by = who(&text(e, "by"));
        match e["type"].as_str() {
            Some("road_refused") if late => {
                if text(e, "from") == name {
                    mine.push(format!(
                        "The mayor did not build the road you asked \
                         for to `{}`: {}",
                        text(e, "to"),
                        reason(e)
                    ));
                }
            }
            Some("agent_created") if late => decided.push(format!(
                "The mayor created `{}`: {}",
                text(e, "name"),
                reason(e)
            )),
            Some("agent_removed") if late => decided.push(format!(
                "{by} removed `{}`: {}",
                text(e, "name"),
                reason(e)
            )),
            Some("agents_merged") if late => decided.push(format!(
                "{by} merged `{}` into `{}`: {}",
                text(e, "gone"),
                text(e, "kept"),
                reason(e)
            )),
            Some("road_built") if late => decided.push(format!(
                "{by} built a road between `{}` and `{}`: {}",
                text(e, "a"),
                text(e, "b"),
                reason(e)
            )),
            Some("mayor_changed") if late => decided.push(format!(
                "{by} made `{}` mayor in place of `{}`: {}",
                text(e, "to"),
                text(e, "from"),
                reason(e)
            )),
            _ => {}
        }
    }
    if decided.is_empty() && mine.is_empty() {
        return String::new();
    }
    let mut note = String::from(
        "\n# How your hamlet was run lately\nFrom the hamlet's \
         record, each with the reason that was given. These are \
         what happened, not a judgement.\n",
    );
    let latest = |lines: &[String]| {
        lines
            .iter()
            .rev()
            .take(SHOWN)
            .map(|line| format!("- {line}\n"))
            .collect::<String>()
    };
    if !mine.is_empty() {
        note.push_str("What touched you:\n");
        note.push_str(&latest(&mine));
    }
    if !decided.is_empty() {
        note.push_str("What was decided, latest first:\n");
        note.push_str(&latest(&decided));
    }
    note
}
