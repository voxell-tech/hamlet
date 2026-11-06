//! The chief's commands: typed lines that begin with `/` (H24, H26),
//! said to an agent, whose hamlet is the one they mean.

mod oversight;
mod shelves;
mod versions;

use anyhow::Result;
use hamlet_git::Repository;
use hamlet_protocol::client as protocol;
use hamlet_store::ids::{Actor, AgentId, HamletId, ScholarId};

use crate::core::Core;
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, AgentKind, Effort, EndHow};
use crate::society::hamlets::{self, Founding, Mode};
use crate::society::hires::{self, Hiring, Spec};
use crate::society::refusal::Refusal;
use crate::society::{authority, city, record, roads};
use crate::spend;

/// A command a chief can type: its name, how it is written, and what
/// it does.
struct Command {
    name: &'static str,
    usage: &'static str,
    does: &'static str,
}

/// The name of the scholar made for a mayor when `/found` names none.
const MAYOR: &str = "Mayor";

/// Whether `word` is a scholar's address, with or without a version.
fn is_scholar(word: &str) -> bool {
    word.starts_with("scholar#")
}

const TABLE: &[Command] = &[
    Command {
        name: "hand",
        usage: "/hand",
        does: "Talk to the Hand",
    },
    Command {
        name: "hamlet",
        usage: "/hamlet <hamlet>",
        does: "Talk to a hamlet's mayor",
    },
    Command {
        name: "agent",
        usage: "/agent <address>",
        does: "Talk to an agent",
    },
    Command {
        name: "new",
        usage: "/new",
        does: "Start a new conversation",
    },
    Command {
        name: "resume",
        usage: "/resume [n]",
        does: "Go back to an earlier conversation",
    },
    Command {
        name: "retry",
        usage: "/retry",
        does: "Carry on a failed or stopped turn",
    },
    Command {
        name: "stop",
        usage: "/stop [address|all]",
        does: "Stop an agent's work",
    },
    Command {
        name: "to",
        usage: "/to <address> <instruction>",
        does: "Instruct another agent from here",
    },
    Command {
        name: "reflect",
        usage: "/reflect",
        does: "Ask the agent to keep what this session taught it",
    },
    Command {
        name: "worker",
        usage: "/worker <task>",
        does: "Start a worker on a task in this hamlet",
    },
    Command {
        name: "queue",
        usage: "/queue",
        does: "List what waits for you",
    },
    Command {
        name: "allow",
        usage: "/allow [#wait] [reason]",
        does: "Allow what waits, this once",
    },
    Command {
        name: "always",
        usage: "/always [#wait] [reason]",
        does: "Allow it, and the like from now on",
    },
    Command {
        name: "deny",
        usage: "/deny [#wait] [reason]",
        does: "Refuse what waits",
    },
    Command {
        name: "go",
        usage: "/go [#wait] [reason]",
        does: "Carry out an instruction an agent objected to",
    },
    Command {
        name: "withdraw",
        usage: "/withdraw [#wait] [reason]",
        does: "Withdraw an instruction an agent objected to",
    },
    Command {
        name: "reply",
        usage: "/reply [#wait] <text>",
        does: "Answer a question that waits",
    },
    Command {
        name: "dismiss",
        usage: "/dismiss [#wait]",
        does: "Put away a fault or a question",
    },
    Command {
        name: "grants",
        usage: "/grants",
        does: "List what agents may use for good",
    },
    Command {
        name: "revoke",
        usage: "/revoke <address> <network|unconfined>",
        does: "Take back a lasting grant",
    },
    Command {
        name: "tag",
        usage: "/tag <scholar#id>[@<commit>] <version> [note]",
        does: "Name a version of a scholar",
    },
    Command {
        name: "versions",
        usage: "/versions <scholar#id>",
        does: "List a scholar's versions and latest commits",
    },
    Command {
        name: "pin",
        usage: "/pin <address> <version|main> [reason]",
        does: "Hold a hire at a version, or let it follow main",
    },
    Command {
        name: "revert",
        usage: "/revert <scholar#id> <commit|version> [reason]",
        does: "Undo a commit of a scholar's main",
    },
    Command {
        name: "fork",
        usage: "/fork <scholar#id>[@<version>] <new name>",
        does: "Make a new scholar from a version of another",
    },
    Command {
        name: "purge",
        usage: "/purge <scholar#id> <path>",
        does: "Take a file out of a scholar's whole history",
    },
    Command {
        name: "give",
        usage: "/give <scholar#id|address> <skill> from \
                <scholar#id|address>",
        does: "Copy a skill from one scholar to another",
    },
    Command {
        name: "unshelve",
        usage: "/unshelve <skill> [reason]",
        does: "Take a skill off the library's shelves",
    },
    Command {
        name: "dream",
        usage: "/dream <scholar#id|address>",
        does: "Have a hire bring its learning home, or a scholar \
               tidy",
    },
    Command {
        name: "hamlets",
        usage: "/hamlets",
        does: "List the hamlets",
    },
    Command {
        name: "found",
        usage: "/found <hamlet> [scholar#id] [purpose]",
        does: "Found a hamlet under a scholar as its mayor, a new \
               scholar named Mayor if none is given",
    },
    Command {
        name: "name-hamlet",
        usage: "/name-hamlet <hamlet> [name]",
        does: "Give a hamlet a name to be shown by",
    },
    Command {
        name: "move-hamlet",
        usage: "/move-hamlet <hamlet> <x> <z>",
        does: "Set a hamlet down elsewhere on the hamlet island",
    },
    Command {
        name: "remove-hamlet",
        usage: "/remove-hamlet <hamlet> <reason>",
        does: "Remove a hamlet and end its hires",
    },
    Command {
        name: "restore-hamlet",
        usage: "/restore-hamlet <hamlet> [reason]",
        does: "Bring a removed hamlet back",
    },
    Command {
        name: "erase-hamlet",
        usage: "/erase-hamlet <hamlet>",
        does: "Erase a removed hamlet for good",
    },
    Command {
        name: "tier",
        usage: "/tier <50|100|200|500>",
        does: "Set how many hires this hamlet holds",
    },
    Command {
        name: "mayor",
        usage: "/mayor <address> <reason>",
        does: "Give this hamlet's office to a hire",
    },
    Command {
        name: "hire",
        usage: "/hire <scholar#id>[@<version>] [under <address>] \
                [purpose]",
        does: "Hire a scholar into this hamlet",
    },
    Command {
        name: "end",
        usage: "/end <address> <reason>",
        does: "End a hire",
    },
    Command {
        name: "restore",
        usage: "/restore <address> [reason]",
        does: "Bring an ended hire back",
    },
    Command {
        name: "erase",
        usage: "/erase <address>",
        does: "Erase an ended hire for good",
    },
    Command {
        name: "merge",
        usage: "/merge <address> into <address> [reason]",
        does: "Make one hire of two; /restore splits them again",
    },
    Command {
        name: "model",
        usage: "/model <address> <model|none> [reason]",
        does: "Choose an agent's model",
    },
    Command {
        name: "effort",
        usage: "/effort <address> \
                <off|minimal|low|medium|high|xhigh|max|none>",
        does: "Choose how hard an agent's model thinks",
    },
    Command {
        name: "fast",
        usage: "/fast <address> <on|off>",
        does: "Turn an agent's fast tier on or off",
    },
    Command {
        name: "nickname",
        usage: "/nickname <address> [nickname]",
        does: "Call a hire something in the world, or by its name \
               again",
    },
    Command {
        name: "tools",
        usage: "/tools <address> -<tool> +<tool>",
        does: "Turn a hire's tools off or back on",
    },
    Command {
        name: "allowance",
        usage: "/allowance <address> <dollars|none>",
        does: "Give a hire an allowance for the period",
    },
    Command {
        name: "road",
        usage: "/road <address> <address> <reason>",
        does: "Lay a road between two hires",
    },
    Command {
        name: "unroad",
        usage: "/unroad <address> <address> <reason>",
        does: "Take up a road",
    },
    Command {
        name: "folder",
        usage: "/folder <folder|none>",
        does: "Choose the folder this hamlet works on",
    },
    Command {
        name: "mode",
        usage: "/mode <manual|auto>",
        does: "Choose how this hamlet's work is overseen",
    },
    Command {
        name: "budget",
        usage: "/budget",
        does: "Show this hamlet's spend",
    },
    Command {
        name: "reset-budget",
        usage: "/reset-budget",
        does: "Start this hamlet's budget period again",
    },
    Command {
        name: "record",
        usage: "/record",
        does: "Show this hamlet's decisions",
    },
    Command {
        name: "scholars",
        usage: "/scholars",
        does: "List the city's scholars",
    },
    Command {
        name: "scholar",
        usage: "/scholar new <name>[: <character>]",
        does: "Make a scholar in the city; a name is a label with \
               no colon, and the character follows the colon",
    },
    Command {
        name: "name-scholar",
        usage: "/name-scholar <scholar#id> <name>",
        does: "Give a scholar a new name",
    },
    Command {
        name: "remove-scholar",
        usage: "/remove-scholar <scholar#id> <reason>",
        does: "Archive a scholar and end its hires",
    },
    Command {
        name: "restore-scholar",
        usage: "/restore-scholar <scholar#id> [reason]",
        does: "Bring an archived scholar back with its hires",
    },
    Command {
        name: "erase-scholar",
        usage: "/erase-scholar <scholar#id>",
        does: "Erase an archived scholar for good",
    },
    Command {
        name: "move-house",
        usage: "/move-house <scholar#id> <x> <z> [degrees]",
        does: "Set a scholar's house down elsewhere in the city",
    },
    Command {
        name: "welcome",
        usage: "/welcome <scholar#id> [reason]",
        does: "Let a scholar waiting at the harbor into the city",
    },
    Command {
        name: "turn-away",
        usage: "/turn-away <scholar#id> [reason]",
        does: "Keep a scholar at the harbor, or send it back there",
    },
];

/// The commands a client may offer.
pub(crate) fn deeds() -> Vec<protocol::Deed> {
    TABLE
        .iter()
        .map(|command| protocol::Deed {
            description: command.does.into(),
            name: command.name.into(),
            // A deed's usage is what follows its name.
            usage: command
                .usage
                .strip_prefix('/')
                .and_then(|rest| rest.strip_prefix(command.name))
                .unwrap_or(command.usage)
                .trim()
                .into(),
        })
        .collect()
}

fn usage(name: &str) -> Refusal {
    let usage = TABLE
        .iter()
        .find(|c| c.name == name)
        .map_or("", |c| c.usage);
    Refusal::Usage {
        words: format!("Use {usage}"),
    }
}

impl Core {
    /// Runs the command `line`, said to `agent`, whose hamlet it
    /// means.
    pub(crate) async fn command(
        &mut self,
        agent: AgentId,
        line: &str,
    ) -> Result<Result<String, Refusal>> {
        let line = line.trim().trim_start_matches('/');
        let (name, rest) = line
            .split_once(char::is_whitespace)
            .map_or((line, ""), |(name, rest)| (name, rest.trim()));
        let words = rest.split_whitespace().collect::<Vec<_>>();
        let chief = self.realm.chief();
        let by = Actor::Chief(chief);
        let here = {
            let db = self.realm.store().connection();
            hires::find(db, agent)?.map(|hire| hire.hamlet)
        };
        if let Some(answer) = self
            .oversight_command(agent, here.as_ref(), name, rest)
            .await?
        {
            return Ok(answer);
        }
        if let Some(answer) =
            self.versions_command(here.as_ref(), name, &words).await?
        {
            return Ok(answer);
        }
        if let Some(answer) =
            self.shelves_command(here.as_ref(), name, &words).await?
        {
            self.publish().await?;
            return Ok(answer);
        }
        let tail = |from: usize| {
            words.get(from..).unwrap_or_default().join(" ")
        };
        match name {
            "hand" | "hamlet" | "agent" => Ok(Ok("Choose who you \
                                                  talk to in your \
                                                  client."
                .into())),
            "new" => {
                let number = self
                    .change(by, |cx, env| {
                        crate::talk::chats::new_session(
                            cx, env, agent, chief,
                        )
                    })
                    .await?;
                // Who watches the chat turns to the new session.
                self.rewatch(agent).await?;
                Ok(Ok(format!("Session {number} began.")))
            }
            "resume" => {
                let Some(number) =
                    words.first().and_then(|w| w.parse::<i64>().ok())
                else {
                    let db = self.realm.store().connection();
                    let sessions = crate::talk::chats::sessions(
                        db, agent, chief,
                    )?;
                    let list = sessions
                        .iter()
                        .map(|(number, _, _)| {
                            format!("- session {number}")
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    return Ok(Ok(if list.is_empty() {
                        "No sessions yet.".into()
                    } else {
                        list
                    }));
                };
                let done = self
                    .attempt(by, |cx, _| {
                        crate::talk::chats::resume(
                            cx, agent, chief, number,
                        )
                    })
                    .await?;
                self.rewatch(agent).await?;
                Ok(done
                    .map(|()| format!("Back in session {number}.")))
            }
            "retry" => {
                let done = self
                    .attempt(by, |cx, env| {
                        crate::talk::chats::retry(
                            cx, env, agent, chief,
                        )
                    })
                    .await?;
                Ok(done.map(|_| "Carrying on.".into()))
            }
            "stop" => {
                let target = match words.first() {
                    None => agent,
                    Some(&"all") => {
                        let mut stopped = 0;
                        for each in self.every_reached()? {
                            stopped += self
                                .change(by, |cx, _| {
                                    crate::talk::chats::stop(cx, each)
                                })
                                .await?;
                        }
                        return Ok(Ok(format!(
                            "Stopped {stopped} running turns."
                        )));
                    }
                    Some(text) => match self.resolve(
                        text,
                        here.as_ref(),
                        Reach::Living,
                    )? {
                        Ok(found) => found,
                        Err(refusal) => return Ok(Err(refusal)),
                    },
                };
                let stopped = self
                    .change(by, |cx, _| {
                        crate::talk::chats::stop(cx, target)
                    })
                    .await?;
                Ok(Ok(format!("Stopped {stopped} running turns.")))
            }
            "hamlets" => {
                let db = self.realm.store().connection();
                let list = hamlets::all(db)?
                    .into_iter()
                    .filter(hamlets::Hamlet::is_living)
                    .map(|h| {
                        format!(
                            "- {} (`{}`)",
                            h.title
                                .clone()
                                .unwrap_or_else(|| h.id.to_string()),
                            h.id
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(Ok(if list.is_empty() {
                    "No hamlet yet. /found makes one.".into()
                } else {
                    list
                }))
            }
            "found" => {
                let Some(id) = words.first() else {
                    return Ok(Err(usage(name)));
                };
                let id = HamletId((*id).to_owned());
                // A mayor not given is a new scholar named Mayor, and
                // one given no purpose is there to run the hamlet.
                let (mayor, purpose_at) =
                    match words.get(1).filter(|w| is_scholar(w)) {
                        Some(scholar) => {
                            match self.hiring(scholar, chief)? {
                                Ok(hiring) => (hiring, 2),
                                Err(refusal) => {
                                    return Ok(Err(refusal));
                                }
                            }
                        }
                        None => (
                            Hiring::New {
                                name: MAYOR.into(),
                                character: String::new(),
                            },
                            1,
                        ),
                    };
                let purpose = match tail(purpose_at) {
                    given if !given.trim().is_empty() => given,
                    _ => format!("Mayor of {id}."),
                };
                let done = self
                    .attempt(by, |cx, _| {
                        hamlets::found(
                            cx,
                            Founding {
                                id: id.clone(),
                                chief,
                                mayor,
                                purpose,
                            },
                        )
                    })
                    .await?;
                let db = self.realm.store().connection();
                Ok(match done {
                    Ok(hired) => Ok(format!(
                        "The hamlet `{id}` is founded under its \
                         mayor `{}`. /hamlet {id} talks to it.",
                        addresses::name_of(db, hired.agent)?
                    )),
                    Err(refusal) => Err(refusal),
                })
            }
            "name-hamlet" => {
                let Some(id) = words.first() else {
                    return Ok(Err(usage(name)));
                };
                let id = HamletId((*id).to_owned());
                if let Err(refusal) = self.chief_of(&id)? {
                    return Ok(Err(refusal));
                }
                let title = tail(1);
                let done = self
                    .attempt(by, |cx, _| {
                        hamlets::rename(
                            cx,
                            &id,
                            Some(title.as_str())
                                .filter(|t| !t.is_empty()),
                        )
                    })
                    .await?;
                Ok(done.map(|()| {
                    format!(
                        "The hamlet `{id}` is shown as {}.",
                        if title.is_empty() {
                            id.to_string()
                        } else {
                            title.clone()
                        }
                    )
                }))
            }
            "move-hamlet" => {
                let (Some(id), Some(x), Some(z)) = (
                    words.first(),
                    words.get(1).and_then(|w| w.parse::<f64>().ok()),
                    words.get(2).and_then(|w| w.parse::<f64>().ok()),
                ) else {
                    return Ok(Err(usage(name)));
                };
                let id = HamletId((*id).to_owned());
                if let Err(refusal) = self.chief_of(&id)? {
                    return Ok(Err(refusal));
                }
                let done = self
                    .attempt(by, |cx, _| {
                        hamlets::place(cx, &id, x, z)
                    })
                    .await?;
                Ok(done.map(|()| {
                    format!(
                        "The hamlet `{id}` is set down at {x}, {z}."
                    )
                }))
            }
            "remove-hamlet" | "restore-hamlet" | "erase-hamlet" => {
                let Some(id) = words.first() else {
                    return Ok(Err(usage(name)));
                };
                let id = HamletId((*id).to_owned());
                if let Err(refusal) = self.chief_of(&id)? {
                    return Ok(Err(refusal));
                }
                let reason = tail(1);
                let done = self
                    .attempt(by, |cx, _| match name {
                        "remove-hamlet" => {
                            hamlets::remove(cx, &id, &reason)
                        }
                        "restore-hamlet" => {
                            hamlets::restore(cx, &id, &reason)
                        }
                        _ => hamlets::erase(cx, &id),
                    })
                    .await?;
                Ok(done.map(|()| match name {
                    "remove-hamlet" => format!(
                        "The hamlet `{id}` was removed and its \
                         hires ended."
                    ),
                    "restore-hamlet" => {
                        format!("The hamlet `{id}` is restored.")
                    }
                    _ => format!(
                        "The hamlet `{id}` was erased for good."
                    ),
                }))
            }
            "tier" => {
                let Some(id) = here.clone() else {
                    return Ok(Err(self.no_hamlet()));
                };
                let Some(tier) =
                    words.first().and_then(|w| w.parse::<i64>().ok())
                else {
                    return Ok(Err(usage(name)));
                };
                if let Err(refusal) = self.chief_of(&id)? {
                    return Ok(Err(refusal));
                }
                let done = self
                    .attempt(by, |cx, _| {
                        hamlets::set_tier(cx, &id, tier)
                    })
                    .await?;
                Ok(done.map(|()| {
                    format!(
                        "The hamlet `{id}` holds at most {tier} \
                         hires."
                    )
                }))
            }
            "mayor" => {
                let Some(id) = here.clone() else {
                    return Ok(Err(self.no_hamlet()));
                };
                let Some(text) = words.first() else {
                    return Ok(Err(usage(name)));
                };
                if let Err(refusal) = self.chief_of(&id)? {
                    return Ok(Err(refusal));
                }
                let hire = match self.resolve(
                    text,
                    Some(&id),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                let reason = tail(1);
                let done = self
                    .attempt(by, |cx, _| {
                        hires::seat(cx, &id, hire, &reason)
                    })
                    .await?;
                Ok(done.map(|()| {
                    format!("`{text}` holds the office of `{id}`.")
                }))
            }
            "hire" => self.hire_command(here.as_ref(), &words).await,
            "end" | "restore" | "erase" => {
                let Some(text) = words.first() else {
                    return Ok(Err(usage(name)));
                };
                let reach = if name == "end" {
                    Reach::Living
                } else {
                    Reach::Kept
                };
                let hire =
                    match self.resolve(text, here.as_ref(), reach)? {
                        Ok(found) => found,
                        Err(refusal) => return Ok(Err(refusal)),
                    };
                if let Err(refusal) = self.leads(hire)? {
                    return Ok(Err(refusal));
                }
                let reason = tail(1);
                let db = self.realm.store().connection();
                // A merged hire comes back by splitting its merge.
                let merge = match name {
                    "restore" => {
                        crate::society::merges::of_absorbed(db, hire)?
                    }
                    _ => None,
                };
                let done = self
                    .attempt(by, |cx, _| match (name, &merge) {
                        ("end", _) => hires::end(
                            cx,
                            hire,
                            EndHow::Ended,
                            &reason,
                        ),
                        ("restore", Some(merge)) => {
                            crate::society::merges::split(
                                cx, merge.id, &reason,
                            )
                        }
                        ("restore", None) => {
                            hires::come_back(cx, hire, &reason)
                        }
                        _ => hires::erase(cx, hire),
                    })
                    .await?;
                Ok(done.map(|()| match name {
                    "end" => format!("`{text}` was ended."),
                    "restore" if merge.is_some() => format!(
                        "`{text}` is back; what it brought to the \
                         hire it was merged into is taken out again."
                    ),
                    "restore" => format!("`{text}` is back."),
                    _ => format!("`{text}` was erased for good."),
                }))
            }
            "merge" => {
                let (Some(absorbed), Some(&"into"), Some(kept)) =
                    (words.first(), words.get(1), words.get(2))
                else {
                    return Ok(Err(usage(name)));
                };
                let absorbed = match self.resolve(
                    absorbed,
                    here.as_ref(),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                let kept = match self.resolve(
                    kept,
                    here.as_ref(),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                for hire in [absorbed, kept] {
                    if let Err(refusal) = self.leads(hire)? {
                        return Ok(Err(refusal));
                    }
                }
                let reason = tail(3);
                let reason = if reason.is_empty() {
                    "merged by the chief".to_owned()
                } else {
                    reason
                };
                let done = self
                    .attempt(by, |cx, _| {
                        crate::society::merges::merge(
                            cx, kept, absorbed, &reason,
                        )
                    })
                    .await?;
                Ok(done.map(|_| {
                    format!(
                        "Merged `{}` into `{}`.",
                        words[0], words[2]
                    )
                }))
            }
            "model" => {
                let (Some(text), Some(model)) =
                    (words.first(), words.get(1))
                else {
                    return Ok(Err(usage(name)));
                };
                let hire = match self.resolve(
                    text,
                    here.as_ref(),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                if let Err(refusal) = self.steers(hire)? {
                    return Ok(Err(refusal));
                }
                let model = (*model != "none").then_some(*model);
                let reason = tail(2);
                let done = self
                    .attempt(by, |cx, _| {
                        agents::set_model(cx, hire, model, &reason)
                    })
                    .await?;
                Ok(done.map(|()| {
                    format!(
                        "`{text}` runs {}.",
                        model.unwrap_or("its default model")
                    )
                }))
            }
            "effort" | "fast" => {
                let (Some(text), Some(value)) =
                    (words.first(), words.get(1))
                else {
                    return Ok(Err(usage(name)));
                };
                let agent = match self.resolve(
                    text,
                    here.as_ref(),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                if let Err(refusal) = self.steers(agent)? {
                    return Ok(Err(refusal));
                }
                if name == "fast" {
                    let fast = match *value {
                        "on" => true,
                        "off" => false,
                        _ => return Ok(Err(usage(name))),
                    };
                    let done = self
                        .attempt(by, |cx, _| {
                            agents::set_fast(cx, agent, fast)
                        })
                        .await?;
                    return Ok(done.map(|()| {
                        format!(
                            "`{text}` asks for the fast tier: \
                             {value}."
                        )
                    }));
                }
                let effort = match *value {
                    "none" => None,
                    level => match Effort::named(level) {
                        Some(effort) => Some(effort),
                        None => return Ok(Err(usage(name))),
                    },
                };
                let done = self
                    .attempt(by, |cx, _| {
                        agents::set_effort(cx, agent, effort)
                    })
                    .await?;
                Ok(done.map(|()| match effort {
                    Some(effort) => {
                        format!("`{text}` thinks at {effort}.")
                    }
                    None => format!(
                        "`{text}` thinks as hard as the realm's \
                         setting says."
                    ),
                }))
            }
            "nickname" => {
                let Some(text) = words.first() else {
                    return Ok(Err(usage(name)));
                };
                let hire = match self.resolve(
                    text,
                    here.as_ref(),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                if let Err(refusal) = self.leads(hire)? {
                    return Ok(Err(refusal));
                }
                let nickname = tail(1);
                let given =
                    (!nickname.is_empty()).then_some(nickname);
                let done = self
                    .attempt(by, |cx, _| {
                        hires::set_nickname(
                            cx,
                            hire,
                            given.as_deref(),
                            "",
                        )
                    })
                    .await?;
                Ok(done.map(|()| match &given {
                    Some(nickname) => {
                        format!("`{text}` is called {nickname} now.")
                    }
                    None => {
                        format!("`{text}` goes by its name again.")
                    }
                }))
            }
            "tools" => {
                let Some(text) = words.first() else {
                    return Ok(Err(usage(name)));
                };
                let hire = match self.resolve(
                    text,
                    here.as_ref(),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                if let Err(refusal) = self.leads(hire)? {
                    return Ok(Err(refusal));
                }
                let off = words[1..]
                    .iter()
                    .filter_map(|w| w.strip_prefix('-'))
                    .map(String::from)
                    .collect::<Vec<_>>();
                let on = words[1..]
                    .iter()
                    .filter_map(|w| w.strip_prefix('+'))
                    .map(String::from)
                    .collect::<Vec<_>>();
                if off.is_empty() && on.is_empty() {
                    let db = self.realm.store().connection();
                    let kept = hires::tools_off(db, hire)?;
                    return Ok(Ok(if kept.is_empty() {
                        format!("`{text}` has every tool.")
                    } else {
                        format!(
                            "`{text}` has these off: {}",
                            kept.join(", ")
                        )
                    }));
                }
                let done = self
                    .attempt(by, |cx, _| {
                        hires::set_tools(cx, hire, &off, &on, "")
                    })
                    .await?;
                Ok(done.map(|()| {
                    format!("`{text}`'s tools are changed.")
                }))
            }
            "allowance" => {
                let (Some(text), Some(amount)) =
                    (words.first(), words.get(1))
                else {
                    return Ok(Err(usage(name)));
                };
                let hire = match self.resolve(
                    text,
                    here.as_ref(),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                if let Err(refusal) = self.leads(hire)? {
                    return Ok(Err(refusal));
                }
                let allowance = match *amount {
                    "none" => None,
                    amount => match amount
                        .trim_start_matches('$')
                        .parse::<f64>()
                    {
                        Ok(dollars) if dollars >= 0.0 => {
                            Some(spend::micros(dollars))
                        }
                        _ => return Ok(Err(usage(name))),
                    },
                };
                let done = self
                    .attempt(by, |cx, _| {
                        hires::set_allowance(cx, hire, allowance, "")
                    })
                    .await?;
                Ok(done.map(|()| match allowance {
                    Some(micros) => format!(
                        "`{text}` may spend {} a period.",
                        spend::money(micros)
                    ),
                    None => format!(
                        "`{text}` has no allowance of its own."
                    ),
                }))
            }
            "road" | "unroad" => {
                let (Some(a), Some(b)) =
                    (words.first(), words.get(1))
                else {
                    return Ok(Err(usage(name)));
                };
                let a = match self.resolve(
                    a,
                    here.as_ref(),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                let b = match self.resolve(
                    b,
                    here.as_ref(),
                    Reach::Living,
                )? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                for end in [a, b] {
                    if let Err(refusal) = self.leads(end)? {
                        return Ok(Err(refusal));
                    }
                }
                let reason = tail(2);
                if name == "road" {
                    let done = self
                        .attempt(by, |cx, _| {
                            roads::lay(cx, a, b, &reason, None)
                        })
                        .await?;
                    Ok(done.map(|_| "The road is laid.".into()))
                } else {
                    let done = self
                        .attempt(by, |cx, _| {
                            roads::take_up(cx, a, b, &reason)
                        })
                        .await?;
                    Ok(done.map(|()| "The road is taken up.".into()))
                }
            }
            "folder" | "mode" => {
                let Some(id) = here.clone() else {
                    return Ok(Err(self.no_hamlet()));
                };
                if let Err(refusal) = self.chief_of(&id)? {
                    return Ok(Err(refusal));
                }
                if name == "folder" {
                    let folder = match words.first() {
                        None => return Ok(Err(usage(name))),
                        Some(&"none") => None,
                        Some(_) => Some(rest.to_owned()),
                    };
                    if let Some(folder) = &folder
                        && !std::path::Path::new(folder).is_dir()
                    {
                        return Ok(Err(Refusal::NoFile {
                            path: folder.clone(),
                        }));
                    }
                    let done = self
                        .attempt(by, |cx, _| {
                            hamlets::set_folder(
                                cx,
                                &id,
                                folder.as_deref(),
                            )
                        })
                        .await?;
                    Ok(done.map(|()| match &folder {
                        Some(folder) => {
                            format!("`{id}` works on {folder}.")
                        }
                        None => format!("`{id}` works on no folder."),
                    }))
                } else {
                    let mode = match words.first() {
                        Some(&"manual") => Mode::Manual,
                        Some(&"auto") => Mode::Auto,
                        _ => return Ok(Err(usage(name))),
                    };
                    let done = self
                        .attempt(by, |cx, _| {
                            hamlets::set_mode(cx, &id, mode)
                        })
                        .await?;
                    Ok(done.map(|()| {
                        format!("`{id}` is in {mode} mode.")
                    }))
                }
            }
            "budget" => {
                let db = self.realm.store().connection();
                let report = spend::report(
                    db,
                    here.as_ref(),
                    crate::clock::now(),
                )?;
                Ok(Ok(report.summary))
            }
            "reset-budget" => {
                if let Some(id) = &here
                    && let Err(refusal) = self.chief_of(id)?
                {
                    return Ok(Err(refusal));
                }
                let at = crate::clock::now();
                let db = self.realm.store().connection();
                spend::reset(db, here.as_ref(), chief, at)?;
                self.publish().await?;
                Ok(Ok("The budget's period starts again from now."
                    .into()))
            }
            "record" => {
                let Some(id) = here.clone() else {
                    return Ok(Err(self.no_hamlet()));
                };
                let db = self.realm.store().connection();
                Ok(Ok(record::lines(db, &id, 0)?))
            }
            "scholars" => {
                let db = self.realm.store().connection();
                let list = city::all(db)?
                    .into_iter()
                    .map(|s| {
                        format!(
                            "- {}{}",
                            s.label(),
                            if s.is_living() {
                                ""
                            } else {
                                " (archived)"
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(Ok(if list.is_empty() {
                    "The city has no scholar yet.".into()
                } else {
                    list
                }))
            }
            "scholar" => {
                let Some(&"new") = words.first() else {
                    return Ok(Err(usage(name)));
                };
                // The name runs to the first colon, the character
                // follows it.
                let (scholar, character) =
                    tail(1).split_once(':').map_or_else(
                        || (tail(1), String::new()),
                        |(scholar, character)| {
                            (
                                scholar.to_owned(),
                                character.trim().to_owned(),
                            )
                        },
                    );
                let done = self
                    .attempt(by, |cx, _| {
                        city::make(
                            cx, &scholar, chief, &character, None,
                        )
                    })
                    .await?;
                let db = self.realm.store().connection();
                Ok(match done {
                    Ok(id) => Ok(format!(
                        "The scholar `{}` lives in the city.",
                        addresses::scholar_name_of(db, id)?
                    )),
                    Err(refusal) => Err(refusal),
                })
            }
            "name-scholar" => {
                let Some(text) = words.first() else {
                    return Ok(Err(usage(name)));
                };
                let scholar = match self.scholar_at(text, false)? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                {
                    let db = self.realm.store().connection();
                    if !authority::may_keep(db, by, scholar)? {
                        return Ok(Err(Refusal::NotOwner {
                            owner: addresses::scholar_name_of(
                                db, scholar,
                            )?,
                        }));
                    }
                }
                let new = tail(1);
                let done = self
                    .attempt(by, |cx, _| {
                        city::rename(cx, scholar, &new)
                    })
                    .await?;
                let db = self.realm.store().connection();
                Ok(match done {
                    Ok(()) => Ok(format!(
                        "The scholar is now named `{}`.",
                        addresses::scholar_name_of(db, scholar)?
                    )),
                    Err(refusal) => Err(refusal),
                })
            }
            "remove-scholar" | "restore-scholar"
            | "erase-scholar" | "move-house" | "welcome"
            | "turn-away" => {
                let Some(text) = words.first() else {
                    return Ok(Err(usage(name)));
                };
                let archived = matches!(
                    name,
                    "restore-scholar" | "erase-scholar"
                );
                let scholar = match self.scholar_at(text, archived)? {
                    Ok(found) => found,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                let text = {
                    let db = self.realm.store().connection();
                    let shown =
                        addresses::scholar_name_of(db, scholar)?;
                    if !authority::may_keep(db, by, scholar)? {
                        return Ok(Err(Refusal::NotOwner {
                            owner: shown,
                        }));
                    }
                    shown
                };
                let reason = tail(1);
                if name == "move-house" {
                    let number = |at: usize| {
                        words.get(at).map(|w| w.parse::<f64>().ok())
                    };
                    let (Some(Some(x)), Some(Some(z)), turn) =
                        (number(1), number(2), number(3))
                    else {
                        return Ok(Err(usage(name)));
                    };
                    let Some(turn) = turn.map_or(Some(None), |t| {
                        t.map(|degrees| Some(degrees.to_radians()))
                    }) else {
                        return Ok(Err(usage(name)));
                    };
                    let done = self
                        .attempt(by, |cx, _| {
                            city::place(cx, scholar, x, z, turn)
                        })
                        .await?;
                    return Ok(done.map(|()| {
                        format!(
                            "`{text}`'s house stands at {x}, {z}."
                        )
                    }));
                }
                let done = self
                    .attempt(by, |cx, _| match name {
                        "remove-scholar" => {
                            city::archive(cx, scholar, &reason)
                        }
                        "restore-scholar" => {
                            city::restore(cx, scholar, &reason)
                        }
                        "welcome" => {
                            city::welcome(cx, scholar, &reason)
                        }
                        "turn-away" => {
                            city::turn_away(cx, scholar, &reason)
                        }
                        _ => city::erase(cx, scholar),
                    })
                    .await?;
                Ok(done.map(|()| match name {
                    "remove-scholar" => format!(
                        "`{text}` left the city, with its hires."
                    ),
                    "restore-scholar" => {
                        format!("`{text}` is back in the city.")
                    }
                    "welcome" => {
                        format!(
                            "`{text}` moved into its house in the \
                             city."
                        )
                    }
                    "turn-away" => {
                        format!("`{text}` waits at the harbor.")
                    }
                    _ => format!("`{text}` was erased for good."),
                }))
            }
            _ => Ok(Err(Refusal::Usage {
                words: format!(
                    "There is no command /{name}; the client lists \
                     the commands."
                ),
            })),
        }
    }

    async fn hire_command(
        &mut self,
        here: Option<&HamletId>,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let chief = self.realm.chief();
        let Some(scholar) = words.first() else {
            return Ok(Err(usage("hire")));
        };
        let mut rest = &words[1..];
        let mut hamlet = here.cloned();
        let mut parent = None;
        while let [key, value, more @ ..] = rest {
            match *key {
                "under" => {
                    match self.resolve(
                        value,
                        hamlet.as_ref(),
                        Reach::Living,
                    )? {
                        Ok(found) => parent = Some(found),
                        Err(refusal) => return Ok(Err(refusal)),
                    }
                }
                "in" => hamlet = Some(HamletId((*value).to_owned())),
                _ => break,
            }
            rest = more;
        }
        let Some(hamlet) = hamlet else {
            return Ok(Err(self.no_hamlet()));
        };
        if let Err(refusal) = self.chief_of(&hamlet)? {
            return Ok(Err(refusal));
        }
        let purpose = rest.join(" ");
        let parent = match parent {
            Some(parent) => parent,
            None => {
                let db = self.realm.store().connection();
                match hamlets::living(db, &hamlet)? {
                    Ok(place) => place.mayor,
                    Err(refusal) => return Ok(Err(refusal)),
                }
            }
        };
        let hiring = match self.hiring(scholar, chief)? {
            Ok(hiring) => hiring,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let done = self
            .attempt(Actor::Chief(chief), |cx, _| {
                hires::hire(
                    cx,
                    Spec {
                        hamlet: hamlet.clone(),
                        parent: Some(parent),
                        hiring,
                        purpose,
                        reason: String::new(),
                        owner: chief,
                    },
                )
            })
            .await?;
        let db = self.realm.store().connection();
        Ok(match done {
            Ok(hired) => Ok(format!(
                "`{}` is hired.",
                addresses::name_of(db, hired.agent)?
            )),
            Err(refusal) => Err(refusal),
        })
    }

    /// The scholar `text` addresses: a living one of the chief's, at
    /// its `main` or pinned to `@version`.
    fn hiring(
        &mut self,
        text: &str,
        chief: hamlet_store::ids::ChiefId,
    ) -> Result<Result<Hiring, Refusal>> {
        let (scholar, version) = match self.scholar_pinned(text)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        if city::get(db, scholar)?.chief != Some(chief) {
            return Ok(Err(Refusal::ScholarNotOwn {
                name: addresses::scholar_name_of(db, scholar)?,
            }));
        }
        self.existing(scholar, version)
    }

    /// The living scholar `text` addresses, `scholar#<id>` with
    /// `@<version>` to pin one, and that version.
    pub(crate) fn scholar_pinned(
        &mut self,
        text: &str,
    ) -> Result<Result<(ScholarId, Option<String>), Refusal>> {
        let (address, version) = match text.split_once('@') {
            Some((address, version)) => {
                (address, Some(version.to_owned()))
            }
            None => (text, None),
        };
        Ok(self
            .scholar_at(address, false)?
            .map(|scholar| (scholar, version)))
    }

    /// A hire of the living `scholar`, at its `main` or pinned to
    /// `version` when that is one the scholar has.
    pub(crate) fn existing(
        &mut self,
        scholar: ScholarId,
        version: Option<String>,
    ) -> Result<Result<Hiring, Refusal>> {
        if let Some(version) = &version {
            let found = Repository::open(
                &self.realm.layout().repository(scholar),
            )
            .ok()
            .and_then(|repo| repo.resolve(version).ok().flatten());
            if found.is_none() || version == hamlet_git::MAIN {
                let db = self.realm.store().connection();
                return Ok(Err(Refusal::NoVersion {
                    name: addresses::scholar_name_of(db, scholar)?,
                    version: version.clone(),
                }));
            }
        }
        Ok(Ok(Hiring::Existing {
            scholar,
            pin: version,
        }))
    }

    /// The agent `text` addresses, said in `here`.
    fn resolve(
        &mut self,
        text: &str,
        here: Option<&HamletId>,
        reach: Reach,
    ) -> Result<Result<AgentId, Refusal>> {
        let address = match addresses::parse(text) {
            Ok(address) => address,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        addresses::resolve(db, &address, here, reach)
    }

    /// The scholar `text` addresses, among the living, or the
    /// archived too when `archived`.
    pub(crate) fn scholar_at(
        &mut self,
        text: &str,
        archived: bool,
    ) -> Result<Result<ScholarId, Refusal>> {
        let id = match addresses::parse_scholar(text) {
            Ok(id) => id,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        let found = city::find(db, id)?.filter(|found| {
            found.kind == city::ScholarKind::Scholar
                && (archived || found.is_living())
        });
        Ok(match found {
            Some(found) => Ok(found.id),
            None => Err(Refusal::NoScholar {
                address: text.into(),
            }),
        })
    }

    /// Refuses unless this server's chief is a chief of `hamlet`.
    fn chief_of(
        &mut self,
        hamlet: &HamletId,
    ) -> Result<Result<(), Refusal>> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if hamlets::find(db, hamlet)?.is_none() {
            return Ok(Err(Refusal::NoHamlet {
                id: hamlet.to_string(),
            }));
        }
        if !authority::is_chief_of(db, chief, hamlet)? {
            return Ok(Err(Refusal::NotChief {
                hamlet: hamlet.to_string(),
            }));
        }
        Ok(Ok(()))
    }

    /// Refuses unless this server's chief leads the hire `hire`.
    pub(crate) fn leads(
        &mut self,
        hire: AgentId,
    ) -> Result<Result<(), Refusal>> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if agents::get(db, hire)?.kind != AgentKind::Hire {
            let name = addresses::name_of(db, hire)?;
            return Ok(Err(Refusal::NotHire { name }));
        }
        if !authority::may_lead(db, Actor::Chief(chief), hire)? {
            let name = addresses::name_of(db, hire)?;
            return Ok(Err(Refusal::NotYours { name }));
        }
        Ok(Ok(()))
    }

    /// Refuses unless this server's chief may choose the model,
    /// effort and fast tier of `agent`.
    fn steers(
        &mut self,
        agent: AgentId,
    ) -> Result<Result<(), Refusal>> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if !authority::may_steer(db, Actor::Chief(chief), agent)? {
            let name = addresses::name_of(db, agent)?;
            return Ok(Err(Refusal::NotYours { name }));
        }
        Ok(Ok(()))
    }

    fn no_hamlet(&self) -> Refusal {
        Refusal::Usage {
            words: "Say this to a hamlet's mayor or one of its \
                    hires."
                .into(),
        }
    }

    fn every_reached(&mut self) -> Result<Vec<AgentId>> {
        use diesel::prelude::*;
        use hamlet_store::schema::agent;
        let living = agent::table
            .filter(agent::ended_at.is_null())
            .select(agent::id)
            .load::<AgentId>(self.realm.store().connection())?;
        let mut out = Vec::new();
        for each in living {
            if self.reaches(each)?.is_ok() {
                out.push(each);
            }
        }
        Ok(out)
    }
}
