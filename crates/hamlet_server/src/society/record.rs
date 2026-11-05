//! The record: every decision of a chief, the Hand or a lead, with
//! its reason (H17).

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{
    Actor, AgentId, HamletId, ScholarId, TaskId,
};
use hamlet_store::schema::decision;
use serde_json::Value;

use crate::change::Change;
use crate::text::text_enum;

text_enum! {
    pub(crate) enum DecisionKind {
        Founded = "founded",
        NamedHamlet = "named-hamlet",
        PlacedHamlet = "placed-hamlet",
        RemovedHamlet = "removed-hamlet",
        RestoredHamlet = "restored-hamlet",
        ErasedHamlet = "erased-hamlet",
        Tier = "tier",
        Folder = "folder",
        Mode = "mode",
        Limits = "limits",
        ScholarMade = "scholar-made",
        HouseMoved = "house-moved",
        ScholarArchived = "scholar-archived",
        ScholarRestored = "scholar-restored",
        ScholarErased = "scholar-erased",
        ScholarEdited = "scholar-edited",
        Tagged = "tagged",
        Reverted = "reverted",
        Forked = "forked",
        Purged = "purged",
        SkillGiven = "skill-given",
        Hired = "hired",
        Ended = "ended",
        Merged = "merged",
        Split = "split",
        Restored = "restored",
        Erased = "erased",
        Seated = "seated",
        Model = "model",
        Tools = "tools",
        Allowance = "allowance",
        Pinned = "pinned",
        Road = "road",
        Unroad = "unroad",
        RoadAgreed = "road-agreed",
        RoadRefused = "road-refused",
    }
}

/// A decision to keep.
#[derive(Clone, Debug, Default)]
pub(crate) struct Decision {
    pub hamlet: Option<HamletId>,
    pub kind: Option<DecisionKind>,
    pub subject: Option<AgentId>,
    pub other: Option<AgentId>,
    pub scholar: Option<ScholarId>,
    pub reason: String,
    pub task: Option<TaskId>,
    pub detail: Option<Value>,
}

impl Decision {
    pub fn new(kind: DecisionKind, reason: &str) -> Self {
        Self {
            kind: Some(kind),
            reason: reason.to_owned(),
            ..Self::default()
        }
    }

    pub fn hamlet(mut self, hamlet: &HamletId) -> Self {
        self.hamlet = Some(hamlet.clone());
        self
    }

    pub fn subject(mut self, agent: AgentId) -> Self {
        self.subject = Some(agent);
        self
    }

    pub fn other(mut self, agent: AgentId) -> Self {
        self.other = Some(agent);
        self
    }

    pub fn scholar(mut self, scholar: ScholarId) -> Self {
        self.scholar = Some(scholar);
        self
    }

    pub fn detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

/// A decision as the record keeps it.
#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = decision)]
pub(crate) struct Kept {
    pub id: i64,
    pub at: i64,
    pub hamlet: Option<HamletId>,
    pub actor: Actor,
    pub kind: DecisionKind,
    pub subject: Option<AgentId>,
    pub other: Option<AgentId>,
    pub scholar: Option<ScholarId>,
    pub reason: String,
    pub task: Option<TaskId>,
    pub detail: Option<String>,
}

pub(crate) fn keep(
    cx: &mut Change<'_>,
    decision: Decision,
) -> Result<()> {
    let kind =
        decision.kind.expect("a decision is made with its kind");
    diesel::insert_into(decision::table)
        .values((
            decision::at.eq(cx.at),
            decision::hamlet.eq(&decision.hamlet),
            decision::actor.eq(cx.actor),
            decision::kind.eq(kind),
            decision::subject.eq(decision.subject),
            decision::other.eq(decision.other),
            decision::scholar.eq(decision.scholar),
            decision::reason.eq(&decision.reason),
            decision::task.eq(decision.task),
            decision::detail
                .eq(decision.detail.map(|d| d.to_string())),
        ))
        .execute(cx.db)?;
    if let Some(hamlet) = decision.hamlet {
        cx.touched.hamlets.insert(hamlet);
    }
    Ok(())
}

/// The decisions of `hamlet` since `since`, oldest first.
pub(crate) fn of_hamlet(
    db: &mut Db,
    hamlet: &HamletId,
    since: i64,
) -> Result<Vec<Kept>> {
    Ok(decision::table
        .filter(decision::hamlet.eq(hamlet))
        .filter(decision::at.ge(since))
        .order(decision::id)
        .select(Kept::as_select())
        .load(db)?)
}

/// Every decision about `agent`, oldest first.
pub(crate) fn about(
    db: &mut Db,
    agent: AgentId,
) -> Result<Vec<Kept>> {
    Ok(decision::table
        .filter(
            decision::subject.eq(agent).or(decision::other.eq(agent)),
        )
        .order(decision::id)
        .select(Kept::as_select())
        .load(db)?)
}
