//! Merging two agents of a hamlet into one, and splitting them again
//! (H5): the kept agent takes the absorbed one's opened channels, its
//! agents and its inbox; a job brings what the absorbed agent learned
//! into the kept one's branch.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ChannelId, MergeId, TaskId};
use hamlet_store::schema::{agent_merge, channel, hamlet_agent};
use serde_json::json;

use super::agents::{self, EndHow};
use super::channels::{self, ChannelKind};
use super::record::{self, Decision, DecisionKind};
use super::rejection::Rejection;
use super::{hamlet_agents, tree};
use crate::realm::change::{Change, Done, done, reject};
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
        reject!(Rejection::SameEnds);
    }
    let (one, other) = (
        hamlet_agents::get(cx.db, kept)?,
        hamlet_agents::get(cx.db, absorbed)?,
    );
    for id in [kept, absorbed] {
        done!(hamlet_agents::require_living(cx.db, id));
    }
    if one.hamlet != other.hamlet {
        reject!(Rejection::OtherHamlet {
            name: super::addresses::name_of(cx.db, absorbed)?
        });
    }
    if hamlet_agents::is_lead(cx.db, absorbed)? {
        reject!(Rejection::EndingLead);
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
    move_channels(cx, kept, absorbed)?;
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

/// Gives `kept` the open channels of `absorbed` that were opened, but
/// one to `kept` or to an agent `kept` has a channel to already.
fn move_channels(
    cx: &mut Change<'_>,
    kept: AgentId,
    absorbed: AgentId,
) -> Result<()> {
    let open = channel::table
        .filter(channel::a.eq(absorbed).or(channel::b.eq(absorbed)))
        .filter(channel::closed_at.is_null())
        .filter(channel::kind.eq(ChannelKind::Opened))
        .select((
            channel::id,
            channel::a,
            channel::b,
            channel::reason,
            channel::task,
        ))
        .load::<(ChannelId, AgentId, AgentId, String, Option<TaskId>)>(
            cx.db,
        )?;
    for (id, a, b, reason, task) in open {
        let other = if a == absorbed { b } else { a };
        diesel::update(channel::table.find(id))
            .set((
                channel::closed_at.eq(cx.at),
                channel::closed_by.eq(cx.actor.to_string()),
                channel::closed_reason.eq("its end was merged"),
            ))
            .execute(cx.db)?;
        if other == kept
            || channels::between(cx.db, kept, other)?.is_some()
        {
            continue;
        }
        let new = channels::insert(
            cx,
            kept,
            other,
            ChannelKind::Opened,
            &reason,
            task,
        )?;
        diesel::update(channel::table.find(new))
            .set(channel::inherited_from.eq(id))
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
        reject!(Rejection::AlreadySplit);
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
