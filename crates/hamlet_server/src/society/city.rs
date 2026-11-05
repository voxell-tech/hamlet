//! The city: every scholar of the realm, with its house.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, ChiefId, ScholarId};
use hamlet_store::schema::{agent, hamlet, hire, scholar};

use super::agents::EndHow;
use super::hires;
use super::names::check_name;
use super::record::{self, Decision, DecisionKind};
use super::refusal::Refusal;
use crate::change::{Change, Done, done, refuse};
use crate::jobs::{self, Fork, Job};
use crate::text::text_enum;

/// The most characters a character may have (C11, C13).
pub(crate) const CHARACTER_MOST: usize = 4000;

text_enum! {
    pub(crate) enum ScholarKind {
        Scholar = "scholar",
        /// The Hand's own knowledge, never hired or listed.
        Hand = "hand",
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = scholar)]
pub(crate) struct Scholar {
    pub id: ScholarId,
    pub kind: ScholarKind,
    pub name: String,
    pub chief: Option<ChiefId>,
    pub since: i64,
    pub made_by: Actor,
    pub planted: bool,
    pub house_x: Option<f64>,
    pub house_z: Option<f64>,
    pub fork_of: Option<ScholarId>,
    pub fork_at: Option<String>,
    pub left_at: Option<i64>,
    pub left_by: Option<Actor>,
    pub left_reason: Option<String>,
    pub erased: bool,
}

impl Scholar {
    pub fn is_living(&self) -> bool {
        self.left_at.is_none()
    }
}

/// Makes a scholar named `name` for `chief`, starting from
/// `character`, or from a commit of another scholar when it is a
/// fork.
pub(crate) fn make(
    cx: &mut Change<'_>,
    name: &str,
    chief: ChiefId,
    character: &str,
    fork: Option<Fork>,
) -> Done<ScholarId> {
    if let Err(refusal) = check_name(name) {
        refuse!(refusal);
    }
    if named(cx.db, name)?.is_some() {
        refuse!(Refusal::NameTaken { name: name.into() });
    }
    if character.chars().count() > CHARACTER_MOST {
        refuse!(Refusal::LongCharacter);
    }
    let id = diesel::insert_into(scholar::table)
        .values((
            scholar::kind.eq(ScholarKind::Scholar),
            scholar::name.eq(name),
            scholar::chief.eq(chief),
            scholar::since.eq(cx.at),
            scholar::made_by.eq(cx.actor),
            scholar::fork_of.eq(fork.as_ref().map(|f| f.from)),
            scholar::fork_at
                .eq(fork.as_ref().map(|f| f.commit.clone())),
        ))
        .returning(scholar::id)
        .get_result::<ScholarId>(cx.db)?;
    let kind = if fork.is_some() {
        DecisionKind::Forked
    } else {
        DecisionKind::ScholarMade
    };
    jobs::add(
        cx,
        &Job::Plant {
            scholar: id,
            character: character.to_owned(),
            fork,
        },
    )?;
    record::keep(cx, Decision::new(kind, "").scholar(id))?;
    cx.touched.scholars.insert(id);
    Ok(Ok(id))
}

/// Makes the Hand's own knowledge, once, with the realm.
pub(crate) fn make_hand(
    cx: &mut Change<'_>,
    character: &str,
) -> Result<ScholarId> {
    let id = diesel::insert_into(scholar::table)
        .values((
            scholar::kind.eq(ScholarKind::Hand),
            scholar::name.eq("hand"),
            scholar::since.eq(cx.at),
            scholar::made_by.eq(cx.actor),
        ))
        .returning(scholar::id)
        .get_result::<ScholarId>(cx.db)?;
    jobs::add(
        cx,
        &Job::Plant {
            scholar: id,
            character: character.to_owned(),
            fork: None,
        },
    )?;
    cx.touched.scholars.insert(id);
    Ok(id)
}

pub(crate) fn find(
    db: &mut Db,
    id: ScholarId,
) -> Result<Option<Scholar>> {
    Ok(scholar::table
        .find(id)
        .select(Scholar::as_select())
        .first(db)
        .optional()?)
}

pub(crate) fn get(db: &mut Db, id: ScholarId) -> Result<Scholar> {
    find(db, id)?.with_context(|| format!("no scholar {id}"))
}

/// The living scholar named `name`.
pub(crate) fn named(
    db: &mut Db,
    name: &str,
) -> Result<Option<Scholar>> {
    Ok(scholar::table
        .filter(scholar::name.eq(name))
        .filter(scholar::left_at.is_null())
        .filter(scholar::kind.eq(ScholarKind::Scholar))
        .select(Scholar::as_select())
        .first(db)
        .optional()?)
}

/// Every scholar of the city, living and archived, but none erased.
pub(crate) fn all(db: &mut Db) -> Result<Vec<Scholar>> {
    Ok(scholar::table
        .filter(scholar::kind.eq(ScholarKind::Scholar))
        .filter(scholar::erased.eq(false))
        .order(scholar::id)
        .select(Scholar::as_select())
        .load(db)?)
}

/// Sets where the house of `id` stands in the city.
pub(crate) fn place(
    cx: &mut Change<'_>,
    id: ScholarId,
    x: f64,
    z: f64,
) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if !scholar.is_living() {
        refuse!(Refusal::ScholarGone { name: scholar.name });
    }
    if !(x.is_finite() && z.is_finite()) {
        refuse!(Refusal::BadAddress {
            address: format!("{x}, {z}")
        });
    }
    diesel::update(scholar::table.find(id))
        .set((scholar::house_x.eq(x), scholar::house_z.eq(z)))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::HouseMoved, "").scholar(id),
    )?;
    cx.touched.scholars.insert(id);
    Ok(Ok(()))
}

/// Archives the scholar `id`, ending its living hires first. Its name
/// is free again. Refused while one of its hires holds a hamlet's
/// office.
pub(crate) fn archive(
    cx: &mut Change<'_>,
    id: ScholarId,
    reason: &str,
) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if !scholar.is_living() || scholar.kind == ScholarKind::Hand {
        refuse!(Refusal::ScholarGone { name: scholar.name });
    }
    if reason.trim().is_empty() {
        refuse!(Refusal::NoReason);
    }
    for hire in hires::living_of(cx.db, id)? {
        done!(hires::end(cx, hire.agent, EndHow::Scholar, reason));
    }
    diesel::update(scholar::table.find(id))
        .set((
            scholar::left_at.eq(cx.at),
            scholar::left_by.eq(cx.actor),
            scholar::left_reason.eq(reason),
        ))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::ScholarArchived, reason)
            .scholar(id),
    )?;
    cx.touched.scholars.insert(id);
    Ok(Ok(()))
}

/// Brings back the archived scholar `id` under its name, with the
/// hires its archiving ended in hamlets that still live.
pub(crate) fn restore(
    cx: &mut Change<'_>,
    id: ScholarId,
    reason: &str,
) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if scholar.erased {
        refuse!(Refusal::ScholarErased { name: scholar.name });
    }
    if scholar.is_living() {
        refuse!(Refusal::ScholarLiving { name: scholar.name });
    }
    if named(cx.db, &scholar.name)?.is_some() {
        refuse!(Refusal::NameTaken { name: scholar.name });
    }
    diesel::update(scholar::table.find(id))
        .set((
            scholar::left_at.eq(None::<i64>),
            scholar::left_by.eq(None::<Actor>),
            scholar::left_reason.eq(None::<String>),
        ))
        .execute(cx.db)?;
    let ended_with = hire::table
        .inner_join(agent::table)
        .inner_join(hamlet::table)
        .filter(hire::scholar.eq(id))
        .filter(agent::ended_how.eq(EndHow::Scholar))
        .filter(agent::erased.eq(false))
        .filter(hamlet::removed_at.is_null())
        .order(hire::agent)
        .select(hire::agent)
        .load(cx.db)?;
    for hire in ended_with {
        done!(hires::come_back(cx, hire, reason));
    }
    record::keep(
        cx,
        Decision::new(DecisionKind::ScholarRestored, reason)
            .scholar(id),
    )?;
    cx.touched.scholars.insert(id);
    Ok(Ok(()))
}

/// Erases the archived scholar `id` for good, with every hire of it.
pub(crate) fn erase(cx: &mut Change<'_>, id: ScholarId) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if scholar.erased {
        refuse!(Refusal::ScholarErased { name: scholar.name });
    }
    if scholar.is_living() {
        refuse!(Refusal::ScholarLiving { name: scholar.name });
    }
    let kept = hire::table
        .inner_join(agent::table)
        .filter(hire::scholar.eq(id))
        .filter(agent::erased.eq(false))
        .select(hire::agent)
        .load(cx.db)?;
    for hire in kept {
        done!(hires::erase(cx, hire));
    }
    diesel::update(scholar::table.find(id))
        .set(scholar::erased.eq(true))
        .execute(cx.db)?;
    jobs::add(cx, &Job::EraseScholar { scholar: id })?;
    record::keep(
        cx,
        Decision::new(DecisionKind::ScholarErased, "").scholar(id),
    )?;
    cx.touched.scholars.insert(id);
    Ok(Ok(()))
}

/// Marks the repository of `id` made.
pub(crate) fn planted(db: &mut Db, id: ScholarId) -> Result<()> {
    diesel::update(scholar::table.find(id))
        .set(scholar::planted.eq(true))
        .execute(db)?;
    Ok(())
}
