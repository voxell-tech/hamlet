//! The entries a conversation's runtime kept, as the server keeps
//! them for clients.

use anyhow::Result;
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::{Bool, Text};
use hamlet_protocol::runtime::{Block, Item, ItemKind};
use hamlet_store::Db;
use hamlet_store::ids::ConversationId;
use hamlet_store::schema::item;

/// Keeps `entry` of `conversation`, once. Returns whether it is new.
pub(crate) fn keep(
    db: &mut Db,
    conversation: ConversationId,
    entry: &Item,
) -> Result<bool> {
    let ordinal = item::table
        .filter(item::conversation.eq(conversation))
        .select(diesel::dsl::max(item::ordinal))
        .first::<Option<i64>>(db)?
        .map_or(0, |last| last + 1);
    let kept = diesel::insert_into(item::table)
        .values((
            item::conversation.eq(conversation),
            item::entry.eq(&entry.entry),
            item::ordinal.eq(ordinal),
            item::body.eq(serde_json::to_string(entry)?),
        ))
        .on_conflict_do_nothing()
        .execute(db)?;
    Ok(kept == 1)
}

/// At most `limit` entries of `conversation` before the ordinal
/// `before`, or its latest, oldest first, with their ordinals.
pub(crate) fn page(
    db: &mut Db,
    conversation: ConversationId,
    before: Option<i64>,
    limit: i64,
) -> Result<Vec<(i64, Item)>> {
    let mut query = item::table
        .filter(item::conversation.eq(conversation))
        .order(item::ordinal.desc())
        .limit(limit)
        .select((item::ordinal, item::body))
        .into_boxed();
    if let Some(before) = before {
        query = query.filter(item::ordinal.lt(before));
    }
    let mut rows = query
        .load::<(i64, String)>(db)?
        .into_iter()
        .map(|(ordinal, body)| {
            Ok((ordinal, serde_json::from_str(&body)?))
        })
        .collect::<Result<Vec<_>>>()?;
    rows.reverse();
    Ok(rows)
}

/// The last words the assistant answered the input `request` of
/// `conversation` with, before the next input.
pub(crate) fn answer_of(
    db: &mut Db,
    conversation: ConversationId,
    request: &str,
) -> Result<Option<String>> {
    let start = item::table
        .filter(item::conversation.eq(conversation))
        .filter(
            sql::<Bool>("json_extract(body, '$.request') = ")
                .bind::<Text, _>(request),
        )
        .select(item::ordinal)
        .first::<i64>(db)
        .optional()?;
    let Some(start) = start else {
        return Ok(None);
    };
    let bodies = item::table
        .filter(item::conversation.eq(conversation))
        .filter(item::ordinal.gt(start))
        .order(item::ordinal)
        .select(item::body)
        .load::<String>(db)?;
    let mut answer = None;
    for body in bodies {
        let entry = serde_json::from_str::<Item>(&body)?;
        match entry.kind {
            ItemKind::User if entry.request.is_some() => break,
            ItemKind::Assistant => {
                let text = entry
                    .blocks
                    .iter()
                    .flatten()
                    .filter_map(|block| match block {
                        Block::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.trim().is_empty() {
                    answer = Some(text.trim().to_owned());
                }
            }
            _ => {}
        }
    }
    Ok(answer)
}

/// The last entry of `conversation`.
pub(crate) fn last(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<Option<Item>> {
    Ok(page(db, conversation, None, 1)?.pop().map(|(_, item)| item))
}
