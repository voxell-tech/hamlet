//! The gate every tool call passes before it runs (C7, H11): what it
//! reaches, what the chief's mode, grants and trust allow, what the
//! chief keeps for itself, and what the chief refused in the turn.

use anyhow::Result;
use diesel::prelude::*;
use diesel::upsert::excluded;
use hamlet_protocol::client::Permission;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ChiefId, ConversationId};
use hamlet_store::schema::{permission_grant, trust, turn_gate};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Danger;
use crate::society::limits::Reserved;
use crate::work::catalogue::SOCIETY;
use crate::work::tools::Dialect;

/// A tool call, classified by its reach.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Call {
    Shell {
        dialect: Dialect,
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
    /// A tool added by hand (C17), with whether its manifest reaches
    /// the network.
    Added {
        name: String,
        args: Value,
        net: bool,
    },
    Other {
        name: String,
        args: Value,
    },
}

/// The call of `tool` with `args`; `added` when it is a tool added by
/// hand, with its network. A write's old text is read apart.
pub(crate) fn call_of(
    tool: &str,
    args: &Value,
    added: Option<bool>,
) -> Call {
    let text = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    if let Some(net) = added {
        return Call::Added {
            name: tool.into(),
            args: args.clone(),
            net,
        };
    }
    if let Some(dialect) = Dialect::of_tool(tool) {
        return Call::Shell {
            dialect,
            command: text("command"),
            net: args.get("network") == Some(&Value::Bool(true)),
        };
    }
    match tool {
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
        Call::Society { name, args }
        | Call::Added { name, args, .. }
        | Call::Other { name, args } => format!("{name} {args}"),
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

/// Who makes a call, as the gate weighs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Caller {
    Hand,
    Hire,
    Worker,
}

/// The permissions and mode in force at a call.
#[derive(Clone, Debug, Default)]
pub(crate) struct Standing {
    /// The edit stays in the agent's own lodge, which is always free
    /// (C21).
    pub lodge_edit: bool,
    pub net: bool,
    pub asks_edits: bool,
    pub asks_commands: bool,
}

/// What a call may do: run, with or without the network; wait for a
/// chief; or not run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Verdict {
    Run { net: bool },
    Ask(Danger),
    Block(String),
}

/// Commands that reach the network in every shell, by the words they
/// name.
const COMMON_NETWORK: &[&str] = &[
    "curl",
    "wget",
    "http://",
    "https://",
    "ssh",
    "scp",
    "ftp",
    "ping",
    "git clone",
    "git fetch",
    "git pull",
    "git push",
    "pip install",
    "pip3 install",
    "npm install",
    "cargo install",
];

/// The Hand's acts that always wait for a chief.
const HAND_RESERVED: &[&str] =
    &["remove_hamlet", "edit_scholar", "tag", "revert"];

/// The H11 decision, ordered by reach, reserved acts and the turn's
/// refusals.
pub(crate) fn rule(
    call: &Call,
    standing: &Standing,
    caller: Caller,
    reserved: &Reserved,
    asked: &Asked,
    about: &str,
    refusals: i64,
) -> Verdict {
    let verdict = if is_reserved(call, caller, reserved) {
        Verdict::Ask(Danger::Reserved)
    } else {
        reach(call, standing)
    };
    let Verdict::Ask(danger) = verdict else {
        return verdict;
    };
    match refused(asked, danger, about, refusals) {
        Some(words) => Verdict::Block(words),
        None => verdict,
    }
}

/// A society act the chief keeps for itself (H22): a lead's acts its
/// hamlet's limits reserve, and the Hand's acts on what chiefs own.
fn is_reserved(
    call: &Call,
    caller: Caller,
    reserved: &Reserved,
) -> bool {
    let Call::Society { name, .. } = call else {
        return false;
    };
    match caller {
        Caller::Hand => HAND_RESERVED.contains(&name.as_str()),
        Caller::Hire => match name.as_str() {
            "hire" => reserved.hire,
            "lay_road" | "take_up_road" => reserved.roads,
            "end_hire" => reserved.end,
            "merge_hires" => reserved.merge,
            _ => false,
        },
        Caller::Worker => false,
    }
}

fn reach(call: &Call, standing: &Standing) -> Verdict {
    match call {
        Call::Read | Call::Society { .. } | Call::Other { .. } => {
            Verdict::Run { net: false }
        }
        Call::Edit { .. } => {
            if standing.asks_edits && !standing.lodge_edit {
                Verdict::Ask(Danger::Edit)
            } else {
                Verdict::Run { net: false }
            }
        }
        Call::Shell { net, .. } | Call::Added { net, .. } => {
            let named = match call {
                Call::Shell {
                    dialect, command, ..
                } => is_networked(*dialect, command),
                _ => false,
            };
            if !standing.net && (*net || named) {
                Verdict::Ask(Danger::Network)
            } else if standing.asks_commands {
                Verdict::Ask(Danger::Command)
            } else {
                Verdict::Run { net: standing.net }
            }
        }
    }
}

fn is_networked(dialect: Dialect, command: &str) -> bool {
    let lower = command.to_lowercase();
    COMMON_NETWORK
        .iter()
        .chain(dialect.network_words())
        .any(|needle| has_mention(&lower, needle))
}

/// Whether `command` names `needle`: a word as a whole word, other
/// text anywhere.
fn has_mention(command: &str, needle: &str) -> bool {
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
    pub refused: Vec<Refused>,
}

/// A request the chief refused in a turn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Refused {
    pub what: Covers,
    pub reason: String,
}

/// What a refusal covers: a reach of every call, or one exact
/// request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Covers {
    Kind(Danger),
    Exact(String),
}

impl Covers {
    fn of(danger: Danger, about: &str) -> Self {
        if danger == Danger::Network {
            Self::Kind(danger)
        } else {
            Self::Exact(about.into())
        }
    }
}

impl Asked {
    /// What `conversation` was asked in its turn of `input`.
    pub fn of(
        db: &mut Db,
        conversation: ConversationId,
        input: Option<&str>,
    ) -> Result<Self> {
        let kept = turn_gate::table
            .find(conversation)
            .select((
                turn_gate::input,
                turn_gate::strikes,
                turn_gate::questions,
                turn_gate::refused,
            ))
            .first::<(Option<String>, i64, i64, String)>(db)
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
        db: &mut Db,
        conversation: ConversationId,
    ) -> Result<()> {
        diesel::insert_into(turn_gate::table)
            .values((
                turn_gate::conversation.eq(conversation),
                turn_gate::input.eq(&self.input),
                turn_gate::strikes.eq(self.strikes),
                turn_gate::questions.eq(self.questions),
                turn_gate::refused
                    .eq(serde_json::to_string(&self.refused)?),
            ))
            .on_conflict(turn_gate::conversation)
            .do_update()
            .set((
                turn_gate::input.eq(excluded(turn_gate::input)),
                turn_gate::strikes.eq(excluded(turn_gate::strikes)),
                turn_gate::questions
                    .eq(excluded(turn_gate::questions)),
                turn_gate::refused.eq(excluded(turn_gate::refused)),
            ))
            .execute(db)?;
        Ok(())
    }

    /// Keeps the chief's refusal of what was asked.
    pub fn refuse(
        &mut self,
        danger: Danger,
        about: &str,
        reason: &str,
    ) {
        self.strikes += 1;
        self.refused.push(Refused {
            what: Covers::of(danger, about),
            reason: reason.into(),
        });
    }
}

/// The denial of a request repeated in a turn, or of one past the
/// cap.
fn refused(
    asked: &Asked,
    danger: Danger,
    about: &str,
    cap: i64,
) -> Option<String> {
    let what = Covers::of(danger, about);
    if let Some(earlier) =
        asked.refused.iter().find(|entry| entry.what == what)
    {
        return Some(format!(
            "the chief already refused this in this turn ({}). Do \
             not ask again: go on without it, or report that you \
             could not.",
            earlier.reason
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Session {
    pub agent: AgentId,
    pub chief: ChiefId,
    pub session: i64,
}

/// Whether the chief trusts the edits and the commands of `session`.
pub(crate) fn trusted(
    db: &mut Db,
    session: Session,
) -> Result<(bool, bool)> {
    Ok(trust::table
        .find((session.agent, session.chief, session.session))
        .select((trust::edit, trust::command))
        .first(db)
        .optional()?
        .unwrap_or((false, false)))
}

/// Trusts the edits or the commands of `session` from now on.
pub(crate) fn trust(
    db: &mut Db,
    session: Session,
    danger: Danger,
) -> Result<()> {
    let (edit, command) =
        (danger == Danger::Edit, danger == Danger::Command);
    diesel::insert_into(trust::table)
        .values((
            trust::agent.eq(session.agent),
            trust::chief.eq(session.chief),
            trust::session.eq(session.session),
            trust::edit.eq(edit),
            trust::command.eq(command),
        ))
        .on_conflict((trust::agent, trust::chief, trust::session))
        .do_update()
        .set((
            trust::edit.eq(trust::edit.or(excluded(trust::edit))),
            trust::command
                .eq(trust::command.or(excluded(trust::command))),
        ))
        .execute(db)?;
    Ok(())
}

/// Takes back the trust of a session that is resumed (H11).
pub(crate) fn reset_trust(
    db: &mut Db,
    session: Session,
) -> Result<()> {
    diesel::delete(trust::table.find((
        session.agent,
        session.chief,
        session.session,
    )))
    .execute(db)?;
    Ok(())
}

/// The word for `permission`, as the chief writes it and a grant
/// keeps it.
pub(crate) fn word(permission: &Permission) -> &'static str {
    match permission {
        Permission::Network => "network",
        Permission::Unconfined => "unconfined",
    }
}

/// The permission named `name`.
pub(crate) fn permission(name: &str) -> Option<Permission> {
    [Permission::Network, Permission::Unconfined]
        .into_iter()
        .find(|permission| word(permission) == name)
}

/// Whether `agent` holds `permission` by a grant.
pub(crate) fn has_grant(
    db: &mut Db,
    agent: AgentId,
    permission: &Permission,
) -> Result<bool> {
    Ok(permission_grant::table
        .filter(permission_grant::agent.eq(agent))
        .filter(permission_grant::permission.eq(word(permission)))
        .filter(permission_grant::revoked_at.is_null())
        .count()
        .get_result::<i64>(db)?
        > 0)
}

/// Gives `agent` `permission` from `chief`, unless it holds it.
pub(crate) fn give(
    db: &mut Db,
    agent: AgentId,
    permission: &Permission,
    chief: ChiefId,
    at: i64,
) -> Result<()> {
    if has_grant(db, agent, permission)? {
        return Ok(());
    }
    diesel::insert_into(permission_grant::table)
        .values((
            permission_grant::agent.eq(agent),
            permission_grant::permission.eq(word(permission)),
            permission_grant::at.eq(at),
            permission_grant::chief.eq(chief),
        ))
        .execute(db)?;
    Ok(())
}

/// Takes `permission` back from `agent`, as `by` says. `false` when
/// it held no grant.
pub(crate) fn revoke(
    db: &mut Db,
    agent: AgentId,
    permission: &Permission,
    by: &str,
    at: i64,
) -> Result<bool> {
    let taken = diesel::update(
        permission_grant::table
            .filter(permission_grant::agent.eq(agent))
            .filter(permission_grant::permission.eq(word(permission)))
            .filter(permission_grant::revoked_at.is_null()),
    )
    .set((
        permission_grant::revoked_at.eq(at),
        permission_grant::revoked_by.eq(by),
    ))
    .execute(db)?;
    Ok(taken > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_reaches_the_network_by_the_words_it_names() {
        let bash = |command| is_networked(Dialect::Bash, command);
        assert!(bash("echo curl > network"));
        assert!(bash("git clone x"));
        assert!(!bash("echo curly"));
        assert!(is_networked(
            Dialect::PowerShell,
            "Invoke-WebRequest x"
        ));
    }

    #[test]
    fn a_refusal_is_not_asked_again_in_its_turn() {
        let mut asked = Asked::default();
        asked.refuse(Danger::Network, "echo curl", "no network");
        assert!(
            refused(&asked, Danger::Network, "echo wget", 3)
                .unwrap()
                .contains("(no network)")
        );
        assert_eq!(refused(&asked, Danger::Command, "ls", 3), None);
        assert!(refused(&asked, Danger::Command, "ls", 1).is_some());
    }
}
