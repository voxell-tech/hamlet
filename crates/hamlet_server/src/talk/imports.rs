//! Projects of other programs brought in (C35): a hamlet for each,
//! working on its folder, and a hire for each of its conversations,
//! whose chat begins with it. The server holds a conversation's
//! entries until its runtime has made it with them, which then keeps
//! them alone (ADR 0002).

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_protocol::client::{FileId, Imported, ImportedEntry};
use hamlet_protocol::runtime::Earlier;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ChiefId, ConversationId, HamletId};
use hamlet_store::schema::{chat, chat_session, import};
use schemars::generate::SchemaSettings;
use serde_json::Value;

use super::marks::Mark;
use super::{Env, chats};
use crate::change::{Change, Done, done};
use crate::files::Files;
use crate::society::chiefs;
use crate::society::hamlets::{self, Founding};
use crate::society::hires::{self, Hiring, Spec};
use crate::society::names::NAME_LENGTH;
use crate::society::refusal::Refusal;

/// A project to bring in.
pub(crate) struct Project<'a> {
    pub name: &'a str,
    /// The folder its hamlet works on, when it is one on this
    /// machine.
    pub folder: Option<&'a str>,
    pub conversations: &'a [Imported],
}

/// What bringing in a project did.
pub(crate) struct Brought {
    pub hamlet: HamletId,
    pub founded: bool,
    /// The hires made, one for each conversation.
    pub hired: usize,
    /// The conversations brought in before, and those the hamlet had
    /// no room for.
    pub before: usize,
    pub full: usize,
}

impl Brought {
    /// What it did, for the chief.
    pub fn words(&self) -> String {
        let hires = match self.hired {
            1 => "1 hire".to_owned(),
            count => format!("{count} hires"),
        };
        let mut words = match self.founded {
            true => format!(
                "Founded hamlet `{}` with {hires}.",
                self.hamlet
            ),
            false => {
                format!("Added {hires} to hamlet `{}`.", self.hamlet)
            }
        };
        if self.before > 0 {
            words.push_str(&format!(
                " {} brought in before were left out.",
                self.before
            ));
        }
        if self.full > 0 {
            words.push_str(&format!(
                " {} did not fit under the hamlet's tier.",
                self.full
            ));
        }
        words
    }
}

/// Brings in `project` for `chief`: into the living hamlet of theirs
/// that works on its folder, else into one founded for it.
pub(crate) fn bring_in_project(
    cx: &mut Change<'_>,
    env: &Env,
    chief: ChiefId,
    project: &Project<'_>,
) -> Done<Brought> {
    let folder = project
        .folder
        .filter(|folder| std::path::Path::new(folder).is_dir());
    let mut working = None;
    if let Some(folder) = folder {
        for hamlet in hamlets::all(cx.db)? {
            if hamlet.is_living()
                && hamlet.folder.as_deref() == Some(folder)
                && hamlets::chiefs(cx.db, &hamlet.id)?
                    .contains(&chief)
            {
                working = Some((hamlet.id, hamlet.mayor));
                break;
            }
        }
    }
    let founded = working.is_none();
    let (hamlet, mayor) = match working {
        Some(working) => working,
        None => {
            let name = label(project.name, "Project");
            let hired = done!(hamlets::found(
                cx,
                Founding {
                    purpose: format!("Mayor of {name}."),
                    name,
                    chief,
                    mayor: Hiring::Blank {
                        name: String::new(),
                        character: String::new(),
                    },
                }
            ));
            if let Some(folder) = folder {
                done!(hamlets::set_folder(
                    cx,
                    &hired.hamlet,
                    Some(folder)
                ));
            }
            (hired.hamlet, hired.agent)
        }
    };
    let tier = hamlets::get(cx.db, &hamlet)?.tier;
    let mut brought = Brought {
        hamlet: hamlet.clone(),
        founded,
        hired: 0,
        before: 0,
        full: 0,
    };
    for imported in project.conversations {
        if is_brought_in(cx.db, chief, imported)? {
            brought.before += 1;
            continue;
        }
        if hires::count_living(cx.db, &hamlet)? >= tier {
            brought.full += 1;
            continue;
        }
        let hired = done!(hires::hire(
            cx,
            Spec {
                hamlet: hamlet.clone(),
                parent: Some(mayor),
                hiring: Hiring::Blank {
                    name: label(&imported.title, ""),
                    character: String::new(),
                },
                purpose: format!(
                    "Carry on the conversation brought in from {}: \
                     {}",
                    imported.source.name(),
                    imported.title
                ),
                reason: String::new(),
                owner: chief,
            }
        ));
        begin(cx, env, hired.agent, chief, imported)?;
        brought.hired += 1;
    }
    Ok(Ok(brought))
}

/// `text` as a name: one line with no colon, cut to the longest a
/// name is; `or` when nothing is left.
fn label(text: &str, or: &str) -> String {
    let line = text
        .replace(':', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let name = line.chars().take(NAME_LENGTH).collect::<String>();
    match name.trim() {
        "" => or.to_owned(),
        name => name.to_owned(),
    }
}

/// Whether `chief` brought in `imported` before.
fn is_brought_in(
    db: &mut Db,
    chief: ChiefId,
    imported: &Imported,
) -> Result<bool> {
    Ok(import::table
        .filter(import::chief.eq(chief))
        .filter(import::source.eq(imported.source.word()))
        .filter(import::session.eq(&imported.session))
        .select(import::conversation)
        .first::<ConversationId>(db)
        .optional()?
        .is_some())
}

/// Begins a session of `chief`'s chat with `agent` with `imported`,
/// current only when the chat has none. Returns its number.
fn begin(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    chief: ChiefId,
    imported: &Imported,
) -> Result<i64> {
    let source = imported.source.word();
    let has_chat =
        chats::current_number(cx.db, agent, chief)?.is_some();
    let number = chats::sessions(cx.db, agent, chief)?
        .last()
        .map_or(1, |(number, _, _)| number + 1);
    let conversation = chats::begin(cx, env, agent, chief, number)?;
    if !has_chat {
        diesel::insert_into(chat::table)
            .values((
                chat::agent.eq(agent),
                chat::chief.eq(chief),
                chat::current.eq(number),
            ))
            .execute(cx.db)?;
    }
    diesel::insert_into(chat_session::table)
        .values((
            chat_session::agent.eq(agent),
            chat_session::chief.eq(chief),
            chat_session::session.eq(number),
            chat_session::conversation.eq(conversation),
            chat_session::began.eq(time(imported.began)?),
        ))
        .execute(cx.db)?;
    let name = chiefs::find(cx.db, chief)?
        .context("the chief is not in the realm")?
        .name;
    let earlier = earlier_of(imported, &name)?;
    diesel::insert_into(import::table)
        .values((
            import::conversation.eq(conversation),
            import::agent.eq(agent),
            import::chief.eq(chief),
            import::source.eq(source),
            import::session.eq(&imported.session),
            import::earlier.eq(serde_json::to_string(&earlier)?),
        ))
        .execute(cx.db)?;
    cx.touched.agents.insert(agent);
    Ok(number)
}

/// The conversation a client sent as the file `id`, checked against
/// the protocol's limits.
pub(crate) fn read(
    files: &Files,
    id: &FileId,
) -> Result<std::result::Result<Imported, Refusal>> {
    let usage = |words: &str| Refusal::Usage {
        words: words.into(),
    };
    let Some(size) = files.size(id) else {
        return Ok(Err(usage(
            "The conversation was not sent first.",
        )));
    };
    if size > Imported::MOST_BYTES {
        return Ok(Err(usage(
            "A conversation brought in is at most 16 MiB.",
        )));
    }
    let bytes = files.read(id)?.context("a sent file went away")?;
    let unfit = || {
        usage("That file is not a conversation Hamlet can bring in.")
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(Err(unfit()));
    };
    let schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<Imported>()
        .to_value();
    if !jsonschema::validator_for(&schema)?.is_valid(&value) {
        return Ok(Err(unfit()));
    }
    Ok(serde_json::from_value(value).map_err(|_| unfit()))
}

/// The entries `conversation` begins with, while its runtime has not
/// made it.
pub(crate) fn earlier(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<Vec<Earlier>> {
    let held = import::table
        .find(conversation)
        .select(import::earlier)
        .first::<Option<String>>(db)
        .optional()?
        .flatten();
    held.map_or(Ok(Vec::new()), |held| {
        serde_json::from_str(&held).context("reading held entries")
    })
}

/// Lets go of the entries of `conversation`, which its runtime keeps
/// now.
pub(crate) fn made(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<()> {
    diesel::update(import::table.find(conversation))
        .set(import::earlier.eq(None::<String>))
        .execute(db)?;
    Ok(())
}

/// The runtime's entries for `imported`: a notice of where it came
/// from, then its messages and tool calls.
fn earlier_of(
    imported: &Imported,
    chief: &str,
) -> Result<Vec<Earlier>> {
    let source = imported.source;
    let mut words = format!("Brought in from {}.", source.name());
    match imported.left_out {
        0 => {}
        1 => words.push_str(" 1 earlier message was left out."),
        count => words.push_str(&format!(
            " {count} earlier messages were left out."
        )),
    }
    let mut earlier = vec![Earlier::Note {
        mark: Mark::Notice {
            text: words,
            at: time(imported.began)?,
        }
        .wire(),
    }];
    let model = imported.model.as_deref().unwrap_or(source.word());
    for entry in &imported.entries {
        earlier.push(match entry {
            ImportedEntry::User { at, text } => Earlier::User {
                at: *at,
                mark: Mark::Earlier {
                    chief: chief.to_owned(),
                    at: time(*at)?,
                }
                .wire(),
                text: text.clone(),
            },
            ImportedEntry::Assistant { at, text } => {
                Earlier::Assistant {
                    at: *at,
                    model: model.to_owned(),
                    source: source.word().to_owned(),
                    text: text.clone(),
                }
            }
            ImportedEntry::Step { at, summary, tool } => {
                Earlier::Note {
                    mark: Mark::Step {
                        tool: tool.clone(),
                        summary: summary.clone(),
                        at: time(*at)?,
                    }
                    .wire(),
                }
            }
        });
    }
    Ok(earlier)
}

fn time(at: u64) -> Result<i64> {
    i64::try_from(at).context("a time past the year 292 million")
}
