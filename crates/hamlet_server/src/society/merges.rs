//! Merging two agents of a hamlet into one, and splitting them again
//! (H5): the kept agent takes the absorbed one's laid roads, its
//! agents and its inbox; a job brings what the absorbed agent learned
//! into the kept one's branch.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, MergeId, RoadId, TaskId};
use hamlet_store::schema::{agent_merge, hamlet_agent, road};
use serde_json::json;

use super::agents::{self, EndHow};
use super::record::{self, Decision, DecisionKind};
use super::refusal::Refusal;
use super::roads::{self, RoadKind};
use super::{hamlet_agents, tree};
use crate::realm::change::{Change, Done, done, refuse};
use crate::realm::jobs::{self, Job};

/// A merge as it is kept.
#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = agent_merge)]
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
    agent_merge::table
        .find(id)
        .select(Merge::as_select())
        .first(db)
        .optional()?
        .with_context(|| format!("no merge {id}"))
}

/// The merge that absorbed `agent`, while it stands.
pub(crate) fn of_absorbed(
    db: &mut Db,
    agent: AgentId,
) -> Result<Option<Merge>> {
    Ok(agent_merge::table
        .filter(agent_merge::absorbed.eq(agent))
        .filter(agent_merge::split_at.is_null())
        .order(agent_merge::id.desc())
        .select(Merge::as_select())
        .first(db)
        .optional()?)
}

/// Merges the living agent `absorbed` into `kept`, of the same
/// hamlet.
pub(crate) fn merge(
    cx: &mut Change<'_>,
    kept: AgentId,
    absorbed: AgentId,
    reason: &str,
) -> Done<MergeId> {
    if kept == absorbed {
        refuse!(Refusal::SameEnds);
    }
    let (one, other) = (
        hamlet_agents::get(cx.db, kept)?,
        hamlet_agents::get(cx.db, absorbed)?,
    );
    for id in [kept, absorbed] {
        done!(hamlet_agents::require_living(cx.db, id));
    }
    if one.hamlet != other.hamlet {
        refuse!(Refusal::OtherHamlet {
            name: super::addresses::name_of(cx.db, absorbed)?
        });
    }
    if hamlet_agents::is_lead(cx.db, absorbed)? {
        refuse!(Refusal::EndingLead);
    }
    for id in [kept, absorbed] {
        done!(hamlet_agents::unprotected(cx, id));
    }
    // The kept agent takes the absorbed one's place in the tree when
    // it was under it, and its agents under it either way.
    let parent = if one.parent == Some(absorbed) {
        other.parent
    } else {
        one.parent
    };
    let children = tree::children(cx.db, Some(absorbed))?
        .into_iter()
        .filter(|child| *child != kept)
        .collect::<Vec<_>>();
    diesel::update(hamlet_agent::table.find(kept))
        .set(hamlet_agent::parent.eq(parent))
        .execute(cx.db)?;
    diesel::update(
        hamlet_agent::table
            .filter(hamlet_agent::agent.eq_any(&children)),
    )
    .set(hamlet_agent::parent.eq(kept))
    .execute(cx.db)?;
    cx.touched.agents.extend(children);
    move_roads(cx, kept, absorbed)?;
    let id = diesel::insert_into(agent_merge::table)
        .values((
            agent_merge::kept.eq(kept),
            agent_merge::absorbed.eq(absorbed),
            agent_merge::at.eq(cx.at),
            agent_merge::by.eq(cx.actor.to_string()),
            agent_merge::reason.eq(reason),
        ))
        .returning(agent_merge::id)
        .get_result::<MergeId>(cx.db)?;
    diesel::update(hamlet_agent::table.find(absorbed))
        .set(hamlet_agent::merged_into.eq(kept))
        .execute(cx.db)?;
    agents::end(cx, absorbed, EndHow::Merged, reason)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Merged, reason)
            .hamlet(&one.hamlet)
            .subject(absorbed)
            .other(kept)
            .profile(other.profile)
            .detail(json!({ "merge": id.0 })),
    )?;
    jobs::add(cx, &Job::Merge { merge: id })?;
    cx.touched.agents.extend([kept, absorbed]);
    cx.touched.hamlets.insert(one.hamlet);
    Ok(Ok(id))
}

/// Gives `kept` the open laid roads of `absorbed`, but one to `kept`
/// or to an agent `kept` has a road to already.
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
        .load::<(RoadId, AgentId, AgentId, String, Option<TaskId>)>(
            cx.db,
        )?;
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

/// Splits the merge `id` again: the absorbed agent comes back and the
/// merge's commit is undone on the kept agent's branch.
pub(crate) fn split(
    cx: &mut Change<'_>,
    id: MergeId,
    reason: &str,
) -> Done<()> {
    let found = get(cx.db, id)?;
    if found.split_at.is_some() {
        refuse!(Refusal::AlreadySplit);
    }
    done!(hamlet_agents::come_back(cx, found.absorbed, reason));
    diesel::update(agent_merge::table.find(id))
        .set((
            agent_merge::split_at.eq(cx.at),
            agent_merge::split_by.eq(cx.actor.to_string()),
        ))
        .execute(cx.db)?;
    let hamlet = hamlet_agents::get(cx.db, found.kept)?.hamlet;
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

/// Keeps what the merge job did to the kept agent's branch.
pub(crate) fn merged(
    db: &mut Db,
    id: MergeId,
    commit: &str,
    clashes: &[String],
) -> Result<()> {
    diesel::update(agent_merge::table.find(id))
        .set((
            agent_merge::merge_commit.eq(commit),
            agent_merge::clashes.eq(serde_json::to_string(clashes)?),
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
    diesel::update(agent_merge::table.find(id))
        .set(agent_merge::split_commit.eq(commit))
        .execute(db)?;
    Ok(())
}
