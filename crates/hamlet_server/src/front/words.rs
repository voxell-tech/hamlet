//! The words a chief reads, written from the realm's world and the
//! actors' answers.

use std::fmt::Display;

use hamlet_protocol::client::{self as protocol, Choice, WaitDetail};

use crate::oversight::{self, Ruling};
use crate::spend;

/// Who did what no agent or chief did.
pub(crate) const REALM: &str = "the realm";

/// The mode of a hamlet that works on no folder.
pub(crate) const NO_FOLDER: &str =
    "Hires work in their lodges; no folder is chosen.";

/// A day's budget period.
pub(crate) const TODAY: &str = "today";

/// The answer to `/hand`, `/hamlet` and `/agent`.
pub(crate) const CHOOSE: &str =
    "Choose who you talk to in your client.";

/// A command that acts in a hamlet, said where there is none.
pub(crate) const NO_HAMLET: &str =
    "Say this to a hamlet's mayor or one of its hires.";

pub(crate) const NOTHING_WAITS: &str = "Nothing waits for you.";

pub(crate) const NO_SESSIONS: &str = "No sessions yet.";

pub(crate) const CARRYING_ON: &str = "Carrying on.";

pub(crate) const NO_GRANT: &str = "No agent holds a lasting grant.";

pub(crate) const NO_HAMLET_YET: &str =
    "No hamlet yet. /create-hamlet makes one.";

pub(crate) const ROAD_LAID: &str = "The road is laid.";

pub(crate) const ROAD_TAKEN_UP: &str = "The road is taken up.";

pub(crate) const BUDGET_RESET: &str =
    "The budget's period starts again from now.";

pub(crate) const CHARTER_SAVED: &str = "The charter is saved.";

pub(crate) const CATALOGUE_SAVED: &str = "Saved.";

pub(crate) const FAUX_MODELS: &str =
    "Every agent runs the faux model here.";

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

/// The answer to a command no row of the table names.
pub(crate) fn no_command(name: &str) -> String {
    format!(
        "There is no command /{name}; the client lists the commands."
    )
}

pub(crate) fn began(number: u32) -> String {
    format!("Session {number} began.")
}

/// The sessions of a chat, one line each.
pub(crate) fn sessions(numbers: impl Iterator<Item = u32>) -> String {
    let list = numbers
        .map(|number| format!("- session {number}"))
        .collect::<Vec<_>>()
        .join("\n");
    if list.is_empty() {
        NO_SESSIONS.into()
    } else {
        list
    }
}

pub(crate) fn back(number: u32) -> String {
    format!("Back in session {number}.")
}

pub(crate) fn stopped(turns: u32) -> String {
    format!("Stopped {turns} running turns.")
}

/// The answer to a stop a client asks for, which says when nothing
/// was running.
pub(crate) fn stopped_or_nothing(turns: u32) -> String {
    if turns == 0 {
        "Nothing was running.".to_owned()
    } else {
        stopped(turns)
    }
}

pub(crate) fn told(name: &str) -> String {
    format!("Told {name}.")
}

/// The answer to `/reflect`: asked now, or reflecting already.
pub(crate) fn reflecting(name: &str, asked: bool) -> String {
    if asked {
        format!("Asked {name} to reflect.")
    } else {
        format!("{name} is reflecting already.")
    }
}

pub(crate) fn worker_started(worker: u32) -> String {
    format!(
        "Started worker#{worker}; open it to watch it and read its \
         answer."
    )
}

/// One line for `wait`, asked by `by`, quoting at most 80 characters
/// of its subject.
pub(crate) fn wait_line(wait: &protocol::Wait, by: &str) -> String {
    let subject = oversight::clip(&wait.subject, 80);
    match &wait.detail {
        WaitDetail::Approval { says, .. } => {
            format!("{by} {says} `{subject}`")
        }
        WaitDetail::Objection {} => {
            format!("{by} objects: {subject}")
        }
        WaitDetail::Question { .. } => {
            format!("{by} asks: {subject}")
        }
        WaitDetail::Fault {} => format!("{by} failed: {subject}"),
        WaitDetail::Newcomer { .. } => {
            format!("{subject}, whom {by} made, waits at the harbor")
        }
    }
}

/// The open waits, each with the agent that asked and the answers it
/// takes.
pub(crate) fn queue(waits: &[(protocol::Wait, String)]) -> String {
    let lines = waits
        .iter()
        .map(|(wait, by)| {
            let answers = wait
                .answers
                .iter()
                .map(|choice| format!("/{}", Ruling::of(choice)))
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "- #{} {}\n  answer with: {answers}",
                wait.id,
                wait_line(wait, by)
            )
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        NOTHING_WAITS.into()
    } else {
        lines.join("\n")
    }
}

pub(crate) fn answered(choice: &Choice, line: &str) -> String {
    format!("Answered /{}: {line}", Ruling::of(choice))
}

/// The lasting grants, each as the agent that holds it and its
/// permission.
pub(crate) fn grants(
    held: impl Iterator<Item = (String, impl Display)>,
) -> String {
    let lines = held
        .map(|(name, permission)| {
            format!("- {name} may use {permission}")
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        NO_GRANT.into()
    } else {
        lines.join("\n")
    }
}

pub(crate) fn revoked(name: &str, word: &str) -> String {
    format!("{name} may no longer use {word}.")
}

pub(crate) fn holds_no_grant(name: &str, word: &str) -> String {
    format!("{name} holds no grant of {word}.")
}

/// The living hamlets, each as its name and id.
pub(crate) fn hamlets<'a>(
    living: impl Iterator<Item = (&'a str, &'a str)>,
) -> String {
    let list = living
        .map(|(name, id)| format!("- {name} (`{id}`)"))
        .collect::<Vec<_>>()
        .join("\n");
    if list.is_empty() {
        NO_HAMLET_YET.into()
    } else {
        list
    }
}

pub(crate) fn founded(
    name: &str,
    hamlet: &str,
    mayor: &str,
) -> String {
    format!(
        "The hamlet {name} (`{hamlet}`) is created under its mayor \
         `{mayor}`."
    )
}

/// The purpose of a hamlet's mayor the chief gave none.
pub(crate) fn mayor_of(name: &str) -> String {
    format!("Mayor of {name}.")
}

pub(crate) fn hamlet_named(hamlet: &str, name: &str) -> String {
    format!("The hamlet `{hamlet}` is named {name}.")
}

pub(crate) fn hamlet_set_down(
    hamlet: &str,
    x: f64,
    z: f64,
) -> String {
    format!("The hamlet `{hamlet}` is set down at {x}, {z}.")
}

pub(crate) fn tier(hamlet: &str, most: u32) -> String {
    format!("The hamlet `{hamlet}` holds at most {most} hires.")
}

pub(crate) fn seated(hire: &str, hamlet: &str) -> String {
    format!("`{hire}` holds the office of `{hamlet}`.")
}

pub(crate) fn hired(hire: &str) -> String {
    format!("`{hire}` is hired.")
}

pub(crate) fn ended(hire: &str) -> String {
    format!("`{hire}` was ended.")
}

pub(crate) fn runs(hire: &str, model: Option<&str>) -> String {
    format!("`{hire}` runs {}.", model.unwrap_or("its default model"))
}

pub(crate) fn fast(hire: &str, value: &str) -> String {
    format!("`{hire}` asks for the fast tier: {value}.")
}

pub(crate) fn thinks(
    hire: &str,
    effort: Option<impl Display>,
) -> String {
    match effort {
        Some(effort) => format!("`{hire}` thinks at {effort}."),
        None => format!(
            "`{hire}` thinks as hard as the realm's setting says."
        ),
    }
}

pub(crate) fn nickname(hire: &str, given: Option<&str>) -> String {
    match given {
        Some(nickname) => {
            format!("`{hire}` is called {nickname} now.")
        }
        None => format!("`{hire}` goes by its name again."),
    }
}

pub(crate) fn tools_changed(hire: &str) -> String {
    format!("`{hire}`'s tools are changed.")
}

pub(crate) fn allowance(hire: &str, micros: Option<i64>) -> String {
    match micros {
        Some(micros) => format!(
            "`{hire}` may spend {} a period.",
            spend::money(micros)
        ),
        None => format!("`{hire}` has no allowance of its own."),
    }
}

pub(crate) fn works_on(hamlet: &str, folder: Option<&str>) -> String {
    match folder {
        Some(folder) => format!("`{hamlet}` works on {folder}."),
        None => format!("`{hamlet}` works on no folder."),
    }
}

pub(crate) fn mode(hamlet: &str, mode: &str) -> String {
    format!("`{hamlet}` is in {mode} mode.")
}
