//! A chief's typed line (H24, H26): a command the server carries out,
//! an answer to what waits, or words for the agent.

use anyhow::Result;
use hamlet_protocol::{
    Address, Choice, Deed, Permission, ReplyKind, Say, ServerMessage,
};
use serde_json::Value;

use super::{Deferred, deeds, records};
use crate::documents::Mode;
use crate::harness::{
    BindingId, Harnesses, Input, InputMark, InputStatus,
};
use crate::oversight::WaitKind;
use crate::protocol::{Refusable, Refusal, reply};
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
        choice: Choice,
        text: String,
    },
    Revoke {
        agent: String,
        permission: Permission,
    },
    Folder(String),
    Mode(Mode),
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
    let trimmed = text.trim();
    let (name, rest) = match trimmed.find(char::is_whitespace) {
        Some(at) => (&trimmed[..at], trimmed[at..].trim()),
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
    let words = rest.split_whitespace();
    Some(match *deed {
        "to" => match rest.find(char::is_whitespace) {
            Some(at) if !rest[at..].trim().is_empty() => {
                Ok(Command::To {
                    agent: rest[..at].into(),
                    text: rest[at..].trim_start().into(),
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
                    choice: serde_json::from_value(Value::from(
                        *deed,
                    ))
                    .expect("an answer's command names its choice"),
                    text: rest.into(),
                })
            }
        }
        "revoke" => {
            let words = words.collect::<Vec<_>>();
            match words[..] {
                [agent, name] => match gate::permission(name) {
                    Some(permission) => Ok(Command::Revoke {
                        agent: agent.into(),
                        permission,
                    }),
                    None => usage(),
                },
                _ => usage(),
            }
        }
        "folder" => Ok(Command::Folder(rest.into())),
        "mode" => match rest {
            "manual" => Ok(Command::Mode(Mode::Manual)),
            "auto" => Ok(Command::Mode(Mode::Auto)),
            _ => usage(),
        },
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

/// A chief's line, as `say` hands it to a command's handler.
struct Said<'a> {
    client: u64,
    id: &'a str,
    to: &'a Address,
    /// The line, with every secret removed.
    text: String,
}

impl Said<'_> {
    fn hamlet(&self) -> &str {
        &self.to.hamlet
    }

    fn notice(&self, text: String) -> Option<ServerMessage> {
        Some(reply(self.id, Ok(text)))
    }

    fn refused(&self, refusal: Refusal) -> Option<ServerMessage> {
        Some(reply(self.id, Err(refusal)))
    }
}

/// Handles a chief's line said to `said.to`: the reply, or `None`
/// when it is given once the line's input ends.
pub(super) async fn say(
    service: &mut Service,
    harnesses: &mut Harnesses,
    client: u64,
    said: &Say,
) -> Result<Option<ServerMessage>> {
    let said = Said {
        client,
        id: &said.id,
        to: &said.to,
        text: service.credentials.scrub_text(&said.text),
    };
    if let Err(refusal) = service.access(said.hamlet(), "say")? {
        return Ok(said.refused(refusal));
    }
    let hamlet = said.hamlet();
    let command = match read_command(&said.text) {
        Some(Err(usage)) => {
            return Ok(Some(answer(said.id, usage.kind, usage.text)));
        }
        Some(Ok(command)) => command,
        None => return plain(service, harnesses, &said).await,
    };
    Ok(match command {
        Command::Deed(deed, rest) => {
            return run_deed(service, harnesses, &said, deed, &rest)
                .await;
        }
        Command::To { agent, text } => {
            return to(service, harnesses, &said, &agent, &text)
                .await;
        }
        Command::Budget => said.notice(budget(service, hamlet)?),
        Command::ResetBudget => {
            said.notice(service.reset_budget(hamlet)?)
        }
        Command::Grants => said.notice(grants(service, hamlet)?),
        Command::Revoke { agent, permission } => {
            said.notice(revoke(service, hamlet, &agent, &permission)?)
        }
        Command::Folder(path) => {
            Some(reply(said.id, service.folder(hamlet, &path)?))
        }
        Command::Mode(mode) => {
            said.notice(service.set_mode(hamlet, mode)?)
        }
        Command::Give { agent, skill, from } => {
            return give(
                service,
                &said,
                &agent,
                &skill,
                from.as_deref(),
            )
            .await;
        }
        Command::Stop(name) => {
            return stop(service, harnesses, &said, name.as_deref())
                .await;
        }
        Command::New => {
            return conversation(
                service,
                harnesses,
                &said,
                deeds::Deed::New,
            )
            .await;
        }
        Command::Resume(Some(n)) => {
            return conversation(
                service,
                harnesses,
                &said,
                deeds::Deed::Resume(n),
            )
            .await;
        }
        Command::Reflect => {
            return conversation(
                service,
                harnesses,
                &said,
                deeds::Deed::Reflect,
            )
            .await;
        }
        Command::Retry => {
            return conversation(
                service,
                harnesses,
                &said,
                deeds::Deed::Retry,
            )
            .await;
        }
        Command::Resume(None) => match addressed(service, &said)? {
            Ok(agent) => said.notice(sessions(service, &agent)?),
            Err(refusal) => said.refused(refusal),
        },
        Command::Queue => return queue(service, &said),
        Command::Answer { choice, text } => {
            return answer_wait(
                service, harnesses, &said, &choice, &text,
            )
            .await;
        }
    })
}

/// The agent `said` is addressed to, or why it is refused.
fn addressed(
    service: &mut Service,
    said: &Said,
) -> Result<Refusable<AgentRef>> {
    Ok(service.agent(said.to, "say")?.map(|agent| AgentRef {
        hamlet: said.hamlet().into(),
        number: agent.number,
    }))
}

/// A server command named by `deed`, with its arguments `rest`.
async fn run_deed(
    service: &mut Service,
    harnesses: &mut Harnesses,
    said: &Said<'_>,
    deed: &str,
    rest: &str,
) -> Result<Option<ServerMessage>> {
    let hamlet = said.hamlet();
    if ["merge", "restore", "undo"].contains(&deed) {
        crate::society::merging::lane::chief(
            service,
            said.client,
            said.id,
            hamlet,
            deed,
            rest,
        )?;
        return Ok(None);
    }
    if let Some(done) =
        crate::society::deed(service, harnesses, hamlet, deed, rest)
            .await?
    {
        return Ok(Some(reply(said.id, done)));
    }
    Ok(match deed {
        "agent" | "hamlet" | "chief" => {
            said.notice("Choose that address in your client.".into())
        }
        "undo" => match service.home.store().record(hamlet)? {
            Some(record) if record.archive.is_empty() => said.notice(
                "Nothing to undo. Only a removal or a merge can be \
                 undone (the archive keeps the house); a permission \
                 is taken back with /revoke. A command that ran, a \
                 file that was changed and what was spent cannot be \
                 undone."
                    .into(),
            ),
            _ => said.refused(Refusal::Unsupported("that command")),
        },
        _ => said.refused(Refusal::Unsupported("that command")),
    })
}

/// A line for the living agent `agent` of the hamlet.
async fn to(
    service: &mut Service,
    harnesses: &mut Harnesses,
    said: &Said<'_>,
    agent: &str,
    text: &str,
) -> Result<Option<ServerMessage>> {
    let target = match living(service, said.hamlet(), agent)? {
        Ok(target) => target,
        Err(words) => {
            return Ok(Some(answer(
                said.id,
                ReplyKind::Error,
                words,
            )));
        }
    };
    line(service, harnesses, said.client, said.id, &target, text)
        .await?;
    Ok(None)
}

/// The hamlet's spend in lines.
fn budget(service: &mut Service, hamlet: &str) -> Result<String> {
    let report = service.budget(hamlet)?;
    let mut lines = vec![report.summary];
    lines.extend(report.lines);
    Ok(lines.join("\n"))
}

/// Gives the skill `skill` to `agent`, from `from` or the chief.
async fn give(
    service: &mut Service,
    said: &Said<'_>,
    agent: &str,
    skill: &str,
    from: Option<&str>,
) -> Result<Option<ServerMessage>> {
    let address = |name: &str| Address {
        agent: name.into(),
        hamlet: said.hamlet().into(),
    };
    let from = from.map(address);
    crate::gifts::skill(
        service,
        said.client,
        said.id,
        &address(agent),
        skill,
        from.as_ref(),
    )
}

/// Stops the agent `name`, or every agent.
async fn stop(
    service: &mut Service,
    harnesses: &mut Harnesses,
    said: &Said<'_>,
    name: Option<&str>,
) -> Result<Option<ServerMessage>> {
    let Some(name) = name else {
        return Ok(Some(reply(
            said.id,
            deeds::stop(service, harnesses, None).await?,
        )));
    };
    let target = match living(service, said.hamlet(), name)? {
        Ok(target) => target,
        Err(words) => {
            return Ok(Some(answer(
                said.id,
                ReplyKind::Error,
                words,
            )));
        }
    };
    deeds::stop_agent(service, harnesses, &target).await?;
    Ok(said.notice(format!("Stopping {name}.")))
}

/// Carries out `deed` on the conversation of the addressed agent.
async fn conversation(
    service: &mut Service,
    harnesses: &mut Harnesses,
    said: &Said<'_>,
    deed: deeds::Deed,
) -> Result<Option<ServerMessage>> {
    let agent = match addressed(service, said)? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(said.refused(refusal)),
    };
    let later =
        matches!(deed, deeds::Deed::Reflect | deeds::Deed::Retry);
    Ok(match deeds::deed(service, harnesses, &agent, deed).await? {
        Err(words) => Some(answer(said.id, ReplyKind::Notice, words)),
        Ok(done) if later => match done.submitted {
            Some((binding, request)) => {
                owe(
                    service,
                    said.client,
                    said.id,
                    binding,
                    &request,
                )?;
                None
            }
            None => Some(answer(
                said.id,
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
                client: said.client,
                id: said.id.into(),
                binding,
                request,
                text,
                kept: true,
            });
            None
        }
        Ok(done) => {
            Some(answer(said.id, ReplyKind::Notice, done.text))
        }
    })
}

/// The addressed agent, or `None` when the hamlet has no such agent;
/// `Err` when the address is refused for another reason.
fn present(
    service: &mut Service,
    said: &Said,
) -> Result<Refusable<Option<AgentRef>>> {
    Ok(match addressed(service, said)? {
        Ok(agent) => Ok(Some(agent)),
        Err(Refusal::NoAgent(_)) => Ok(None),
        Err(refusal) => Err(refusal),
    })
}

/// The open wait the chief's line to `agent` answers: one of its
/// current chat, else any of its own.
fn own_wait(
    service: &mut Service,
    waits: &[oversight::Wait],
    agent: &AgentRef,
) -> Result<Option<oversight::Wait>> {
    let chief = service.chief.clone();
    let number = records::current_number(
        service.home.store().connection(),
        agent,
        &chief,
    )?;
    Ok(oversight::for_chat(waits, agent, &chief)
        .find(|wait| {
            wait.chat.as_ref().map(|chat| chat.session) == number
        })
        .or_else(|| oversight::for_agent(waits, agent).next())
        .cloned())
}

/// What waits for the chief.
fn queue(
    service: &mut Service,
    said: &Said,
) -> Result<Option<ServerMessage>> {
    if let Err(refusal) = present(service, said)? {
        return Ok(said.refused(refusal));
    }
    let waits = oversight::open(service.home.store().connection())?;
    let now = crate::clock::now();
    Ok(said.notice(if waits.is_empty() {
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
    }))
}

/// Answers the addressed agent's wait, else the first that waits.
async fn answer_wait(
    service: &mut Service,
    harnesses: &mut Harnesses,
    said: &Said<'_>,
    choice: &Choice,
    text: &str,
) -> Result<Option<ServerMessage>> {
    let agent = match present(service, said)? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(said.refused(refusal)),
    };
    let waits = oversight::open(service.home.store().connection())?;
    let own = match &agent {
        Some(agent) => own_wait(service, &waits, agent)?,
        None => None,
    };
    let Some(wait) = own.as_ref().or(waits.first()) else {
        return Ok(said.notice("Nothing waits for you.".into()));
    };
    let wait = wait.id.clone();
    let words = match super::calls::answer_words(
        service, harnesses, &wait, choice, text, None,
    )
    .await?
    {
        Ok(words) => words,
        Err(refusal) => refusal.text(),
    };
    Ok(Some(answer(said.id, ReplyKind::Notice, words)))
}

/// A line that is no command: the reply to the agent's question, or
/// words for it.
async fn plain(
    service: &mut Service,
    harnesses: &mut Harnesses,
    said: &Said<'_>,
) -> Result<Option<ServerMessage>> {
    let agent = match addressed(service, said)? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(said.refused(refusal)),
    };
    let waits = oversight::open(service.home.store().connection())?;
    match own_wait(service, &waits, &agent)? {
        Some(wait) if matches!(wait.kind, WaitKind::Question(_)) => {
            let outcome = super::calls::answer_words(
                service,
                harnesses,
                &wait.id,
                &Choice::Reply,
                &said.text,
                None,
            )
            .await?;
            return Ok(Some(match outcome {
                Ok(words) => {
                    answer(said.id, ReplyKind::Notice, words)
                }
                Err(refusal) => {
                    answer(said.id, ReplyKind::Error, refusal.text())
                }
            }));
        }
        Some(wait)
            if !said.text.trim_start().starts_with('/')
                && matches!(
                    wait.kind,
                    WaitKind::Approval(_) | WaitKind::Objection
                ) =>
        {
            return Ok(said.notice(format!(
                "Not sent. {} is waiting for your answer:\n{}",
                wait.by,
                oversight::strip(&wait)
            )));
        }
        _ => {}
    }
    line(
        service,
        harnesses,
        said.client,
        said.id,
        &agent,
        &said.text,
    )
    .await?;
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
    let at = crate::clock::now();
    let task = format!("chief:{}:{id}", binding.0);
    records::instruct(
        service.home.store().connection(),
        agent,
        &task,
        &chief,
        text,
        at,
    )?;
    let mark = InputMark::Chief {
        chief,
        text: text.into(),
        task,
        at,
    };
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
    for (permission, names) in [
        (Permission::Network, &limits.permissions.network),
        (Permission::Unconfined, &limits.permissions.unconfined),
    ] {
        for name in names {
            lines.push(format!(
                "{name} may use: {} (limits.toml)",
                gate::word(&permission)
            ));
        }
    }
    for grant in gate::grants(service.home.store().connection())?
        .into_iter()
        .filter(|grant| grant.agent.hamlet == hamlet)
    {
        let permission = gate::word(&grant.permission);
        lines.push(format!(
            "{} may use: {permission} (given by {}; /revoke {} \
             {permission})",
            grant.name, grant.by, grant.name,
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
    permission: &Permission,
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
            crate::clock::now(),
        )?;
    }
    let held = service.limits_of(hamlet)?.permits(permission, agent);
    let permission = gate::word(permission);
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
    ServerMessage::Reply(hamlet_protocol::Reply {
        kind,
        text,
        to: id.into(),
        upload: None,
    })
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
                choice: Choice::Deny,
                text: "not here".into()
            }))
        );
    }
}
