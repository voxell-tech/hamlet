//! A command's line read into the act it asks.

use std::path::PathBuf;

use hamlet_messages::profile::VersionName;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::hamlets::{
    MoveHamlet, NameHamlet, SetFolder, SetMode,
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

/// The profile a scholar's address names.
fn profile(text: &str) -> Result<ProfileId, Refusal> {
    if text.contains('@') {
        return Err(Refusal::Unsupported {
            what: "a scholar's `@<version>`".into(),
        });
    }
    let id = addresses::parse_scholar(text)?;
    u32::try_from(id.0).map(ProfileId).map_err(|_| {
        Refusal::BadAddress {
            address: text.into(),
        }
    })
}

/// The agent `text` addresses, said in `here`: `mayor` is the
/// office of `here`.
fn target(
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
        Address::Hire(id) | Address::Worker(id) => Named::Agent {
            agent: AgentNumber(
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
mod tests {
    use super::*;

    /// A hamlet's id, in the one form an address writes it.
    const OTHER: &str = "0b9e2c4a-6f1d-4e8b-9a3c-5d7f1e2a4b6c";

    fn docs() -> HamletId {
        HamletId("docs".into())
    }

    fn hire(number: u32, text: &str) -> Target {
        Target {
            named: Named::Agent {
                agent: AgentNumber(number),
            },
            text: text.into(),
        }
    }

    #[test]
    fn mayor_is_the_office_of_the_hamlet_it_is_said_in() {
        assert_eq!(
            parse("/end mayor", Some(&docs())),
            Ok(Act::End {
                hire: Target {
                    named: Named::MayorOf { hamlet: docs() },
                    text: "mayor".into(),
                },
                why: None,
            })
        );
        assert_eq!(
            parse(&format!("/stop mayor@{OTHER}"), None),
            Ok(Act::Stop(Some(Target {
                named: Named::MayorOf {
                    hamlet: HamletId(OTHER.into()),
                },
                text: format!("mayor@{OTHER}"),
            })))
        );
    }

    #[test]
    fn a_hire_and_a_worker_are_named_by_number() {
        assert_eq!(
            parse("/nickname worker#7 Scout", None),
            Ok(Act::Nickname {
                hire: hire(7, "worker#7"),
                name: Some("Scout".into()),
            })
        );
        assert_eq!(
            parse("/road hire#2 hire#3 they share a page", None),
            Ok(Act::Road {
                a: hire(2, "hire#2"),
                b: hire(3, "hire#3"),
                why: Some("they share a page".into()),
                lay: true,
            })
        );
    }

    #[test]
    fn a_bad_address_is_refused_in_todays_words() {
        assert_eq!(
            parse("/end scholar#2", None).unwrap_err().to_string(),
            "`scholar#2` is not an address"
        );
    }

    #[test]
    fn effort_and_fast_each_set_one_part() {
        assert_eq!(
            parse("/effort hire#2 high", None),
            Ok(Act::Steer {
                hire: hire(2, "hire#2"),
                part: Part::Effort(Some(Level::High)),
            })
        );
        assert_eq!(
            parse("/fast hire#2 on", None),
            Ok(Act::Steer {
                hire: hire(2, "hire#2"),
                part: Part::Fast(true),
            })
        );
        assert_eq!(
            parse("/effort hire#2 hard", None)
                .unwrap_err()
                .to_string(),
            "Use /effort <address> \
             <off|minimal|low|medium|high|xhigh|max|none>"
        );
    }

    #[test]
    fn a_wait_is_the_one_named_or_none() {
        assert_eq!(
            parse("/reply #4 Use the blue one.", None),
            Ok(Act::Answer {
                choice: Choice::Reply,
                wait: Some("4".into()),
                text: Some("Use the blue one.".into()),
            })
        );
        assert_eq!(
            parse("/deny", None),
            Ok(Act::Answer {
                choice: Choice::Deny,
                wait: None,
                text: None,
            })
        );
    }

    #[test]
    fn a_hamlet_is_founded_under_a_blank_mayor_unless_a_profile_is_named()
     {
        assert_eq!(
            parse("/create-hamlet Docs", None),
            Ok(Act::Found(Found {
                name: "Docs".into(),
                mayor: None,
                tier: Tier::Fifty,
                purpose: Some("Mayor of Docs.".into()),
            }))
        );
        assert_eq!(
            parse(
                "/create-hamlet scholar#3 Docs: Keep the docs.",
                None
            ),
            Ok(Act::Found(Found {
                name: "Docs".into(),
                mayor: Some(ProfileId(3)),
                tier: Tier::Fifty,
                purpose: Some("Keep the docs.".into()),
            }))
        );
    }

    #[test]
    fn a_hire_goes_into_the_hamlet_here_unless_one_is_named() {
        assert_eq!(
            parse(
                "/hire blank under mayor write the guide",
                Some(&docs())
            ),
            Ok(Act::Hire(Hire {
                hamlet: Some(docs()),
                under: Some(Named::MayorOf { hamlet: docs() }),
                profile: None,
                version: None,
                purpose: Some("write the guide".into()),
                name: None,
            }))
        );
        assert_eq!(
            parse("/hire scholar#2", None).unwrap_err().to_string(),
            words::NO_HAMLET
        );
    }

    #[test]
    fn every_command_served_is_parsed_into_its_act() {
        for name in [
            "hand",
            "hamlet",
            "agent",
            "new",
            "resume",
            "retry",
            "stop",
            "to",
            "reflect",
            "worker",
            "queue",
            "allow",
            "always",
            "deny",
            "go",
            "withdraw",
            "reply",
            "dismiss",
            "grants",
            "revoke",
            "hamlets",
            "create-hamlet",
            "name-hamlet",
            "move-hamlet",
            "tier",
            "mayor",
            "hire",
            "end",
            "model",
            "effort",
            "fast",
            "nickname",
            "tools",
            "allowance",
            "road",
            "unroad",
            "folder",
            "mode",
            "budget",
            "reset-budget",
        ] {
            let parsed = parse(&format!("/{name}"), Some(&docs()));
            assert_ne!(
                parsed,
                Ok(Act::Unsupported(format!("`/{name}`"))),
                "/{name}"
            );
            if let Err(refusal) = parsed {
                assert_ne!(
                    refusal.to_string(),
                    words::no_command(name),
                    "/{name}"
                );
            }
        }
    }

    #[test]
    fn a_command_of_the_table_not_done_yet_is_unsupported() {
        assert_eq!(
            parse("/merge hire#2 into hire#3", None),
            Ok(Act::Unsupported("`/merge`".into()))
        );
        assert_eq!(
            parse("/history", Some(&docs())),
            Ok(Act::Unsupported("`/history`".into()))
        );
        assert_eq!(
            parse("/dance", None).unwrap_err().to_string(),
            "There is no command /dance; the client lists the \
             commands."
        );
    }
}
