//! Roads, along which agents send letters and questions (H3, H14,
//! H30): the tree's, the leads', and those laid or agreed.

use anyhow::Result;
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::sql_types::Text;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, HamletId, RoadId, TaskId};
use hamlet_store::schema::{agreement, road, road_charge};
use serde::{Deserialize, Serialize};
use serde_json::json;
use strum::{Display, EnumString, IntoStaticStr};

use super::addresses::name_of;
use super::agents::{self, AgentKind};
use super::hamlet_agents::{self, HamletAgent};
use super::record::{self, Decision, DecisionKind};
use super::refusal::Refusal;
use super::{hamlets, limits, tree};
use crate::realm::change::{Change, Done, done, refuse};

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    AsExpression,
    FromSqlRow,
    IntoStaticStr,
    EnumString,
    Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[diesel(sql_type = Text)]
pub(crate) enum RoadKind {
    /// Between two agents of a hamlet.
    Laid,
    /// Between agents of two hamlets, once both leads agreed.
    Agreed,
}
hamlet_store::named_text!(RoadKind);

/// How two agents are joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    /// A parent and its child, or the Hand and a lead.
    Tree,
    /// Two leads.
    Leads,
    Road(RoadId),
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = road)]
pub(crate) struct Road {
    pub id: RoadId,
    pub a: AgentId,
    pub b: AgentId,
    pub kind: RoadKind,
    pub laid_at: i64,
    pub laid_by: Actor,
    pub reason: String,
    pub task: Option<TaskId>,
    pub inherited_from: Option<RoadId>,
    pub up_at: Option<i64>,
    pub up_by: Option<Actor>,
    pub up_reason: Option<String>,
}

/// How `from` reaches `to`, if it can: both living, and joined.
pub(crate) fn way(
    db: &mut Db,
    from: AgentId,
    to: AgentId,
) -> Result<Option<Way>> {
    if from == to {
        return Ok(None);
    }
    let (one, other) = (agents::get(db, from)?, agents::get(db, to)?);
    if !one.is_living() || !other.is_living() {
        return Ok(None);
    }
    match (one.kind, other.kind) {
        (AgentKind::Hand, AgentKind::Hamlet)
        | (AgentKind::Hamlet, AgentKind::Hand) => {
            let agent = if one.kind == AgentKind::Hamlet {
                from
            } else {
                to
            };
            let agent = hamlet_agents::get(db, agent)?;
            Ok(is_lead(db, &agent)?.then_some(Way::Tree))
        }
        (AgentKind::Hamlet, AgentKind::Hamlet) => {
            let (a, b) = (
                hamlet_agents::get(db, from)?,
                hamlet_agents::get(db, to)?,
            );
            if a.parent == Some(to) || b.parent == Some(from) {
                return Ok(Some(Way::Tree));
            }
            if is_lead(db, &a)? && is_lead(db, &b)? {
                return Ok(Some(Way::Leads));
            }
            let Some(found) = open(db, from, to)? else {
                return Ok(None);
            };
            if found.kind == RoadKind::Agreed
                && !limits::realm(db)?.roads_between
            {
                return Ok(None);
            }
            Ok(Some(Way::Road(found.id)))
        }
        _ => Ok(None),
    }
}

fn is_lead(db: &mut Db, agent: &HamletAgent) -> Result<bool> {
    Ok(agent.parent.is_none()
        && hamlets::get(db, &agent.hamlet)?.is_living()
        && tree::lead(db, &agent.hamlet)? == agent.agent)
}

/// Every agent `id` reaches, and how.
pub(crate) fn ways(
    db: &mut Db,
    id: AgentId,
) -> Result<Vec<(AgentId, Way)>> {
    let found = agents::get(db, id)?;
    if !found.is_living() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    match found.kind {
        AgentKind::Hand => {
            for lead in tree::children(db, None)? {
                out.push((lead, Way::Tree));
            }
        }
        AgentKind::Hamlet => {
            let agent = hamlet_agents::get(db, id)?;
            match agent.parent {
                Some(parent) => out.push((parent, Way::Tree)),
                None => out.push((super::hand::id(db)?, Way::Tree)),
            }
            for child in tree::children(db, Some(id))? {
                out.push((child, Way::Tree));
            }
            if agent.parent.is_none() {
                for lead in tree::children(db, None)? {
                    if lead != id {
                        out.push((lead, Way::Leads));
                    }
                }
            }
            let roads = road::table
                .filter(road::a.eq(id).or(road::b.eq(id)))
                .filter(road::up_at.is_null())
                .select(Road::as_select())
                .load(db)?;
            for found in roads {
                let other =
                    if found.a == id { found.b } else { found.a };
                if way(db, id, other)? == Some(Way::Road(found.id)) {
                    out.push((other, Way::Road(found.id)));
                }
            }
        }
        AgentKind::Worker => {}
    }
    Ok(out)
}

/// The open road between `a` and `b`.
pub(crate) fn open(
    db: &mut Db,
    a: AgentId,
    b: AgentId,
) -> Result<Option<Road>> {
    let (a, b) = (a.min(b), a.max(b));
    Ok(road::table
        .filter(road::a.eq(a))
        .filter(road::b.eq(b))
        .filter(road::up_at.is_null())
        .select(Road::as_select())
        .first(db)
        .optional()?)
}

/// The open laid roads of `hamlet`.
pub(crate) fn laid_in(
    db: &mut Db,
    hamlet: &HamletId,
) -> Result<Vec<Road>> {
    let living = hamlet_agents::living_in(db, hamlet)?
        .into_iter()
        .map(|agent| agent.agent)
        .collect::<Vec<_>>();
    Ok(road::table
        .filter(road::kind.eq(RoadKind::Laid))
        .filter(road::up_at.is_null())
        .filter(road::a.eq_any(&living))
        .select(Road::as_select())
        .load(db)?)
}

/// Lays a road between two living agents of one hamlet, charged to
/// the hamlet and to `task` (H3).
pub(crate) fn lay(
    cx: &mut Change<'_>,
    a: AgentId,
    b: AgentId,
    reason: &str,
    task: Option<TaskId>,
) -> Done<RoadId> {
    if a == b {
        refuse!(Refusal::SameEnds);
    }
    let (Some(one), Some(other)) = (
        hamlet_agents::find(cx.db, a)?,
        hamlet_agents::find(cx.db, b)?,
    ) else {
        refuse!(Refusal::RoadEnds);
    };
    for id in [a, b] {
        done!(hamlet_agents::require_living(cx.db, id));
    }
    if one.hamlet != other.hamlet {
        refuse!(Refusal::OtherHamlet {
            name: name_of(cx.db, b)?
        });
    }
    if way(cx.db, a, b)?.is_some() {
        refuse!(Refusal::RoadExists {
            a: name_of(cx.db, a)?,
            b: name_of(cx.db, b)?,
        });
    }
    let rules = limits::of(cx.db, &one.hamlet)?.road;
    if laid_in(cx.db, &one.hamlet)?.len() as i64 >= rules.most {
        refuse!(Refusal::TooManyRoads {
            hamlet: one.hamlet.to_string(),
            most: rules.most,
        });
    }
    let id = insert(cx, a, b, RoadKind::Laid, reason, task)?;
    charge(cx, id, &one.hamlet, task, rules.price)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Road, reason)
            .hamlet(&one.hamlet)
            .subject(a)
            .other(b),
    )?;
    Ok(Ok(id))
}

/// Lays a road of `kind` between `a` and `b`.
pub(crate) fn insert(
    cx: &mut Change<'_>,
    a: AgentId,
    b: AgentId,
    kind: RoadKind,
    reason: &str,
    task: Option<TaskId>,
) -> Result<RoadId> {
    let (a, b) = (a.min(b), a.max(b));
    let id = diesel::insert_into(road::table)
        .values((
            road::a.eq(a),
            road::b.eq(b),
            road::kind.eq(kind),
            road::laid_at.eq(cx.at),
            road::laid_by.eq(cx.actor),
            road::reason.eq(reason),
            road::task.eq(task),
        ))
        .returning(road::id)
        .get_result(cx.db)?;
    cx.touched.agents.extend([a, b]);
    Ok(id)
}

fn charge(
    cx: &mut Change<'_>,
    road: RoadId,
    hamlet: &HamletId,
    task: Option<TaskId>,
    micros: i64,
) -> Result<()> {
    let agent = match cx.actor {
        Actor::Agent(agent) => Some(agent),
        _ => None,
    };
    diesel::insert_into(road_charge::table)
        .values((
            road_charge::road.eq(road),
            road_charge::hamlet.eq(hamlet),
            road_charge::agent.eq(agent),
            road_charge::task.eq(task),
            road_charge::at.eq(cx.at),
            road_charge::micros.eq(micros),
        ))
        .execute(cx.db)?;
    cx.touched.hamlets.insert(hamlet.clone());
    Ok(())
}

/// Takes up the laid or agreed road between `a` and `b`. What it cost
/// is not given back.
pub(crate) fn take_up(
    cx: &mut Change<'_>,
    a: AgentId,
    b: AgentId,
    reason: &str,
) -> Done<()> {
    let Some(found) = open(cx.db, a, b)? else {
        refuse!(Refusal::NoRoad {
            a: name_of(cx.db, a)?,
            b: name_of(cx.db, b)?,
        });
    };
    diesel::update(road::table.find(found.id))
        .set((
            road::up_at.eq(cx.at),
            road::up_by.eq(cx.actor),
            road::up_reason.eq(reason),
        ))
        .execute(cx.db)?;
    let hamlet = hamlet_agents::get(cx.db, a)?.hamlet;
    record::keep(
        cx,
        Decision::new(DecisionKind::Unroad, reason)
            .hamlet(&hamlet)
            .subject(a)
            .other(b),
    )?;
    cx.touched.agents.extend([a, b]);
    Ok(Ok(()))
}

/// What a lead's agreement to a road between hamlets came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Agreed {
    /// The other hamlet's lead has not agreed yet.
    Waiting,
    Laid(RoadId),
}

/// Keeps the agreement of `lead` to a road between agents `a` and `b`
/// of different hamlets, one of them its own, and lays the road once
/// the other hamlet's lead agreed too (H14).
pub(crate) fn agree(
    cx: &mut Change<'_>,
    lead: AgentId,
    a: AgentId,
    b: AgentId,
    reason: &str,
    task: Option<TaskId>,
) -> Done<Agreed> {
    if !limits::realm(cx.db)?.roads_between {
        refuse!(Refusal::NoRoadsBetween);
    }
    let (Some(one), Some(other)) = (
        hamlet_agents::find(cx.db, a)?,
        hamlet_agents::find(cx.db, b)?,
    ) else {
        refuse!(Refusal::RoadEnds);
    };
    for id in [a, b] {
        done!(hamlet_agents::require_living(cx.db, id));
    }
    if one.hamlet == other.hamlet {
        refuse!(Refusal::SameHamlet);
    }
    let Some(own) = hamlet_agents::find(cx.db, lead)?.filter(|m| {
        m.hamlet == one.hamlet || m.hamlet == other.hamlet
    }) else {
        refuse!(Refusal::NotLead);
    };
    if tree::lead(cx.db, &own.hamlet)? != lead {
        refuse!(Refusal::NotLead);
    }
    if way(cx.db, a, b)?.is_some() {
        refuse!(Refusal::RoadExists {
            a: name_of(cx.db, a)?,
            b: name_of(cx.db, b)?,
        });
    }
    let (low, high) = (a.min(b), a.max(b));
    diesel::insert_into(agreement::table)
        .values((
            agreement::a.eq(low),
            agreement::b.eq(high),
            agreement::hamlet.eq(&own.hamlet),
            agreement::lead.eq(lead),
            agreement::at.eq(cx.at),
            agreement::reason.eq(reason),
            agreement::task.eq(task),
        ))
        .on_conflict_do_nothing()
        .execute(cx.db)?;
    let theirs = if own.hamlet == one.hamlet {
        &other.hamlet
    } else {
        &one.hamlet
    };
    let agreed_there = agreement::table
        .filter(agreement::a.eq(low))
        .filter(agreement::b.eq(high))
        .filter(agreement::hamlet.eq(theirs))
        .filter(agreement::road.is_null())
        .count()
        .get_result::<i64>(cx.db)?
        > 0;
    if !agreed_there {
        return Ok(Ok(Agreed::Waiting));
    }
    let id = insert(cx, a, b, RoadKind::Agreed, reason, task)?;
    diesel::update(
        agreement::table
            .filter(agreement::a.eq(low))
            .filter(agreement::b.eq(high))
            .filter(agreement::road.is_null()),
    )
    .set(agreement::road.eq(id))
    .execute(cx.db)?;
    for hamlet in [&one.hamlet, &other.hamlet] {
        let price = limits::of(cx.db, hamlet)?.road.price;
        charge(cx, id, hamlet, task, price)?;
        record::keep(
            cx,
            Decision::new(DecisionKind::RoadAgreed, reason)
                .hamlet(hamlet)
                .subject(a)
                .other(b)
                .detail(json!({ "road": id })),
        )?;
    }
    Ok(Ok(Agreed::Laid(id)))
}
