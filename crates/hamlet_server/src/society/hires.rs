//! Hires: scholars working in a hamlet, each with a branch, a lodge
//! and a place in the hamlet's tree.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{
    Actor, AgentId, ChiefId, HamletId, ScholarId,
};
use hamlet_store::schema::{agent, hamlet, hire, hire_tool_off};
use serde_json::json;

use super::agents::{self, AgentKind, EndHow};
use super::city::{self, ScholarKind};
use super::limits::{self, Micros};
use super::record::{self, Decision, DecisionKind};
use super::refusal::Refusal;
use super::{hamlets, tree};
use crate::change::{Change, Done, done, refuse};
use crate::jobs::{self, Job};
use crate::text::text_enum;

text_enum! {
    /// How hard a hire's model thinks (H29a).
    pub(crate) enum Effort {
        Off = "off",
        Minimal = "minimal",
        Low = "low",
        Medium = "medium",
        High = "high",
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = hire)]
pub(crate) struct Hire {
    pub agent: AgentId,
    pub hamlet: HamletId,
    pub number: i64,
    pub scholar: ScholarId,
    pub parent: Option<AgentId>,
    pub purpose: String,
    pub pin: Option<String>,
    pub base: Option<String>,
    pub model: Option<String>,
    pub model_by: Option<Actor>,
    pub model_reason: Option<String>,
    pub effort: Option<Effort>,
    pub allowance: Option<Micros>,
    pub allowance_by: Option<Actor>,
    pub merged_into: Option<AgentId>,
}

/// The scholar a hire is of.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Hiring {
    /// A new scholar of the city, made for the hire.
    New { name: String, character: String },
    /// A living scholar, at its `main` or pinned to a version.
    Existing {
        scholar: ScholarId,
        pin: Option<String>,
    },
}

/// A hire to make.
#[derive(Clone, Debug)]
pub(crate) struct Spec {
    pub hamlet: HamletId,
    /// The hire it answers to; `None` makes the hamlet's mayor.
    pub parent: Option<AgentId>,
    pub hiring: Hiring,
    pub purpose: String,
    pub reason: String,
    /// The chief a new scholar belongs to.
    pub owner: ChiefId,
}

/// A hire just made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hired {
    pub agent: AgentId,
    pub scholar: ScholarId,
    pub number: i64,
}

/// Hires a scholar into a hamlet under a parent. With no parent, the
/// hire is the mayor of a hamlet being founded.
pub(crate) fn hire(cx: &mut Change<'_>, spec: Spec) -> Done<Hired> {
    let place = hamlets::get(cx.db, &spec.hamlet)?;
    if !place.is_living() {
        refuse!(Refusal::HamletRemoved {
            id: spec.hamlet.to_string()
        });
    }
    if spec.purpose.trim().is_empty() {
        refuse!(Refusal::NoPurpose);
    }
    if count_living(cx.db, &spec.hamlet)? >= place.tier {
        refuse!(Refusal::HamletFull {
            id: spec.hamlet.to_string(),
            most: place.tier,
        });
    }
    if let Some(parent) = spec.parent {
        let above = get(cx.db, parent)?;
        if above.hamlet != spec.hamlet {
            refuse!(Refusal::OtherHamlet {
                name: super::addresses::name_of(cx.db, parent)?
            });
        }
        if !agents::get(cx.db, parent)?.is_living() {
            refuse!(left(cx.db, parent)?);
        }
        let most = limits::of(cx.db, &spec.hamlet)?.depth;
        if tree::depth(cx.db, parent)? + 1 > most {
            refuse!(Refusal::TooDeep { most });
        }
    }
    let (scholar, pin, new) = match spec.hiring {
        Hiring::New { name, character } => {
            let made = done!(city::make(
                cx, &name, spec.owner, &character, None
            ));
            (made, None, true)
        }
        Hiring::Existing { scholar, pin } => {
            let found = city::get(cx.db, scholar)?;
            if !found.is_living()
                || found.kind != ScholarKind::Scholar
            {
                refuse!(Refusal::ScholarGone { name: found.name });
            }
            (scholar, pin, false)
        }
    };
    let agent = agents::make(cx, AgentKind::Hire)?;
    let number = place.next_number;
    diesel::update(hamlet::table.find(&spec.hamlet))
        .set(hamlet::next_number.eq(number + 1))
        .execute(cx.db)?;
    diesel::insert_into(hire::table)
        .values((
            hire::agent.eq(agent),
            hire::hamlet.eq(&spec.hamlet),
            hire::number.eq(number),
            hire::scholar.eq(scholar),
            hire::parent.eq(spec.parent),
            hire::purpose.eq(spec.purpose.trim()),
            hire::pin.eq(&pin),
        ))
        .execute(cx.db)?;
    jobs::add(cx, &Job::Lodge { hire: agent })?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Hired, &spec.reason)
            .hamlet(&spec.hamlet)
            .subject(agent)
            .scholar(scholar)
            .detail(json!({
                "new": new,
                "pin": pin,
                "purpose": spec.purpose.trim(),
            })),
    )?;
    cx.touched.hamlets.insert(spec.hamlet);
    cx.touched.scholars.insert(scholar);
    Ok(Ok(Hired {
        agent,
        scholar,
        number,
    }))
}

pub(crate) fn find(db: &mut Db, id: AgentId) -> Result<Option<Hire>> {
    Ok(hire::table
        .find(id)
        .select(Hire::as_select())
        .first(db)
        .optional()?)
}

pub(crate) fn get(db: &mut Db, id: AgentId) -> Result<Hire> {
    find(db, id)?.with_context(|| format!("no hire {id}"))
}

/// The living hires of `hamlet`, by number.
pub(crate) fn living_in(
    db: &mut Db,
    hamlet: &HamletId,
) -> Result<Vec<Hire>> {
    Ok(hire::table
        .inner_join(agent::table)
        .filter(hire::hamlet.eq(hamlet))
        .filter(agent::ended_at.is_null())
        .order(hire::number)
        .select(Hire::as_select())
        .load(db)?)
}

/// The living hires of the scholar `scholar`.
pub(crate) fn living_of(
    db: &mut Db,
    scholar: ScholarId,
) -> Result<Vec<Hire>> {
    Ok(hire::table
        .inner_join(agent::table)
        .filter(hire::scholar.eq(scholar))
        .filter(agent::ended_at.is_null())
        .order(hire::agent)
        .select(Hire::as_select())
        .load(db)?)
}

pub(crate) fn count_living(
    db: &mut Db,
    hamlet: &HamletId,
) -> Result<i64> {
    Ok(hire::table
        .inner_join(agent::table)
        .filter(hire::hamlet.eq(hamlet))
        .filter(agent::ended_at.is_null())
        .count()
        .get_result(db)?)
}

/// Whether `id` holds its hamlet's office.
pub(crate) fn is_mayor(db: &mut Db, id: AgentId) -> Result<bool> {
    let hire = get(db, id)?;
    Ok(tree::mayor(db, &hire.hamlet)? == id)
}

/// The refusal of a call that names the ended hire `id`.
pub(crate) fn left(db: &mut Db, id: AgentId) -> Result<Refusal> {
    let agent = agents::get(db, id)?;
    let name = super::addresses::name_of(db, id)?;
    Ok(match agent.ended_how {
        _ if agent.erased => Refusal::Erased { name },
        Some(how) => Refusal::Left {
            name,
            how: how.words().into(),
        },
        None => Refusal::NotLeft { name },
    })
}

/// Ends the living hire `id`. Its children go to its parent, but when
/// its whole hamlet is removed and the tree is kept to come back.
pub(crate) fn end(
    cx: &mut Change<'_>,
    id: AgentId,
    how: EndHow,
    reason: &str,
) -> Done<()> {
    let hire = get(cx.db, id)?;
    if !agents::get(cx.db, id)?.is_living() {
        refuse!(left(cx.db, id)?);
    }
    if how != EndHow::Hamlet {
        if reason.trim().is_empty() {
            refuse!(Refusal::NoReason);
        }
        if is_mayor(cx.db, id)? {
            refuse!(Refusal::EndingMayor);
        }
        let children = tree::children(cx.db, Some(id))?;
        diesel::update(
            hire::table.filter(hire::agent.eq_any(&children)),
        )
        .set(hire::parent.eq(hire.parent))
        .execute(cx.db)?;
        cx.touched.agents.extend(children);
        record::keep(
            cx,
            Decision::new(DecisionKind::Ended, reason)
                .hamlet(&hire.hamlet)
                .subject(id)
                .scholar(hire.scholar)
                .detail(json!({ "how": how })),
        )?;
    }
    agents::end(cx, id, how, reason)?;
    jobs::add(cx, &Job::Commit { hire: id })?;
    cx.touched.hamlets.insert(hire.hamlet);
    cx.touched.scholars.insert(hire.scholar);
    Ok(Ok(()))
}

/// Brings back the ended hire `id` under its old parent, or the mayor
/// when that has gone.
pub(crate) fn come_back(
    cx: &mut Change<'_>,
    id: AgentId,
    reason: &str,
) -> Done<()> {
    let hire = get(cx.db, id)?;
    let agent = agents::get(cx.db, id)?;
    if agent.is_living() || agent.erased {
        refuse!(left(cx.db, id)?);
    }
    let place = hamlets::get(cx.db, &hire.hamlet)?;
    if !place.is_living() {
        refuse!(Refusal::HamletRemoved {
            id: hire.hamlet.to_string()
        });
    }
    let scholar = city::get(cx.db, hire.scholar)?;
    if !scholar.is_living() {
        refuse!(Refusal::ScholarGone { name: scholar.name });
    }
    if count_living(cx.db, &hire.hamlet)? >= place.tier {
        refuse!(Refusal::HamletFull {
            id: hire.hamlet.to_string(),
            most: place.tier,
        });
    }
    let most = limits::of(cx.db, &hire.hamlet)?.depth;
    let mayor = tree::mayor(cx.db, &hire.hamlet)?;
    let parent = match hire.parent {
        Some(parent)
            if parent != id
                && agents::get(cx.db, parent)?.is_living()
                && tree::depth(cx.db, parent)? < most =>
        {
            parent
        }
        _ => mayor,
    };
    diesel::update(hire::table.find(id))
        .set((
            hire::parent.eq(Some(parent)),
            hire::merged_into.eq(None::<AgentId>),
        ))
        .execute(cx.db)?;
    agents::come_back(cx, id)?;
    jobs::add(cx, &Job::Lodge { hire: id })?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Restored, reason)
            .hamlet(&hire.hamlet)
            .subject(id)
            .scholar(hire.scholar),
    )?;
    cx.touched.hamlets.insert(hire.hamlet);
    cx.touched.scholars.insert(hire.scholar);
    Ok(Ok(()))
}

/// Erases the ended hire `id` for good: its lodge and branch go.
pub(crate) fn erase(cx: &mut Change<'_>, id: AgentId) -> Done<()> {
    let hire = get(cx.db, id)?;
    let agent = agents::get(cx.db, id)?;
    if agent.is_living() || agent.erased {
        refuse!(left(cx.db, id)?);
    }
    agents::erase(cx, id)?;
    jobs::add(cx, &Job::EraseHire { hire: id })?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Erased, "")
            .hamlet(&hire.hamlet)
            .subject(id)
            .scholar(hire.scholar),
    )?;
    cx.touched.hamlets.insert(hire.hamlet);
    Ok(Ok(()))
}

/// Gives the office of `hamlet` to its hire `id`. The old mayor and
/// its children go under the new one.
pub(crate) fn seat(
    cx: &mut Change<'_>,
    hamlet_id: &HamletId,
    id: AgentId,
    reason: &str,
) -> Done<()> {
    let hire = get(cx.db, id)?;
    if &hire.hamlet != hamlet_id {
        refuse!(Refusal::OtherHamlet {
            name: super::addresses::name_of(cx.db, id)?
        });
    }
    if !agents::get(cx.db, id)?.is_living() {
        refuse!(left(cx.db, id)?);
    }
    let old = tree::mayor(cx.db, hamlet_id)?;
    if old == id {
        refuse!(Refusal::AlreadyMayor {
            name: super::addresses::name_of(cx.db, id)?
        });
    }
    if reason.trim().is_empty() {
        refuse!(Refusal::NoReason);
    }
    let moved = tree::children(cx.db, Some(old))?
        .into_iter()
        .filter(|child| *child != id)
        .collect::<Vec<_>>();
    diesel::update(hire::table.filter(hire::agent.eq_any(&moved)))
        .set(hire::parent.eq(Some(id)))
        .execute(cx.db)?;
    diesel::update(hire::table.find(old))
        .set(hire::parent.eq(Some(id)))
        .execute(cx.db)?;
    diesel::update(hire::table.find(id))
        .set(hire::parent.eq(None::<AgentId>))
        .execute(cx.db)?;
    diesel::update(hamlet::table.find(hamlet_id))
        .set(hamlet::mayor.eq(id))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Seated, reason)
            .hamlet(hamlet_id)
            .subject(id)
            .other(old),
    )?;
    cx.touched.agents.extend(moved);
    cx.touched.agents.extend([old, id]);
    cx.touched.hamlets.insert(hamlet_id.clone());
    Ok(Ok(()))
}

/// Chooses the model of the living hire `id`, or its hamlet's when
/// `None` (H29). A lead may not change a chief's choice.
pub(crate) fn set_model(
    cx: &mut Change<'_>,
    id: AgentId,
    model: Option<&str>,
    reason: &str,
) -> Done<()> {
    let hire = get(cx.db, id)?;
    if !agents::get(cx.db, id)?.is_living() {
        refuse!(left(cx.db, id)?);
    }
    if matches!(hire.model_by, Some(Actor::Chief(_)))
        && !matches!(cx.actor, Actor::Chief(_))
    {
        refuse!(Refusal::ChiefChose {
            name: super::addresses::name_of(cx.db, id)?
        });
    }
    if let Some(model) = model {
        let allowed = limits::of(cx.db, &hire.hamlet)?.models;
        if !allowed.is_empty() && !allowed.iter().any(|m| m == model)
        {
            refuse!(Refusal::ModelNotAllowed {
                model: model.into()
            });
        }
    }
    diesel::update(hire::table.find(id))
        .set((
            hire::model.eq(model),
            hire::model_by.eq(model.map(|_| cx.actor)),
            hire::model_reason.eq(model.map(|_| reason)),
        ))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Model, reason)
            .hamlet(&hire.hamlet)
            .subject(id)
            .detail(json!({ "model": model })),
    )?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// Turns tools of the living hire `id` off, and back on.
pub(crate) fn set_tools(
    cx: &mut Change<'_>,
    id: AgentId,
    off: &[String],
    on: &[String],
    reason: &str,
) -> Done<()> {
    let hire = get(cx.db, id)?;
    if !agents::get(cx.db, id)?.is_living() {
        refuse!(left(cx.db, id)?);
    }
    for tool in off {
        diesel::insert_into(hire_tool_off::table)
            .values((
                hire_tool_off::hire.eq(id),
                hire_tool_off::tool.eq(tool),
                hire_tool_off::at.eq(cx.at),
                hire_tool_off::by.eq(cx.actor),
            ))
            .on_conflict_do_nothing()
            .execute(cx.db)?;
    }
    diesel::delete(
        hire_tool_off::table
            .filter(hire_tool_off::hire.eq(id))
            .filter(hire_tool_off::tool.eq_any(on)),
    )
    .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Tools, reason)
            .hamlet(&hire.hamlet)
            .subject(id)
            .detail(json!({ "off": off, "on": on })),
    )?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// The tools turned off for `id`.
pub(crate) fn tools_off(
    db: &mut Db,
    id: AgentId,
) -> Result<Vec<String>> {
    Ok(hire_tool_off::table
        .filter(hire_tool_off::hire.eq(id))
        .order(hire_tool_off::tool)
        .select(hire_tool_off::tool)
        .load(db)?)
}

/// Gives the living hire `id` an allowance for its hamlet's period,
/// or none.
pub(crate) fn set_allowance(
    cx: &mut Change<'_>,
    id: AgentId,
    allowance: Option<Micros>,
    reason: &str,
) -> Done<()> {
    let hire = get(cx.db, id)?;
    if !agents::get(cx.db, id)?.is_living() {
        refuse!(left(cx.db, id)?);
    }
    diesel::update(hire::table.find(id))
        .set((
            hire::allowance.eq(allowance),
            hire::allowance_by.eq(allowance.map(|_| cx.actor)),
        ))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Allowance, reason)
            .hamlet(&hire.hamlet)
            .subject(id)
            .detail(json!({ "micros": allowance })),
    )?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// Keeps the commit of `main`, or the pin, that the branch of `id`
/// took in last.
pub(crate) fn took_in(
    db: &mut Db,
    id: AgentId,
    base: &str,
) -> Result<()> {
    diesel::update(hire::table.find(id))
        .set(hire::base.eq(base))
        .execute(db)?;
    Ok(())
}
