//! The gate every tool call passes before it runs (C7, H11): what it
//! reaches, what the chief's mode, grants and trust allow, and what
//! the chief refused in the turn.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::harness::BindingId;
use crate::record::AgentRef;

/// A tool call, classified by its reach.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Call {
    Shell {
        command: String,
        net: bool,
    },
    Edit {
        write: bool,
        path: String,
        old: String,
        now: String,
        created: bool,
    },
    Read,
    Society {
        name: String,
        args: Value,
    },
    Other {
        name: String,
        args: Value,
    },
}

/// The society tools the server answers itself.
const SOCIETY: &[&str] = &[
    "create_agent",
    "build_road",
    "set_model",
    "remove_agent",
    "merge_agents",
    "agree_road",
    "agree_move",
    "ask_removal",
    "tell_agent",
    "ask_agent",
    "call_vote",
    "submit_skill",
    "ask_librarian",
    "request_road",
    "object",
    "ask_chief",
    "cast_ballot",
];

/// The call of `tool` with `args`. A write's old text is read apart.
pub(crate) fn call_of(tool: &str, args: &Value) -> Call {
    let text = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match tool {
        "bash" => Call::Shell {
            command: text("command"),
            net: args.get("network") == Some(&Value::Bool(true)),
        },
        "write" => Call::Edit {
            write: true,
            path: text("path"),
            old: String::new(),
            now: text("content"),
            created: true,
        },
        "edit" => {
            let edits = args
                .get("edits")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let part = |key: &str| {
                edits
                    .iter()
                    .map(|edit| {
                        edit.get(key)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let old = part("oldText");
            Call::Edit {
                write: false,
                path: text("path"),
                created: old.is_empty(),
                old,
                now: part("newText"),
            }
        }
        "read" | "search" | "find" => Call::Read,
        _ if SOCIETY.contains(&tool) => Call::Society {
            name: tool.into(),
            args: args.clone(),
        },
        _ => Call::Other {
            name: tool.into(),
            args: args.clone(),
        },
    }
}

/// The bounded subject a chief sees when an approval waits.
pub(crate) fn call_text(call: &Call) -> String {
    match call {
        Call::Shell { command, .. } => command.clone(),
        Call::Edit {
            path,
            old,
            now,
            created,
            ..
        } => {
            if *created {
                format!("{}\nA new file:\n{}", cut(path), cut(now))
            } else {
                format!(
                    "{}\nReplace:\n{}\nWith:\n{}",
                    cut(path),
                    cut(old),
                    cut(now)
                )
            }
        }
        Call::Society { name, args } | Call::Other { name, args } => {
            format!("{name} {args}")
        }
        Call::Read => String::new(),
    }
}

fn cut(text: &str) -> String {
    const SHOWN: usize = 2000;
    let count = text.chars().count();
    if count <= SHOWN {
        return text.into();
    }
    let end = text
        .char_indices()
        .nth(SHOWN)
        .map_or(text.len(), |(i, _)| i);
    format!("{}\n[{} more characters]", &text[..end], count - SHOWN)
}

/// The permissions and mode in force at a call.
#[derive(Clone, Debug, Default)]
pub(crate) struct Standing {
    pub house_edit: bool,
    pub net: bool,
    pub asks_edits: bool,
    pub asks_commands: bool,
}

/// What a call may do: run, with or without the network; wait for a
/// chief; or not run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Verdict {
    Run { net: bool },
    Ask(&'static str),
    Block(String),
}

/// Commands that reach the network, by the words they name.
const NETWORK: &[&str] = &[
    "curl",
    "wget",
    "http://",
    "https://",
    "ssh",
    "scp",
    "rsync",
    "ftp",
    "ping",
    "nc",
    "git clone",
    "git fetch",
    "git pull",
    "git push",
    "pip install",
    "pip3 install",
    "npm install",
    "cargo install",
    "brew install",
];

/// The H11 decision, ordered by reach, reserved acts and the turn's
/// refusals.
pub(crate) fn rule(
    call: &Call,
    standing: &Standing,
    asked: &Asked,
    by: &str,
    about: &str,
    limits: &Value,
    mayor: bool,
) -> Verdict {
    let verdict = if mayor && mayor_reserved(call, limits) {
        Verdict::Ask("reserved")
    } else {
        reach(call, standing)
    };
    let Verdict::Ask(danger) = verdict else {
        return verdict;
    };
    let cap = limits["asking"]["refusals"].as_i64().unwrap_or(3);
    match refused(asked, by, danger, about, cap) {
        Some(words) => Verdict::Block(words),
        None => verdict,
    }
}

/// A society act the chief's limits reserve to the chief.
fn mayor_reserved(call: &Call, limits: &Value) -> bool {
    let Call::Society { name, args } = call else {
        return false;
    };
    let allows =
        |key: &str| limits["mayor"][key] != Value::Bool(false);
    match name.as_str() {
        "create_agent" => {
            !allows("createAgents")
                || (!allows("buildRoads")
                    && args["roads"]
                        .as_array()
                        .is_some_and(|roads| !roads.is_empty()))
        }
        "build_road" => !allows("buildRoads"),
        "remove_agent" => !allows("removeAgents"),
        "merge_agents" => !allows("mergeAgents"),
        _ => false,
    }
}

fn reach(call: &Call, standing: &Standing) -> Verdict {
    match call {
        Call::Read | Call::Society { .. } | Call::Other { .. } => {
            Verdict::Run { net: false }
        }
        Call::Edit { .. } => {
            if standing.asks_edits && !standing.house_edit {
                Verdict::Ask("edit")
            } else {
                Verdict::Run { net: false }
            }
        }
        Call::Shell { command, net } => {
            if !standing.net && (*net || networked(command)) {
                Verdict::Ask("network")
            } else if standing.asks_commands {
                Verdict::Ask("command")
            } else {
                Verdict::Run { net: standing.net }
            }
        }
    }
}

fn networked(command: &str) -> bool {
    let lower = command.to_lowercase();
    NETWORK.iter().any(|needle| mentions(&lower, needle))
}

/// Whether `command` names `needle`: a word as a whole word, other
/// text anywhere.
fn mentions(command: &str, needle: &str) -> bool {
    let word =
        |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    if needle.chars().all(word) {
        command.split(|c| !word(c)).any(|part| part == needle)
    } else {
        command.contains(needle)
    }
}

/// The refusals and questions of a conversation's current turn.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Asked {
    pub input: Option<String>,
    pub strikes: i64,
    pub questions: i64,
    pub refused: Vec<Value>,
}

impl Asked {
    /// What `binding` was asked in its turn of `input`.
    pub fn of(
        db: &Connection,
        binding: BindingId,
        input: Option<&str>,
    ) -> Result<Self> {
        let kept = db
            .query_row(
                "SELECT input, strikes, questions, refused FROM \
                 gate_asked WHERE binding = ?1",
                [binding.0],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        Ok(match kept {
            Some((kept, strikes, questions, refused))
                if kept.as_deref() == input =>
            {
                Self {
                    input: kept,
                    strikes,
                    questions,
                    refused: serde_json::from_str(&refused)?,
                }
            }
            _ => Self {
                input: input.map(String::from),
                ..Self::default()
            },
        })
    }

    pub fn keep(
        &self,
        db: &Connection,
        binding: BindingId,
    ) -> Result<()> {
        db.execute(
            "INSERT INTO gate_asked (binding, input, strikes, \
             questions, refused) VALUES (?1, ?2, ?3, ?4, ?5) ON \
             CONFLICT (binding) DO UPDATE SET input = \
             excluded.input, strikes = excluded.strikes, questions \
             = excluded.questions, refused = excluded.refused",
            params![
                binding.0,
                self.input,
                self.strikes,
                self.questions,
                serde_json::to_string(&self.refused)?
            ],
        )?;
        Ok(())
    }

    /// Keeps the chief's refusal of what `by` asked.
    pub fn refuse(
        &mut self,
        by: &str,
        danger: &str,
        about: &str,
        reason: &str,
    ) {
        self.strikes += 1;
        self.refused.push(json!({
            "by": by,
            "what": refusal(danger, about),
            "reason": reason,
        }));
    }
}

fn refusal(danger: &str, about: &str) -> Value {
    if danger == "network" || danger == "unconfined" {
        json!({"kind": danger})
    } else {
        json!({"exact": about})
    }
}

/// The denial of a request repeated in a turn, or of one past the
/// cap.
fn refused(
    asked: &Asked,
    by: &str,
    danger: &str,
    about: &str,
    cap: i64,
) -> Option<String> {
    let what = refusal(danger, about);
    if let Some(earlier) = asked
        .refused
        .iter()
        .find(|entry| entry["by"] == by && entry["what"] == what)
    {
        return Some(format!(
            "the chief already refused this in this turn ({}). Do \
             not ask again: go on without it, or report that you \
             could not.",
            earlier["reason"].as_str().unwrap_or_default()
        ));
    }
    (asked.strikes >= cap).then(|| {
        format!(
            "the chief has refused {} requests this turn and will \
             not be asked again in it. Stop and report what you \
             could not do.",
            asked.strikes
        )
    })
}

/// The chat session whose edits and commands a chief may trust.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrustKey<'a> {
    pub agent: &'a AgentRef,
    pub chief: &'a str,
    pub session: i64,
}

/// Whether the chief trusts the edits and the commands of the
/// session.
pub(crate) fn trusted(
    db: &Connection,
    key: &TrustKey,
) -> Result<(bool, bool)> {
    Ok(db
        .query_row(
            "SELECT edit, command FROM gate_trust WHERE hamlet = ?1 \
             AND number = ?2 AND chief = ?3 AND session = ?4",
            params![
                key.agent.hamlet,
                i64::try_from(key.agent.number)?,
                key.chief,
                key.session
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .unwrap_or((false, false)))
}

/// Trusts the edits or the commands of the session from now on.
pub(crate) fn trust(
    db: &Connection,
    key: &TrustKey,
    danger: &str,
) -> Result<()> {
    let (edit, command) = (danger == "edit", danger == "command");
    db.execute(
        "INSERT INTO gate_trust (hamlet, number, chief, session, \
         edit, command) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT \
         (hamlet, number, chief, session) DO UPDATE SET edit = edit \
         OR excluded.edit, command = command OR excluded.command",
        params![
            key.agent.hamlet,
            i64::try_from(key.agent.number)?,
            key.chief,
            key.session,
            edit,
            command
        ],
    )?;
    Ok(())
}

/// Takes back the trust of a session that is resumed (H11).
pub(crate) fn reset_trust(
    db: &Connection,
    key: &TrustKey,
) -> Result<()> {
    db.execute(
        "DELETE FROM gate_trust WHERE hamlet = ?1 AND number = ?2 \
         AND chief = ?3 AND session = ?4",
        params![
            key.agent.hamlet,
            i64::try_from(key.agent.number)?,
            key.chief,
            key.session
        ],
    )?;
    Ok(())
}

/// A lasting permission kept by the agent's number (C9, H11).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Grant {
    pub agent: AgentRef,
    pub name: String,
    pub permission: String,
    pub by: String,
}

/// The grants still held, after the latest event for each agent and
/// permission.
pub(crate) fn grants(db: &Connection) -> Result<Vec<Grant>> {
    let mut query = db.prepare(
        "SELECT type, hamlet, number, name, permission, by FROM \
         grant_event ORDER BY ordinal",
    )?;
    let events = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut held = Vec::<Grant>::new();
    for (kind, hamlet, number, name, permission, by) in events {
        let agent = AgentRef {
            hamlet,
            number: u64::try_from(number)?,
        };
        held.retain(|grant| {
            !(grant.agent == agent && grant.permission == permission)
        });
        if kind == "granted" {
            held.push(Grant {
                agent,
                name,
                permission,
                by,
            });
        }
    }
    Ok(held)
}

/// Whether `agent`, called `name`, has `permission` by the limits or
/// a grant.
pub(crate) fn holds(
    limits: &Value,
    grants: &[Grant],
    name: &str,
    agent: &AgentRef,
    permission: &str,
) -> bool {
    limits["permissions"][permission]
        .as_array()
        .is_some_and(|names| names.iter().any(|n| n == name))
        || grants.iter().any(|grant| {
            &grant.agent == agent && grant.permission == permission
        })
}

/// Gives `agent`, called `name`, `permission` from the chief `by`.
pub(crate) fn give(
    db: &Connection,
    agent: &AgentRef,
    name: &str,
    permission: &str,
    by: &str,
    at: i64,
) -> Result<()> {
    event(db, "granted", agent, name, permission, by, at)
}

/// Takes `permission` back from `agent`, when it holds it by a grant.
pub(crate) fn revoke(
    db: &Connection,
    agent: &AgentRef,
    permission: &str,
    by: &str,
    at: i64,
) -> Result<bool> {
    let Some(grant) = grants(db)?.into_iter().find(|grant| {
        &grant.agent == agent && grant.permission == permission
    }) else {
        return Ok(false);
    };
    event(db, "revoked", agent, &grant.name, permission, by, at)?;
    Ok(true)
}

fn event(
    db: &Connection,
    kind: &str,
    agent: &AgentRef,
    name: &str,
    permission: &str,
    by: &str,
    at: i64,
) -> Result<()> {
    db.execute(
        "INSERT INTO grant_event (type, hamlet, number, name, \
         permission, by, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            kind,
            agent.hamlet,
            i64::try_from(agent.number)?,
            name,
            permission,
            by,
            at
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_reaches_the_network_by_the_words_it_names() {
        assert!(networked("echo curl > network"));
        assert!(networked("git clone x"));
        assert!(!networked("echo curly"));
        assert!(!networked("echo ran > never"));
    }

    #[test]
    fn a_refusal_is_not_asked_again_in_its_turn() {
        let mut asked = Asked::default();
        asked.refuse("mayor", "network", "echo curl", "no network");
        assert!(
            refused(&asked, "mayor", "network", "echo wget", 3)
                .unwrap()
                .contains("(no network)")
        );
        assert_eq!(
            refused(&asked, "mayor", "command", "ls", 3),
            None
        );
        assert!(
            refused(&asked, "mayor", "command", "ls", 1).is_some()
        );
    }
}
