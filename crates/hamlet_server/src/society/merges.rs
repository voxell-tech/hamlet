//! Merging two hires of a hamlet into one, and splitting them again
//! (H5): the kept hire takes the absorbed one's laid roads, its
//! hires and its inbox; a job brings what the absorbed hire learned
//! into the kept one's branch.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, MergeId};
use hamlet_store::schema::{hire, hire_merge, road};
use serde_json::json;

use super::agents::{self, EndHow};
use super::record::{self, Decision, DecisionKind};
use super::refusal::Refusal;
use super::roads::{self, RoadKind};
use super::{hires, limits, tree};
use crate::change::{Change, Done, refuse};
use crate::jobs::{self, Job};

/// A merge as it is kept.
#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = hire_merge)]
pub(crate) struct Merge {
    pub id: MergeId,
    pub kept: AgentId,
    pub absorbed: AgentId,
    pub at: i64,
    pub by: String,
    pub reason: String,
    pub merge_commit: Option<String>,
    pub clashes: Option<String>,
    pub split_at: Option<i64>,
    pub split_by: Option<String>,
    pub split_commit: Option<String>,
}

pub(crate) fn get(db: &mut Db, id: MergeId) -> Result<Merge> {
    hire_merge::table
        .find(id)
        .select(Merge::as_select())
        .first(db)
        .optional()?
        .with_context(|| format!("no merge {id}"))
}

/// The merge that absorbed `hire`, while it stands.
pub(crate) fn of_absorbed(
    db: &mut Db,
    hire: AgentId,
) -> Result<Option<Merge>> {
    Ok(hire_merge::table
        .filter(hire_merge::absorbed.eq(hire))
        .filter(hire_merge::split_at.is_null())
        .order(hire_merge::id.desc())
        .select(Merge::as_select())
        .first(db)
        .optional()?)
}

/// Merges the living hire `absorbed` into `kept`, of the same hamlet.
pub(crate) fn merge(
    cx: &mut Change<'_>,
    kept: AgentId,
    absorbed: AgentId,
    reason: &str,
) -> Done<MergeId> {
    if kept == absorbed {
        refuse!(Refusal::SameEnds);
    }
    if reason.trim().is_empty() {
        refuse!(Refusal::NoReason);
    }
    let (one, other) =
        (hires::get(cx.db, kept)?, hires::get(cx.db, absorbed)?);
    for id in [kept, absorbed] {
        if !agents::get(cx.db, id)?.is_living() {
            refuse!(hires::left(cx.db, id)?);
        }
    }
    if one.hamlet != other.hamlet {
        refuse!(Refusal::OtherHamlet {
            name: super::addresses::name_of(cx.db, absorbed)?
        });
    }
    if hires::is_mayor(cx.db, absorbed)? {
        refuse!(Refusal::EndingMayor);
    }
    // The kept hire takes the absorbed one's place in the tree when
    // it was under it, and its hires under it either way.
    let parent = if one.parent == Some(absorbed) {
        other.parent
    } else {
        one.parent
    };
    let children = tree::children(cx.db, Some(absorbed))?
        .into_iter()
        .filter(|child| *child != kept)
        .collect::<Vec<_>>();
    let most = limits::of(cx.db, &one.hamlet)?.depth;
    let kept_depth = match parent {
        Some(parent) => tree::depth(cx.db, parent)? + 1,
        None => 0,
    };
    for child in &children {
        let below = deepest_below(cx.db, *child)?;
        if kept_depth + 1 + below > most {
            refuse!(Refusal::TooDeep { most });
        }
    }
    diesel::update(hire::table.find(kept))
        .set(hire::parent.eq(parent))
        .execute(cx.db)?;
    diesel::update(hire::table.filter(hire::agent.eq_any(&children)))
        .set(hire::parent.eq(kept))
        .execute(cx.db)?;
    cx.touched.agents.extend(children);
    move_roads(cx, kept, absorbed)?;
    let id = diesel::insert_into(hire_merge::table)
        .values((
            hire_merge::kept.eq(kept),
            hire_merge::absorbed.eq(absorbed),
            hire_merge::at.eq(cx.at),
            hire_merge::by.eq(cx.actor.to_string()),
            hire_merge::reason.eq(reason),
        ))
        .returning(hire_merge::id)
        .get_result::<MergeId>(cx.db)?;
    diesel::update(hire::table.find(absorbed))
        .set(hire::merged_into.eq(kept))
        .execute(cx.db)?;
    agents::end(cx, absorbed, EndHow::Merged, reason)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Merged, reason)
            .hamlet(&one.hamlet)
            .subject(absorbed)
            .other(kept)
            .scholar(other.scholar)
            .detail(json!({ "merge": id.0 })),
    )?;
    jobs::add(cx, &Job::Merge { merge: id })?;
    cx.touched.agents.extend([kept, absorbed]);
    cx.touched.hamlets.insert(one.hamlet);
    Ok(Ok(id))
}

/// How many generations of hires are under `hire`.
fn deepest_below(db: &mut Db, hire: AgentId) -> Result<i64> {
    let mut deepest = 0;
    for child in tree::children(db, Some(hire))? {
        deepest = deepest.max(1 + deepest_below(db, child)?);
    }
    Ok(deepest)
}

/// Gives `kept` the open laid roads of `absorbed`, but one to `kept`
/// or to a hire `kept` has a road to already.
fn move_roads(
    cx: &mut Change<'_>,
    kept: AgentId,
    absorbed: AgentId,
) -> Result<()> {
    let open = road::table
        .filter(road::a.eq(absorbed).or(road::b.eq(absorbed)))
        .filter(road::up_at.is_null())
        .filter(road::kind.eq(RoadKind::Laid))
        .select((
            road::id,
            road::a,
            road::b,
            road::reason,
            road::task,
        ))
        .load::<(
            hamlet_store::ids::RoadId,
            AgentId,
            AgentId,
            String,
            Option<hamlet_store::ids::TaskId>,
        )>(cx.db)?;
    for (id, a, b, reason, task) in open {
        let other = if a == absorbed { b } else { a };
        diesel::update(road::table.find(id))
            .set((
                road::up_at.eq(cx.at),
                road::up_by.eq(cx.actor.to_string()),
                road::up_reason.eq("its end was merged"),
            ))
            .execute(cx.db)?;
        if other == kept || roads::open(cx.db, kept, other)?.is_some()
        {
            continue;
        }
        let new = roads::insert(
            cx,
            kept,
            other,
            RoadKind::Laid,
            &reason,
            task,
        )?;
        diesel::update(road::table.find(new))
            .set(road::inherited_from.eq(id))
            .execute(cx.db)?;
    }
    Ok(())
}

/// Splits the merge `id` again: the absorbed hire comes back and the
/// merge's commit is undone on the kept hire's branch.
pub(crate) fn split(
    cx: &mut Change<'_>,
    id: MergeId,
    reason: &str,
) -> Done<()> {
    let found = get(cx.db, id)?;
    if found.split_at.is_some() {
        refuse!(Refusal::AlreadySplit);
    }
    match hires::come_back(cx, found.absorbed, reason)? {
        Ok(()) => {}
        Err(refusal) => refuse!(refusal),
    }
    diesel::update(hire_merge::table.find(id))
        .set((
            hire_merge::split_at.eq(cx.at),
            hire_merge::split_by.eq(cx.actor.to_string()),
        ))
        .execute(cx.db)?;
    let hamlet = hires::get(cx.db, found.kept)?.hamlet;
    record::keep(
        cx,
        Decision::new(DecisionKind::Split, reason)
            .hamlet(&hamlet)
            .subject(found.absorbed)
            .other(found.kept)
            .detail(json!({ "merge": id.0 })),
    )?;
    jobs::add(cx, &Job::Split { merge: id })?;
    cx.touched.agents.extend([found.kept, found.absorbed]);
    Ok(Ok(()))
}

/// Keeps what the merge job did to the kept hire's branch.
pub(crate) fn merged(
    db: &mut Db,
    id: MergeId,
    commit: &str,
    clashes: &[String],
) -> Result<()> {
    diesel::update(hire_merge::table.find(id))
        .set((
            hire_merge::merge_commit.eq(commit),
            hire_merge::clashes.eq(serde_json::to_string(clashes)?),
        ))
        .execute(db)?;
    Ok(())
}

/// Keeps the commit that undid the merge.
pub(crate) fn unmerged(
    db: &mut Db,
    id: MergeId,
    commit: &str,
) -> Result<()> {
    diesel::update(hire_merge::table.find(id))
        .set(hire_merge::split_commit.eq(commit))
        .execute(db)?;
    Ok(())
}
