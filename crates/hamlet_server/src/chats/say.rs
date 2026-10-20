//! A chief's typed line (H24, H26): a command the server carries out,
//! an answer to what waits, or words for the agent.

use anyhow::Result;
use hamlet_protocol::v0::{
    Address, Deed, ReplyKind, Say, ServerMessage,
};
use serde_json::{Value, json};

use super::{Deferred, deeds, records};
use crate::harness::{BindingId, Harnesses, Input, InputStatus};
use crate::protocol::{Refusal, reply};
use crate::record::AgentRef;
use crate::service::{Service, records as hamlets};
use crate::{gate, oversight};

/// The chief's commands, with their usage and what they do (H26).
const TABLE: &[(&str, &str, &str)] = &[
    (
        "new",
        "",
        "Start a new session and keep this one in history",
    ),
    (
        "resume",
        "<number>",
        "Go back to a past session, by its number in History",
    ),
    (
        "retry",
        "",
        "Carry on a turn that failed or was stopped, from where it \
         ended",
    ),
    ("agent", "<name>", "Talk to that agent, in whichever hamlet"),
    ("hamlet", "<name>", "Talk to that hamlet's mayor"),
    ("to", "<agent> <instruction>", "Instruct an agent directly"),
    ("stop", "[agent|all]", "Stop an agent's work, or everyone's"),
    ("queue", "", "List what waits for you"),
    ("allow", "[reason]", "Allow what waits, this once"),
    ("always", "[reason]", "Allow it, and the like from now on"),
    ("deny", "[reason]", "Refuse what waits"),
    (
        "go",
        "[reason]",
        "Carry out an instruction an agent objected to",
    ),
    (
        "withdraw",
        "[reason]",
        "Withdraw an instruction an agent objected to",
    ),
    ("reply", "<text>", "Answer what waits in words"),
    ("dismiss", "", "Put away a fault or a lapsed request"),
    ("undo", "", "Reverse the latest thing that can be reversed"),
    ("grants", "", "List what agents are allowed for good"),
    (
        "revoke",
        "<agent> <network|unconfined>",
        "Take back a lasting permission",
    ),
    (
        "remove",
        "<agent> [reason]",
        "Remove an agent and archive its house",
    ),
    (
        "merge",
        "<absorbed> into <kept> [reason]",
        "Merge one agent into another",
    ),
    (
        "road",
        "<agent> <agent> [reason]",
        "Build a road between two agents",
    ),
    (
        "unroad",
        "<agent> <agent> [reason]",
        "Take up the road between two agents",
    ),
    (
        "give",
        "<agent> <skill> [from <agent>]",
        "Give an agent a starter skill, or another's",
    ),
    (
        "folder",
        "<folder|none>",
        "Point this hamlet at a folder to work on, or at none",
    ),
    (
        "mode",
        "<manual|auto>",
        "Whether agents ask before they change the folder",
    ),
    (
        "reset-budget",
        "",
        "Start this hamlet's budget again from nothing spent",
    ),
    (
        "restore",
        "<agent> [as <new-name>] [reason]",
        "Bring back a removed or merged agent",
    ),
    (
        "erase",
        "<agent>",
        "Delete a removed agent for good, to free its room on disk",
    ),
    (
        "record",
        "",
        "Who was created, removed, merged and restored",
    ),
    ("budget", "", "The hamlet's budget and each agent's spend"),
    (
        "reflect",
        "",
        "Have the agent reflect on this conversation now",
    ),
    (
        "mayor",
        "<agent> [reason]",
        "Make an agent of this hamlet its mayor",
    ),
    (
        "vote",
        "<agent> [reason]",
        "Ask this hamlet's agents to vote on making that agent \
         their mayor",
    ),
    ("votes", "", "This hamlet's votes, with every ballot"),
    (
        "overturn",
        "<vote> [reason]",
        "Reverse a vote that passed, or stop an open one",
    ),
    ("hamlets", "", "List the hamlets and their mayors"),
    (
        "found",
        "<hamlet> <mayor> <purpose>",
        "Found a hamlet of your own under a new mayor",
    ),
    (
        "remove-hamlet",
        "<hamlet> [reason]",
        "Remove a hamlet and archive its houses",
    ),
    ("restore-hamlet", "<hamlet>", "Bring a removed hamlet back"),
    (
        "unshelve",
        "<skill> [reason]",
        "Take a skill out of the library",
    ),
    (
        "erase-hamlet",
        "<hamlet>",
        "Delete a removed hamlet for good",
    ),
    (
        "name-hamlet",
        "<hamlet> <new-name>",
        "Call a hamlet something else",
    ),
    (
        "move-hamlet",
        "<hamlet> <x> <z>",
        "Set a hamlet's town down somewhere else",
    ),
    ("chief", "", "The commands about hamlets and votes"),
];

/// The chief's commands the server carries out (H26).
pub(crate) fn deeds() -> Vec<Deed> {
    TABLE
        .iter()
        .map(|(name, usage, description)| Deed {
            name: (*name).into(),
            usage: (*usage).into(),
            description: (*description).into(),
        })
        .collect()
}

/// A parsed typed command.
#[derive(Debug, PartialEq)]
enum Command {
    Deed(&'static str, String),
    To {
        agent: String,
        text: String,
    },
    New,
    Retry,
    Reflect,
    Budget,
    ResetBudget,
    Queue,
    Grants,
    Resume(Option<i64>),
    Stop(Option<String>),
    Answer {
        choice: &'static str,
        text: String,
    },
    Revoke {
        agent: String,
        permission: String,
    },
    Folder(String),
    Mode(String),
    Give {
        agent: String,
        skill: String,
        from: Option<String>,
    },
}

/// A command whose arguments need correcting, as a chief reads it.
#[derive(Debug, PartialEq)]
struct Usage {
    text: String,
    kind: ReplyKind,
}

/// A chief's command, a usage to correct, or `None` for words to the
/// agent.
fn read_command(
    text: &str,
) -> Option<std::result::Result<Command, Usage>> {
    let trimmed = js_trim(text);
    let (name, rest) = match trimmed.find(js_space) {
        Some(at) => (&trimmed[..at], js_trim(&trimmed[at..])),
        None => (trimmed, ""),
    };
    let (deed, args, _) = TABLE
        .iter()
        .find(|(deed, ..)| name.strip_prefix('/') == Some(deed))?;
    let usage = || {
        let command = format!("/{deed}");
        Err(Usage {
            kind: if ["/to", "/mode", "/give"]
                .contains(&command.as_str())
            {
                ReplyKind::Error
            } else {
                ReplyKind::Notice
            },
            text: if command == "/reply" {
                "say what to reply: /reply <text>".into()
            } else {
                format!("usage: {command} {args}").trim().into()
            },
        })
    };
    let words = rest.split(js_space).filter(|w| !w.is_empty());
    Some(match *deed {
        "to" => match rest.find(js_space) {
            Some(at) if !js_trim(&rest[at..]).is_empty() => {
                Ok(Command::To {
                    agent: rest[..at].into(),
                    text: rest[at..]
                        .trim_start_matches(js_space)
                        .into(),
                })
            }
            _ => usage(),
        },
        "new" | "retry" | "reflect" | "budget" => {
            if !rest.is_empty() {
                return None;
            }
            Ok(match *deed {
                "new" => Command::New,
                "retry" => Command::Retry,
                "reflect" => Command::Reflect,
                _ => Command::Budget,
            })
        }
        "queue" => Ok(Command::Queue),
        "grants" => Ok(Command::Grants),
        "reset-budget" => Ok(Command::ResetBudget),
        "resume" => {
            if rest.is_empty() {
                Ok(Command::Resume(None))
            } else {
                match rest.parse::<i64>() {
                    Ok(n)
                        if rest
                            .bytes()
                            .all(|b| b.is_ascii_digit())
                            && n <= 9_007_199_254_740_991 =>
                    {
                        Ok(Command::Resume(Some(n)))
                    }
                    _ => Err(Usage {
                        text: "Say which: `/resume <number>`, with \
                               the number History shows for a past \
                               session."
                            .into(),
                        kind: ReplyKind::Notice,
                    }),
                }
            }
        }
        "stop" => Ok(Command::Stop(
            (!rest.is_empty() && rest != "all").then(|| rest.into()),
        )),
        "allow" | "always" | "deny" | "go" | "withdraw"
        | "dismiss" | "reply" => {
            if *deed == "reply" && rest.is_empty() {
                usage()
            } else {
                Ok(Command::Answer {
                    choice: deed,
                    text: rest.into(),
                })
            }
        }
        "revoke" => {
            let words = words.collect::<Vec<_>>();
            match words[..] {
                [agent, permission]
                    if permission == "network"
                        || permission == "unconfined" =>
                {
                    Ok(Command::Revoke {
                        agent: agent.into(),
                        permission: permission.into(),
                    })
                }
                _ => usage(),
            }
        }
        "folder" => Ok(Command::Folder(rest.into())),
        "mode" => {
            if rest == "manual" || rest == "auto" {
                Ok(Command::Mode(rest.into()))
            } else {
                usage()
            }
        }
        "give" => {
            let words = words.collect::<Vec<_>>();
            match words[..] {
                [agent, skill] => Ok(Command::Give {
                    agent: agent.into(),
                    skill: skill.into(),
                    from: None,
                }),
                [agent, skill, "from", from] => Ok(Command::Give {
                    agent: agent.into(),
                    skill: skill.into(),
                    from: Some(from.into()),
                }),
                _ => usage(),
            }
        }
        _ => Ok(Command::Deed(deed, rest.into())),
    })
}

/// Handles a chief's line said to `said.to`: the reply, or `None`
/// when it is given once the line's input ends.
pub(super) async fn say(
    service: &mut Service,
    harnesses: &mut Harnesses,
    client: u64,
    said: &Say,
) -> Result<Option<ServerMessage>> {
    let id = said.id.as_str();
    let hamlet = said.to.hamlet.as_str();
    if let Err(refusal) = service.access(hamlet, "say")? {
        return Ok(Some(reply(id, Err(refusal))));
    }
    let mut text = Value::String(said.text.clone());
    service.credentials.scrub(&mut text);
    let text = text.as_str().unwrap_or_default().to_owned();
    let command = match read_command(&text) {
        Some(Err(usage)) => {
            return Ok(Some(answer(id, usage.kind, usage.text)));
        }
        Some(Ok(command)) => Some(command),
        None => None,
    };
    let notice = |text: String| Ok(Some(reply(id, Ok(text))));
    match &command {
        Some(Command::Deed(deed, rest)) => {
            if ["merge", "restore", "undo"].contains(deed) {
                crate::society::merging::lane::chief(
                    service, client, id, hamlet, deed, rest,
                )?;
                return Ok(None);
            }
            if let Some(done) = crate::society::deed(
                service, harnesses, hamlet, deed, rest,
            )
            .await?
            {
                return Ok(Some(reply(id, done)));
            }
            return match *deed {
                "agent" | "hamlet" | "chief" => notice(
                    "Choose that address in your client.".into(),
                ),
                "undo" => {
                    match service.home.store().record(hamlet)? {
                        Some(record) if record.archive.is_empty() => {
                            notice(
                                "Nothing to undo. Only a removal or \
                                 a merge can be undone (the archive \
                                 keeps the house); a permission is \
                                 taken back with /revoke. A command \
                                 that ran, a file that was changed \
                                 and what was spent cannot be \
                                 undone."
                                    .into(),
                            )
                        }
                        _ => Ok(Some(reply(
                            id,
                            Err(Refusal::Unsupported("that command")),
                        ))),
                    }
                }
                _ => Ok(Some(reply(
                    id,
                    Err(Refusal::Unsupported("that command")),
                ))),
            };
        }
        Some(Command::To { agent, text }) => {
            let target = match living(service, hamlet, agent)? {
                Ok(target) => target,
                Err(words) => {
                    return Ok(Some(answer(
                        id,
                        ReplyKind::Error,
                        words,
                    )));
                }
            };
            line(service, harnesses, client, id, &target, text)
                .await?;
            return Ok(None);
        }
        Some(Command::Budget) => {
            let report = service.budget(hamlet)?;
            let mut lines = vec![
                report["summary"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            ];
            lines.extend(
                report["lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(String::from)),
            );
            return notice(lines.join("\n"));
        }
        Some(Command::ResetBudget) => {
            return notice(service.reset_budget(hamlet)?);
        }
        Some(Command::Grants) => {
            return notice(grants(service, hamlet)?);
        }
        Some(Command::Revoke { agent, permission }) => {
            return notice(revoke(
                service, hamlet, agent, permission,
            )?);
        }
        Some(Command::Folder(path)) => {
            return Ok(Some(reply(
                id,
                service.folder(hamlet, path)?,
            )));
        }
        Some(Command::Mode(mode)) => {
            return notice(service.set_mode(hamlet, mode)?);
        }
        Some(Command::Give { agent, skill, from }) => {
            let address = |name: &str| Address {
                agent: name.into(),
                hamlet: hamlet.into(),
            };
            return crate::gifts::skill(
                service,
                client,
                id,
                &address(agent),
                skill,
                from.as_deref().map(address).as_ref(),
            );
        }
        Some(Command::Stop(None)) => {
            return Ok(Some(reply(
                id,
                deeds::stop(service, harnesses, None).await?,
            )));
        }
        Some(Command::Stop(Some(name))) => {
            let target = match living(service, hamlet, name)? {
                Ok(target) => target,
                Err(words) => {
                    return Ok(Some(answer(
                        id,
                        ReplyKind::Error,
                        words,
                    )));
                }
            };
            deeds::stop_agent(service, harnesses, &target).await?;
            return notice(format!("Stopping {name}."));
        }
        _ => {}
    }
    let target = match service.agent(&said.to, "say")? {
        Ok(agent) => Ok(AgentRef {
            hamlet: hamlet.into(),
            number: agent.number,
        }),
        Err(refusal) => {
            let words = refusal.text();
            if !words.starts_with("no agent named") {
                return Ok(Some(reply(id, Err(refusal))));
            }
            Err(refusal)
        }
    };
    if let Ok(agent) = &target {
        let deed = match &command {
            Some(Command::New) => Some(deeds::Deed::New),
            Some(Command::Resume(Some(n))) => {
                Some(deeds::Deed::Resume(*n))
            }
            Some(Command::Reflect) => Some(deeds::Deed::Reflect),
            Some(Command::Retry) => Some(deeds::Deed::Retry),
            Some(Command::Resume(None)) => {
                return notice(sessions(service, agent)?);
            }
            _ => None,
        };
        if let Some(deed) = deed {
            let later = matches!(
                deed,
                deeds::Deed::Reflect | deeds::Deed::Retry
            );
            return Ok(
                match deeds::deed(service, harnesses, agent, deed)
                    .await?
                {
                    Err(words) => {
                        Some(answer(id, ReplyKind::Notice, words))
                    }
                    Ok(done) if later => match done.submitted {
                        Some((binding, request)) => {
                            owe(
                                service, client, id, binding,
                                &request,
                            )?;
                            None
                        }
                        None => Some(answer(
                            id,
                            ReplyKind::Notice,
                            String::new(),
                        )),
                    },
                    Ok(deeds::Done {
                        text,
                        written: Some((binding, request)),
                        ..
                    }) => {
                        service.chats.deferred.push(Deferred {
                            client,
                            id: id.into(),
                            binding,
                            request,
                            text,
                            kept: true,
                        });
                        None
                    }
                    Ok(done) => {
                        Some(answer(id, ReplyKind::Notice, done.text))
                    }
                },
            );
        }
    }
    let db = service.home.store().connection();
    let waits = oversight::open(db)?;
    let chief = service.chief.clone();
    let own = match &target {
        Ok(agent) => {
            let number = records::current_number(db, agent, &chief)?;
            oversight::for_chat(&waits, agent, &chief)
                .find(|wait| {
                    wait.chat.as_ref().map(|chat| chat.session)
                        == number
                })
                .or_else(|| {
                    oversight::for_agent(&waits, agent).next()
                })
                .cloned()
        }
        Err(_) => None,
    };
    match &command {
        Some(Command::Queue) => {
            let now = super::now();
            return notice(if waits.is_empty() {
                "Nothing waits for you.".into()
            } else {
                waits
                    .iter()
                    .map(|wait| {
                        format!(
                            "{} (waited {}s, asked {})",
                            oversight::strip(wait),
                            (now - wait.since).max(0) / 1000,
                            wait.asked
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            });
        }
        Some(Command::Answer { choice, text }) => {
            let Some(wait) = own.as_ref().or(waits.first()) else {
                return notice("Nothing waits for you.".into());
            };
            let wait = wait.id.clone();
            let outcome = super::calls::answer_words(
                service, harnesses, &wait, choice, text, None,
            )
            .await?;
            return Ok(Some(match outcome {
                Ok(words) | Err(words) => {
                    answer(id, ReplyKind::Notice, words)
                }
            }));
        }
        _ => {}
    }
    if command.is_none()
        && let Some(wait) =
            own.as_ref().filter(|w| w.kind == "question")
    {
        let wait = wait.id.clone();
        let outcome = super::calls::answer_words(
            service, harnesses, &wait, "reply", &text, None,
        )
        .await?;
        return Ok(Some(match outcome {
            Ok(words) => answer(id, ReplyKind::Notice, words),
            Err(words) => answer(id, ReplyKind::Error, words),
        }));
    }
    if !text.trim_start().starts_with('/')
        && let Some(wait) = own
            .as_ref()
            .filter(|w| w.kind == "approval" || w.kind == "objection")
    {
        return notice(format!(
            "Not sent. {} is waiting for your answer:\n{}",
            wait.by,
            oversight::strip(wait)
        ));
    }
    let agent = match target {
        Ok(agent) => agent,
        Err(refusal) => return Ok(Some(reply(id, Err(refusal)))),
    };
    line(service, harnesses, client, id, &agent, &text).await?;
    Ok(None)
}

/// Submits a chief's typed line `id` to the current conversation of
/// its chat with `agent`, as a new task; its reply is owed once its
/// input ends.
async fn line(
    service: &mut Service,
    harnesses: &mut Harnesses,
    client: u64,
    id: &str,
    agent: &AgentRef,
    text: &str,
) -> Result<()> {
    let binding =
        admit_line(service, harnesses, id, agent, text).await?;
    owe(service, client, id, binding, &format!("chief:{id}"))
}

/// Admits a chief's instruction without owing a receipt.
pub(crate) async fn admit_line(
    service: &mut Service,
    harnesses: &mut Harnesses,
    id: &str,
    agent: &AgentRef,
    text: &str,
) -> Result<BindingId> {
    let binding = super::current(service, harnesses, agent).await?;
    let chief = service.chief.clone();
    let at = super::now();
    let task = format!("chief:{}:{id}", binding.0);
    records::instruct(
        service.home.store().connection(),
        agent,
        &task,
        &chief,
        text,
        at,
    )?;
    let mark = super::mark(
        "hamlet.chief",
        json!({"chief": chief, "text": text, "task": task, "at": at}),
    );
    harnesses
        .submit(
            service.home.store(),
            binding,
            &Input {
                request: &format!("chief:{id}"),
                content: text,
                steer: false,
                mark: Some(&mark),
                write: false,
                boundary: false,
            },
        )
        .await?;
    Ok(binding)
}

/// Owes `id` its reply once the input `request` of `binding` ends.
fn owe(
    service: &mut Service,
    client: u64,
    id: &str,
    binding: BindingId,
    request: &str,
) -> Result<()> {
    let ended = service
        .home
        .store()
        .standing(binding, request)?
        .is_some_and(|standing| {
            standing.status.is_some_and(InputStatus::is_ended)
        });
    if ended {
        service
            .chats
            .outbox
            .push((client, reply(id, Ok(String::new()))));
    } else {
        service.chats.deferred.push(Deferred {
            client,
            id: id.into(),
            binding,
            request: request.into(),
            text: String::new(),
            kept: false,
        });
    }
    Ok(())
}

/// The living agent `text` names from `hamlet`: a name or office,
/// with `@hamlet` for another hamlet's; else the words that refuse
/// it.
pub(super) fn living(
    service: &mut Service,
    hamlet: &str,
    text: &str,
) -> Result<std::result::Result<AgentRef, String>> {
    let missing = || Ok(Err(format!("no agent named `{text}`")));
    let mut parts = text.split('@');
    let (Some(name), place, None) =
        (parts.next(), parts.next(), parts.next())
    else {
        return missing();
    };
    if name.is_empty() || place == Some("") {
        return missing();
    }
    for record in hamlets(service.home.store().connection())? {
        let wanted = match place {
            Some(place) => record.id == place,
            None => name != "mayor" || record.id == hamlet,
        };
        if !wanted {
            continue;
        }
        if let Some(agent) = record.named(name) {
            let found = AgentRef {
                hamlet: record.id.clone(),
                number: agent.number,
            };
            if let Err(refusal) =
                service.access(&found.hamlet, "say")?
            {
                return Ok(Err(refusal.text()));
            }
            return Ok(Ok(found));
        }
        if let Some(gone) = record
            .archive
            .iter()
            .rev()
            .find(|a| a.back.is_none() && a.agent.name == name)
        {
            return Ok(Err(record.farewell(gone)));
        }
    }
    missing()
}

/// The words of `/grants`: what the limits give, then each grant.
fn grants(service: &mut Service, hamlet: &str) -> Result<String> {
    let limits = service.limits_of(hamlet)?;
    let mut lines = Vec::new();
    for (permission, names) in
        limits["permissions"].as_object().into_iter().flatten()
    {
        for name in names.as_array().into_iter().flatten() {
            lines.push(format!(
                "{} may use: {permission} (limits.toml)",
                name.as_str().unwrap_or_default()
            ));
        }
    }
    for grant in gate::grants(service.home.store().connection())?
        .into_iter()
        .filter(|grant| grant.agent.hamlet == hamlet)
    {
        lines.push(format!(
            "{} may use: {} (given by {}; /revoke {} {})",
            grant.name,
            grant.permission,
            grant.by,
            grant.name,
            grant.permission
        ));
    }
    Ok(if lines.is_empty() {
        "No agent holds a permission.".into()
    } else {
        lines.join("\n")
    })
}

/// Takes back what was granted to `agent`, and says what it still
/// holds by the limits.
fn revoke(
    service: &mut Service,
    hamlet: &str,
    agent: &str,
    permission: &str,
) -> Result<String> {
    let record = service.home.store().record(hamlet)?;
    let db = service.home.store().connection();
    let target = record
        .and_then(|record| {
            record.named(agent).map(|a| AgentRef {
                hamlet: hamlet.into(),
                number: a.number,
            })
        })
        .or_else(|| {
            gate::grants(db).ok()?.into_iter().find_map(|grant| {
                (grant.agent.hamlet == hamlet && grant.name == agent)
                    .then_some(grant.agent)
            })
        });
    let chief = service.chief.clone();
    if let Some(target) = target {
        gate::revoke(
            service.home.store().connection(),
            &target,
            permission,
            &chief,
            super::now(),
        )?;
    }
    let limits = service.limits_of(hamlet)?;
    let held = limits["permissions"][permission]
        .as_array()
        .is_some_and(|names| names.iter().any(|n| n == agent));
    Ok(if held {
        format!(
            "Revoked what was granted. `{agent}` still has \
             {permission} under [permissions] in limits.toml."
        )
    } else {
        format!("`{agent}` no longer has {permission}.")
    })
}

/// The words of a bare `/resume`: the chat's past sessions.
fn sessions(
    service: &mut Service,
    agent: &AgentRef,
) -> Result<String> {
    let past = super::watch::pasts(service, agent)?
        .into_iter()
        .filter(|past| !past.current)
        .collect::<Vec<_>>();
    Ok(if past.is_empty() {
        "There are no past sessions here.".into()
    } else {
        past.iter()
            .map(|past| {
                format!(
                    "{}: {} ({} messages, {} tokens; {})",
                    past.number,
                    past.title,
                    past.messages,
                    past.tokens,
                    chrono::DateTime::from_timestamp_millis(
                        past.began
                    )
                    .unwrap_or_default()
                    .to_rfc3339_opts(
                        chrono::SecondsFormat::Millis,
                        true
                    )
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn answer(id: &str, kind: ReplyKind, text: String) -> ServerMessage {
    ServerMessage::Reply(hamlet_protocol::v0::Reply {
        kind,
        text,
        to: id.into(),
        upload: None,
    })
}

/// The characters JavaScript's `\s` and `trim` take as white space.
fn js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(js_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_commands_and_leaves_words_for_the_agent() {
        assert_eq!(read_command("Hello."), None);
        assert_eq!(read_command("/nothing"), None);
        assert_eq!(read_command("/new words"), None);
        assert_eq!(read_command("/NEW"), None);
        assert_eq!(read_command("/new"), Some(Ok(Command::New)));
        assert_eq!(
            read_command("/stop all"),
            Some(Ok(Command::Stop(None)))
        );
        assert_eq!(
            read_command("/resume wrong").unwrap().unwrap_err().kind,
            ReplyKind::Notice
        );
        assert_eq!(
            read_command("/mode fast").unwrap().unwrap_err(),
            Usage {
                text: "usage: /mode <manual|auto>".into(),
                kind: ReplyKind::Error
            }
        );
        assert_eq!(
            read_command("/revoke mayor").unwrap().unwrap_err().text,
            "usage: /revoke <agent> <network|unconfined>"
        );
        assert_eq!(
            read_command("/deny not here"),
            Some(Ok(Command::Answer {
                choice: "deny",
                text: "not here".into()
            }))
        );
    }
}
