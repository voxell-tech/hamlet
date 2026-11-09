//! What the library keeps as rows (H33): each skill offered and how
//! the librarian judged it, and what came to and left the shelves.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ChiefId, OfferId, TaskId};
use hamlet_store::schema::{library_event, skill_offer};

use crate::change::Change;
use crate::text::text_enum;

text_enum! {
    pub(crate) enum Verdict {
        Accepted = "accepted",
        Refused = "refused",
        /// The librarian could not judge it.
        Failed = "failed",
    }
}

text_enum! {
    pub(crate) enum EventKind {
        Starter = "starter",
        Shelved = "shelved",
        Refused = "refused",
        Unshelved = "unshelved",
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = skill_offer)]
pub(crate) struct Offer {
    pub id: OfferId,
    pub agent: AgentId,
    pub skill: String,
    pub reason: String,
    pub task: Option<TaskId>,
    pub runtime: String,
    pub key: String,
    pub tree: Option<String>,
    pub verdict: Option<Verdict>,
    pub words: Option<String>,
    pub at: i64,
}

/// The offer the call `key` of `runtime` made.
pub(crate) fn offer_of(
    db: &mut Db,
    runtime: &str,
    key: &str,
) -> Result<Option<Offer>> {
    Ok(skill_offer::table
        .filter(skill_offer::runtime.eq(runtime))
        .filter(skill_offer::key.eq(key))
        .select(Offer::as_select())
        .first(db)
        .optional()?)
}

pub(crate) fn get(db: &mut Db, id: OfferId) -> Result<Offer> {
    skill_offer::table
        .find(id)
        .select(Offer::as_select())
        .first(db)
        .optional()?
        .with_context(|| format!("no offer {id}"))
}

/// Whether another offer of `skill` waits for the librarian.
pub(crate) fn is_judging(db: &mut Db, skill: &str) -> Result<bool> {
    Ok(skill_offer::table
        .filter(skill_offer::skill.eq(skill))
        .filter(skill_offer::verdict.is_null())
        .count()
        .get_result::<i64>(db)?
        > 0)
}

/// An offer to keep.
pub(crate) struct Offering<'a> {
    pub agent: AgentId,
    pub skill: &'a str,
    pub reason: &'a str,
    pub task: Option<TaskId>,
    pub runtime: &'a str,
    pub key: &'a str,
}

pub(crate) fn offer(
    cx: &mut Change<'_>,
    offering: Offering<'_>,
) -> Result<OfferId> {
    Ok(diesel::insert_into(skill_offer::table)
        .values((
            skill_offer::agent.eq(offering.agent),
            skill_offer::skill.eq(offering.skill),
            skill_offer::reason.eq(offering.reason),
            skill_offer::task.eq(offering.task),
            skill_offer::runtime.eq(offering.runtime),
            skill_offer::key.eq(offering.key),
            skill_offer::at.eq(cx.at),
        ))
        .returning(skill_offer::id)
        .get_result(cx.db)?)
}

/// Keeps the commit holding the offer's files.
pub(crate) fn kept(
    db: &mut Db,
    id: OfferId,
    tree: &str,
) -> Result<()> {
    diesel::update(skill_offer::table.find(id))
        .set(skill_offer::tree.eq(tree))
        .execute(db)?;
    Ok(())
}

/// Keeps the librarian's verdict on the offer `id`, once.
pub(crate) fn judge(
    cx: &mut Change<'_>,
    id: OfferId,
    verdict: Verdict,
    words: &str,
) -> Result<bool> {
    let changed = diesel::update(
        skill_offer::table
            .find(id)
            .filter(skill_offer::verdict.is_null()),
    )
    .set((
        skill_offer::verdict.eq(verdict),
        skill_offer::words.eq(words),
    ))
    .execute(cx.db)?;
    Ok(changed == 1)
}

/// A change to the shelves.
pub(crate) struct Event<'a> {
    pub kind: EventKind,
    pub skill: &'a str,
    pub agent: Option<AgentId>,
    pub chief: Option<ChiefId>,
    pub reason: Option<&'a str>,
    pub commit: Option<&'a str>,
}

pub(crate) fn event(
    db: &mut Db,
    at: i64,
    event: Event<'_>,
) -> Result<()> {
    diesel::insert_into(library_event::table)
        .values((
            library_event::kind.eq(event.kind),
            library_event::skill.eq(event.skill),
            library_event::agent.eq(event.agent),
            library_event::chief.eq(event.chief),
            library_event::at.eq(at),
            library_event::reason.eq(event.reason),
            library_event::commit_id.eq(event.commit),
        ))
        .execute(db)?;
    Ok(())
}

/// Who shelved `skill` last, as an agent or a chief.
pub(crate) fn shelved_by(
    db: &mut Db,
    skill: &str,
) -> Result<Option<(Option<AgentId>, Option<ChiefId>)>> {
    Ok(library_event::table
        .filter(library_event::skill.eq(skill))
        .filter(library_event::kind.eq(EventKind::Shelved))
        .order(library_event::id.desc())
        .select((library_event::agent, library_event::chief))
        .first(db)
        .optional()?)
}
