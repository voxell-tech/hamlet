//! The words a chief reads, written from the realm's world and the
//! actors' answers.

use std::fmt::Display;

use hamlet_messages::profile::{CommitId, Path, VersionList};
use hamlet_protocol::client::{self as protocol, Choice, WaitDetail};

use crate::oversight::{self, Ruling};
use crate::spend;

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

pub(crate) const CHARACTER_SAVED: &str = "The character is saved.";

pub(crate) const MEMORY_SAVED: &str = "The memory is saved.";

pub(crate) const SETTINGS_SAVED: &str = "The settings are saved.";

pub(crate) const REFLECT_EVERY: &str =
    "Reflect after zero or more turns.";

pub(crate) const REALM_LIMITS_SAVED: &str =
    "The realm's limits are saved.";

pub(crate) const LIMITS_UNFIT: &str =
    "Those limits do not fit their form.";

pub(crate) const NO_WORKSPACE: &str =
    "Choose a remote workspace in the settings first.";

pub(crate) const FAUX_MODELS: &str =
    "Every agent runs the faux model here.";

/// The reason of a `/merge` said without one.
pub(crate) const NO_SCHOLAR_YET: &str =
    "The city has no scholar yet.";

pub(crate) const DREAM_USAGE: &str =
    "Use /dream <scholar#id|address>";

pub(crate) const DREAM_BEGAN: &str = "The dream began.";

pub(crate) const MERGED_BY_THE_CHIEF: &str = "merged by the chief";

/// The reason of a `/remove-hamlet` said without one.
pub(crate) const CLOSED_BY_THE_CHIEF: &str = "closed by the chief";

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
pub(crate) fn limits_saved(hamlet: &str) -> String {
    format!("The limits of `{hamlet}` are saved.")
}

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

pub(crate) fn merged(absorbed: &str, into: &str) -> String {
    format!("Merged `{absorbed}` into `{into}`.")
}

pub(crate) fn hire_back(hire: &str) -> String {
    format!("`{hire}` is back.")
}

/// The answer to restoring a hire that was merged.
pub(crate) fn hire_split(hire: &str) -> String {
    format!(
        "`{hire}` is back; what it brought to the hire it was \
         merged into is taken out again."
    )
}

pub(crate) fn erased(subject: &str) -> String {
    format!("`{subject}` was erased for good.")
}

pub(crate) fn hamlet_removed(hamlet: &str) -> String {
    format!("The hamlet `{hamlet}` was removed and its hires ended.")
}

pub(crate) fn hamlet_restored(hamlet: &str) -> String {
    format!("The hamlet `{hamlet}` is restored.")
}

pub(crate) fn hamlet_erased(hamlet: &str) -> String {
    format!("The hamlet `{hamlet}` was erased for good.")
}

/// `at` is absent for `main`.
pub(crate) fn forked(
    from: &str,
    at: Option<&str>,
    name: &str,
) -> String {
    match at {
        Some(at) => format!("Forked {from} at {at} as {name}."),
        None => format!("Forked {from} as {name}."),
    }
}

pub(crate) fn profile_archived(profile: &str) -> String {
    format!("`{profile}` left the city, with its hires.")
}

pub(crate) fn profile_restored(profile: &str) -> String {
    format!("`{profile}` is back in the city.")
}

pub(crate) fn welcomed(profile: &str) -> String {
    format!("`{profile}` moved into its house in the city.")
}

pub(crate) fn turned_away(profile: &str) -> String {
    format!("`{profile}` waits at the harbor.")
}

pub(crate) fn house_moved(profile: &str, x: f64, z: f64) -> String {
    format!("`{profile}`'s house stands at {x}, {z}.")
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

pub(crate) fn tools_off(hire: &str, off: &[String]) -> String {
    if off.is_empty() {
        format!("`{hire}` has every tool.")
    } else {
        format!("`{hire}` has these off: {}", off.join(", "))
    }
}

pub(crate) fn scholars<'a>(
    shown: impl Iterator<Item = (String, bool)> + 'a,
) -> String {
    let list = shown
        .map(|(label, living)| {
            format!(
                "- {label}{}",
                if living { "" } else { " (archived)" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if list.is_empty() {
        NO_SCHOLAR_YET.into()
    } else {
        list
    }
}

pub(crate) fn short(commit: &CommitId) -> String {
    commit.0.to_string().chars().take(8).collect()
}

/// A profile's versions and the latest commits of its `main`.
pub(crate) fn versions(list: &VersionList) -> String {
    let mut lines = Vec::new();
    if list.versions.is_empty() {
        lines.push("No versions yet; /tag names one.".to_owned());
    } else {
        lines.push("Versions:".into());
        for version in &list.versions {
            let note = if version.note.is_empty() {
                String::new()
            } else {
                format!(": {}", version.note)
            };
            lines.push(format!(
                "- {} ({}){note}",
                version.version.0,
                short(&version.commit)
            ));
        }
    }
    if !list.commits.is_empty() {
        lines.push("Latest commits of main:".into());
        for commit in &list.commits {
            lines.push(format!(
                "- {} {} by {}",
                short(&commit.commit),
                commit.message,
                commit.by
            ));
        }
    }
    lines.join("\n")
}

pub(crate) fn profile_made(profile: &str) -> String {
    format!("The scholar `{profile}` lives in the city.")
}

pub(crate) fn tagged(profile: &str, version: &str) -> String {
    format!("Tagged {profile} as {version}.")
}

pub(crate) fn reverted(
    commit: &str,
    profile: &str,
    clashes: &[Path],
) -> String {
    if clashes.is_empty() {
        format!("Reverted {commit} on {profile}'s main.")
    } else {
        let clashes = clashes
            .iter()
            .map(|clash| clash.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Reverted {commit} on {profile}'s main; these changed \
             again since and kept their later lines: {clashes}"
        )
    }
}

pub(crate) fn purged(
    path: &str,
    profile: &str,
    branches: u32,
) -> String {
    format!(
        "Purged {path} from {profile}: {branches} branches \
         rewritten."
    )
}
