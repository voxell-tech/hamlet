//! A command's line read into the act it asks.

use std::path::PathBuf;

use hamlet_git::MAIN;
use hamlet_messages::profile::VersionName;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::hamlets::{
    EraseHamlet, MoveHamlet, NameHamlet, RemoveHamlet, RestoreHamlet,
    SetFolder, SetMode,
};
use hamlet_messages::realm::profiles::{
    ArchiveProfile, EraseProfile, ForkProfile, RestoreProfile, Spot,
    TurnAway, Welcome,
};
use hamlet_messages::realm::workers::StartWorker;
use hamlet_messages::realm::{
    Found, HamletId, Hire, Named, ProfileId, SetTier, Tier,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Mode;
use hamlet_protocol::client::{self as protocol, Choice};

use super::super::words;
use crate::commands::{deeds, usage};
use crate::oversight::gate;
use crate::society::addresses::{self, Address};
use crate::society::agents::Effort as Level;
use crate::society::refusal::Refusal;
use crate::spend;

/// The word `/create-hamlet` and `/hire` take in place of a scholar
/// for a blank agent.
const BLANK: &str = "blank";

/// An agent as a command names it: as the realm takes it, and as the
/// chief wrote it, for the reply.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    pub(super) named: Named,
    pub(super) text: String,
}

/// The part of an agent's model a command sets; the others stay as
/// they are.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Part {
    Model {
        model: Option<String>,
        why: Option<String>,
    },
    Effort(Option<Level>),
    Fast(bool),
}

/// What a command's line asks, its addresses read but none of them
/// resolved.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Act {
    Choose,
    New,
    Sessions,
    Resume(i64),
    Retry,
    /// The agent spoken to when none is named.
    Stop(Option<Target>),
    StopAll,
    To {
        to: Target,
        text: String,
    },
    Reflect,
    Worker(StartWorker),
    Queue,
    Answer {
        choice: Choice,
        wait: Option<String>,
        text: Option<String>,
    },
    Grants,
    Revoke {
        agent: Target,
        permission: protocol::Permission,
    },
    Hamlets,
    Found(Found),
    NameHamlet(NameHamlet),
    MoveHamlet(MoveHamlet),
    Tier {
        set: SetTier,
        most: u32,
    },
    Seat {
        hire: Target,
        hamlet: HamletId,
        why: Option<String>,
    },
    Hire(Hire),
    End {
        hire: Target,
        why: Option<String>,
    },
    Merge {
        absorbed: Target,
        /// Refused only once `absorbed` is found, as today reads
        /// them.
        into: Result<Target, Refusal>,
        why: String,
    },
    Restore {
        hire: Target,
        why: Option<String>,
    },
    Erase {
        hire: Target,
    },
    RemoveHamlet(RemoveHamlet),
    RestoreHamlet(RestoreHamlet),
    EraseHamlet(EraseHamlet),
    Fork(ForkProfile),
    ArchiveProfile(ArchiveProfile),
    RestoreProfile(RestoreProfile),
    EraseProfile(EraseProfile),
    Welcome(Welcome),
    TurnAway(TurnAway),
    MoveHouse {
        profile: ProfileId,
        /// Refused only once the profile is found, as today reads
        /// the numbers.
        spot: Result<Spot, Refusal>,
    },
    Steer {
        hire: Target,
        part: Part,
    },
    Nickname {
        hire: Target,
        name: Option<String>,
    },
    Tools {
        hire: Target,
        off: Vec<String>,
        on: Vec<String>,
    },
    Allowance {
        hire: Target,
        micros: Option<i64>,
    },
    Road {
        a: Target,
        b: Target,
        why: Option<String>,
        lay: bool,
    },
    Folder(SetFolder),
    Mode {
        set: SetMode,
        word: String,
    },
    Budget(Option<HamletId>),
    ResetBudget(Payer),
    /// A command of the table this server does not do.
    Unsupported(String),
}

/// The commands that answer a wait.
const ANSWERS: [&str; 7] = [
    "allow", "always", "deny", "go", "withdraw", "reply", "dismiss",
];

pub(crate) fn is_stop(line: &str) -> bool {
    split(line).0 == "stop"
}

/// Whether `line` is an answer that names its wait.
pub(crate) fn names_its_wait(line: &str) -> bool {
    let (name, rest) = split(line);
    ANSWERS.contains(&name) && rest.starts_with('#')
}

pub(crate) fn is_new(line: &str) -> bool {
    split(line).0 == "new"
}

/// A command's name, and the rest of its line.
fn split(line: &str) -> (&str, &str) {
    let line = line.trim().trim_start_matches('/');
    line.split_once(char::is_whitespace)
        .map_or((line, ""), |(name, rest)| (name, rest.trim()))
}

/// Reads `line` as a command said in `here`.
pub(super) fn parse(
    line: &str,
    here: Option<&HamletId>,
) -> Result<Act, Refusal> {
    let (name, rest) = split(line);
    let words = rest.split_whitespace().collect::<Vec<_>>();
    let tail =
        |from: usize| words.get(from..).unwrap_or_default().join(" ");
    let why =
        |from: usize| Some(tail(from)).filter(|why| !why.is_empty());
    let first = || words.first().copied().ok_or_else(|| usage(name));
    let at = |text: &str| target(text, here);
    let hamlet_here = || here.cloned().ok_or_else(no_hamlet);
    Ok(match name {
        "hand" | "hamlet" | "agent" => Act::Choose,
        "new" => Act::New,
        "resume" => {
            match words.first().and_then(|word| word.parse().ok()) {
                Some(number) => Act::Resume(number),
                None => Act::Sessions,
            }
        }
        "retry" => Act::Retry,
        "stop" => match words.first() {
            None => Act::Stop(None),
            Some(&"all") => Act::StopAll,
            Some(text) => Act::Stop(Some(at(text)?)),
        },
        "to" => {
            let (address, text) = rest
                .split_once(char::is_whitespace)
                .ok_or_else(|| usage(name))?;
            let text = text.trim();
            if text.is_empty() {
                return Err(usage(name));
            }
            Act::To {
                to: at(address)?,
                text: text.to_owned(),
            }
        }
        "reflect" => Act::Reflect,
        "worker" => {
            if rest.is_empty() {
                return Err(usage(name));
            }
            Act::Worker(StartWorker {
                brief: rest.to_owned(),
                tools: Vec::new(),
                task: None,
                hamlet: hamlet_here().ok(),
            })
        }
        "queue" => Act::Queue,
        name if ANSWERS.contains(&name) => answer(name, rest),
        "grants" => Act::Grants,
        "revoke" => {
            let (Some(address), Some(word)) =
                (words.first(), words.get(1))
            else {
                return Err(usage(name));
            };
            Act::Revoke {
                agent: at(address)?,
                permission: gate::permission(word)
                    .ok_or_else(|| usage(name))?,
            }
        }
        "hamlets" => Act::Hamlets,
        "create-hamlet" => found(name, &words)?,
        "name-hamlet" => Act::NameHamlet(NameHamlet {
            hamlet: HamletId(first()?.to_owned()),
            name: tail(1).trim().to_owned(),
        }),
        "move-hamlet" => {
            let number = |at: usize| {
                words.get(at).and_then(|w| w.parse().ok())
            };
            let (Some(hamlet), Some(x), Some(z)) =
                (words.first(), number(1), number(2))
            else {
                return Err(usage(name));
            };
            Act::MoveHamlet(MoveHamlet {
                hamlet: HamletId((*hamlet).to_owned()),
                x,
                z,
            })
        }
        "tier" => {
            let hamlet = hamlet_here()?;
            let most = words
                .first()
                .and_then(|word| word.parse::<i64>().ok())
                .ok_or_else(|| usage(name))?;
            let tier = match most {
                50 => Tier::Fifty,
                100 => Tier::Hundred,
                200 => Tier::TwoHundred,
                500 => Tier::FiveHundred,
                _ => return Err(Refusal::BadTier),
            };
            Act::Tier {
                set: SetTier { hamlet, tier },
                most: u32::try_from(most).unwrap_or_default(),
            }
        }
        "mayor" => {
            let hamlet = hamlet_here()?;
            Act::Seat {
                hire: at(first()?)?,
                hamlet,
                why: why(1),
            }
        }
        "hire" => hire(name, &words, hamlet_here().ok())?,
        "end" => Act::End {
            hire: at(first()?)?,
            why: why(1),
        },
        "merge" => {
            let (Some(absorbed), Some(&"into"), Some(into)) =
                (words.first(), words.get(1), words.get(2))
            else {
                return Err(usage(name));
            };
            Act::Merge {
                absorbed: at(absorbed)?,
                into: at(into),
                why: why(3).unwrap_or_else(|| {
                    words::MERGED_BY_THE_CHIEF.into()
                }),
            }
        }
        "restore" => Act::Restore {
            hire: at(first()?)?,
            why: why(1),
        },
        "erase" => Act::Erase {
            hire: at(first()?)?,
        },
        "remove-hamlet" => Act::RemoveHamlet(RemoveHamlet {
            hamlet: HamletId(first()?.to_owned()),
            why: Some(why(1).unwrap_or_else(|| {
                words::CLOSED_BY_THE_CHIEF.into()
            })),
        }),
        "restore-hamlet" => Act::RestoreHamlet(RestoreHamlet {
            hamlet: HamletId(first()?.to_owned()),
            why: why(1),
        }),
        "erase-hamlet" => Act::EraseHamlet(EraseHamlet {
            hamlet: HamletId(first()?.to_owned()),
        }),
        "fork" => {
            let given = first()?;
            let (from, at) =
                given.split_once('@').unwrap_or((given, MAIN));
            Act::Fork(ForkProfile {
                from: profile_of(from)?,
                at: (at != MAIN).then(|| VersionName(at.to_owned())),
                name: tail(1),
            })
        }
        "remove-scholar" => Act::ArchiveProfile(ArchiveProfile {
            profile: profile_of(first()?)?,
            why: why(1),
        }),
        "restore-scholar" => Act::RestoreProfile(RestoreProfile {
            profile: profile_of(first()?)?,
            why: why(1),
        }),
        "erase-scholar" => Act::EraseProfile(EraseProfile {
            profile: profile_of(first()?)?,
        }),
        "welcome" => Act::Welcome(Welcome {
            profile: profile_of(first()?)?,
            why: why(1),
        }),
        "turn-away" => Act::TurnAway(TurnAway {
            profile: profile_of(first()?)?,
            why: why(1),
        }),
        "move-house" => {
            let profile = profile_of(first()?)?;
            let number = |at: usize| {
                words.get(at).map(|word| word.parse::<f64>().ok())
            };
            let spot = match (number(1), number(2), number(3)) {
                (Some(Some(x)), Some(Some(z)), None) => Ok(Spot {
                    x,
                    z,
                    degrees: None,
                }),
                (
                    Some(Some(x)),
                    Some(Some(z)),
                    Some(Some(degrees)),
                ) => Ok(Spot {
                    x,
                    z,
                    degrees: Some(degrees),
                }),
                _ => Err(usage(name)),
            };
            Act::MoveHouse { profile, spot }
        }
        "model" => {
            let (Some(text), Some(model)) =
                (words.first(), words.get(1))
            else {
                return Err(usage(name));
            };
            Act::Steer {
                hire: at(text)?,
                part: Part::Model {
                    model: (*model != "none")
                        .then(|| (*model).to_owned()),
                    why: why(2),
                },
            }
        }
        "effort" | "fast" => {
            let (Some(text), Some(value)) =
                (words.first(), words.get(1))
            else {
                return Err(usage(name));
            };
            let part = match (name, *value) {
                ("fast", "on") => Part::Fast(true),
                ("fast", "off") => Part::Fast(false),
                ("effort", "none") => Part::Effort(None),
                ("effort", level) => Part::Effort(Some(
                    Level::named(level).ok_or_else(|| usage(name))?,
                )),
                _ => return Err(usage(name)),
            };
            Act::Steer {
                hire: at(text)?,
                part,
            }
        }
        "nickname" => Act::Nickname {
            hire: at(first()?)?,
            name: why(1),
        },
        "tools" => {
            let hire = at(first()?)?;
            let marked = |mark: char| {
                words[1..]
                    .iter()
                    .filter_map(|word| word.strip_prefix(mark))
                    .map(String::from)
                    .collect::<Vec<_>>()
            };
            let (off, on) = (marked('-'), marked('+'));
            if off.is_empty() && on.is_empty() {
                return Ok(Act::Unsupported(format!(
                    "`/{name}` with no change"
                )));
            }
            Act::Tools { hire, off, on }
        }
        "allowance" => {
            let (Some(text), Some(amount)) =
                (words.first(), words.get(1))
            else {
                return Err(usage(name));
            };
            let micros = match *amount {
                "none" => None,
                amount => match amount
                    .trim_start_matches('$')
                    .parse::<f64>()
                {
                    Ok(dollars) if dollars >= 0.0 => {
                        Some(spend::micros(dollars))
                    }
                    _ => return Err(usage(name)),
                },
            };
            Act::Allowance {
                hire: at(text)?,
                micros,
            }
        }
        "road" | "unroad" => {
            let (Some(a), Some(b)) = (words.first(), words.get(1))
            else {
                return Err(usage(name));
            };
            Act::Road {
                a: at(a)?,
                b: at(b)?,
                why: why(2),
                lay: name == "road",
            }
        }
        "folder" => {
            let hamlet = hamlet_here()?;
            let folder = match first()? {
                "none" => None,
                _ => Some(PathBuf::from(rest)),
            };
            Act::Folder(SetFolder { hamlet, folder })
        }
        "mode" => {
            let hamlet = hamlet_here()?;
            let (mode, word) = match words.first() {
                Some(&"manual") => (Mode::Manual, "manual"),
                Some(&"auto") => (Mode::Auto, "auto"),
                _ => return Err(usage(name)),
            };
            Act::Mode {
                set: SetMode { hamlet, mode },
                word: word.to_owned(),
            }
        }
        "budget" => Act::Budget(hamlet_here().ok()),
        "reset-budget" => Act::ResetBudget(match hamlet_here() {
            Ok(hamlet) => Payer::Hamlet { hamlet },
            Err(_) => Payer::Realm,
        }),
        _ if deeds().iter().any(|deed| deed.name == name) => {
            Act::Unsupported(format!("`/{name}`"))
        }
        _ => {
            return Err(Refusal::Usage {
                words: words::no_command(name),
            });
        }
    })
}

/// A chief's answer to a wait: the one `#<id>` names, or none.
fn answer(name: &str, rest: &str) -> Act {
    let choice = match name {
        "allow" => Choice::Allow,
        "always" => Choice::Always,
        "deny" => Choice::Deny,
        "go" => Choice::Go,
        "withdraw" => Choice::Withdraw,
        "reply" => Choice::Reply,
        _ => Choice::Dismiss,
    };
    let (wait, text) = match rest.strip_prefix('#') {
        Some(after) => {
            let (id, text) = after
                .split_once(char::is_whitespace)
                .map_or((after, ""), |(id, text)| (id, text.trim()));
            (Some(id.to_owned()), text)
        }
        None => (None, rest),
    };
    Act::Answer {
        choice,
        wait,
        text: Some(text.to_owned()).filter(|text| !text.is_empty()),
    }
}

/// `/create-hamlet [scholar#id|blank] <name>[: purpose]`: a mayor
/// not given is a blank agent, and one given no purpose is there to
/// run the hamlet.
fn found(name: &str, words: &[&str]) -> Result<Act, Refusal> {
    let (mayor, from) = match words.first() {
        Some(&BLANK) => (None, 1),
        Some(first) if first.starts_with("scholar#") => {
            (Some(profile(first)?), 1)
        }
        _ => (None, 0),
    };
    let rest = words.get(from..).unwrap_or_default().join(" ");
    let (shown, purpose) = rest
        .split_once(':')
        .map_or((rest.as_str(), ""), |(shown, purpose)| {
            (shown, purpose)
        });
    let shown = shown.trim();
    if shown.is_empty() {
        return Err(usage(name));
    }
    let purpose = match purpose.trim() {
        "" => words::mayor_of(shown),
        given => given.to_owned(),
    };
    Ok(Act::Found(Found {
        name: shown.to_owned(),
        mayor,
        tier: Tier::Fifty,
        purpose: Some(purpose),
    }))
}

fn hire(
    name: &str,
    words: &[&str],
    here: Option<HamletId>,
) -> Result<Act, Refusal> {
    let Some(first) = words.first() else {
        return Err(usage(name));
    };
    let (profile, version) = match *first {
        BLANK => (None, None),
        given => match given.split_once('@') {
            Some((address, version)) => (
                Some(profile(address)?),
                Some(VersionName(version.to_owned())),
            ),
            None => (Some(profile(given)?), None),
        },
    };
    let mut rest = &words[1..];
    let mut hamlet = here;
    let mut under = None;
    while let [key, value, more @ ..] = rest {
        match *key {
            "under" => {
                under = Some(target(value, hamlet.as_ref())?.named);
            }
            "in" => hamlet = Some(HamletId((*value).to_owned())),
            _ => break,
        }
        rest = more;
    }
    let hamlet = hamlet.ok_or_else(no_hamlet)?;
    let purpose = rest.join(" ");
    Ok(Act::Hire(Hire {
        hamlet: Some(hamlet),
        under,
        profile,
        version,
        purpose: Some(purpose).filter(|purpose| !purpose.is_empty()),
        name: None,
    }))
}

/// The profile a scholar's address names, refusing a version.
fn profile(text: &str) -> Result<ProfileId, Refusal> {
    if text.contains('@') {
        return Err(Refusal::Unsupported {
            what: "a scholar's `@<version>`".into(),
        });
    }
    profile_of(text)
}

/// The profile a scholar's address names.
fn profile_of(text: &str) -> Result<ProfileId, Refusal> {
    let id = addresses::parse_scholar(text)?;
    u32::try_from(id.0).map(ProfileId).map_err(|_| {
        Refusal::BadAddress {
            address: text.into(),
        }
    })
}

/// The agent `text` addresses, said in `here`: `mayor` is the
/// office of `here`.
pub(super) fn target(
    text: &str,
    here: Option<&HamletId>,
) -> Result<Target, Refusal> {
    let bad = || Refusal::BadAddress {
        address: text.into(),
    };
    let named = match addresses::parse(text)? {
        Address::Hand => Named::Hand,
        Address::Mayor {
            hamlet: Some(hamlet),
        } => Named::MayorOf {
            hamlet: HamletId(hamlet.0),
        },
        Address::Mayor { hamlet: None } => match here {
            Some(hamlet) => Named::MayorOf {
                hamlet: hamlet.clone(),
            },
            None => Named::Mayor,
        },
        Address::Hire(id) => Named::Agent {
            agent: AgentNumber(
                u32::try_from(id.0).map_err(|_| bad())?,
            ),
        },
        Address::Worker(id) => Named::Worker {
            worker: AgentNumber(
                u32::try_from(id.0).map_err(|_| bad())?,
            ),
        },
    };
    Ok(Target {
        named,
        text: text.trim().to_owned(),
    })
}

fn no_hamlet() -> Refusal {
    Refusal::Usage {
        words: words::NO_HAMLET.into(),
    }
}

#[cfg(test)]
mod tests;
