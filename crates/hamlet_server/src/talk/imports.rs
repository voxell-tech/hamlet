//! Chat sessions brought in from another program (C35). The server
//! holds a session's entries until its runtime has made the
//! conversation with them, which then keeps them alone (ADR 0002).

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_protocol::client::{FileId, Imported, ImportedEntry};
use hamlet_protocol::runtime::Earlier;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ChiefId, ConversationId};
use hamlet_store::schema::{chat, chat_session, import};
use schemars::generate::SchemaSettings;
use serde_json::Value;

use super::marks::Mark;
use super::{Env, chats};
use crate::change::{Change, Done, refuse};
use crate::files::Files;
use crate::society::chiefs;
use crate::society::refusal::Refusal;

/// Brings in `imported` as a new session of `chief`'s chat with
/// `agent`, current only when the chat has none. Returns its number.
pub(crate) fn bring_in(
    cx: &mut Change<'_>,
    env: &Env,
    agent: AgentId,
    chief: ChiefId,
    imported: &Imported,
) -> Done<i64> {
    let source = imported.source.word();
    let already = import::table
        .filter(import::agent.eq(agent))
        .filter(import::chief.eq(chief))
        .filter(import::source.eq(source))
        .filter(import::session.eq(&imported.session))
        .select(import::conversation)
        .first::<ConversationId>(cx.db)
        .optional()?;
    if let Some(conversation) = already {
        let number = chat_session::table
            .filter(chat_session::conversation.eq(conversation))
            .select(chat_session::session)
            .first(cx.db)?;
        refuse!(Refusal::AlreadyImported { number });
    }
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
    Ok(Ok(number))
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
