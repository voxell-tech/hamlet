//! Roads, along which agents send letters and questions (H3, H14,
//! H30): the tree's, the mayors', and those laid or agreed.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, HamletId, RoadId, TaskId};
use hamlet_store::schema::{agreement, road, road_charge};
use serde_json::json;

use super::addresses::name_of;
use super::agents::{self, AgentKind};
use super::hires::{self, Hire};
use super::record::{self, Decision, DecisionKind};
use super::refusal::Refusal;
use super::{hamlets, limits, tree};
use crate::change::{Change, Done, refuse};
use crate::text::text_enum;

text_enum! {
    pub(crate) enum RoadKind {
        /// Between two hires of a hamlet.
        Laid = "laid",
        /// Between hires of two hamlets, once both mayors agreed.
        Agreed = "agreed",
    }
}

/// How two agents are joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    /// A parent and its child, or the Hand and a mayor.
    Tree,
    /// Two mayors.
    Mayors,
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
        (AgentKind::Hand, AgentKind::Hire)
        | (AgentKind::Hire, AgentKind::Hand) => {
            let hire = if one.kind == AgentKind::Hire {
                from
            } else {
                to
            };
            let hire = hires::get(db, hire)?;
            Ok(is_mayor(db, &hire)?.then_some(Way::Tree))
        }
        (AgentKind::Hire, AgentKind::Hire) => {
            let (a, b) = (hires::get(db, from)?, hires::get(db, to)?);
            if a.parent == Some(to) || b.parent == Some(from) {
                return Ok(Some(Way::Tree));
            }
            if is_mayor(db, &a)? && is_mayor(db, &b)? {
                return Ok(Some(Way::Mayors));
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

fn is_mayor(db: &mut Db, hire: &Hire) -> Result<bool> {
    Ok(hire.parent.is_none()
        && hamlets::get(db, &hire.hamlet)?.is_living()
        && tree::mayor(db, &hire.hamlet)? == hire.agent)
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
            for mayor in tree::children(db, None)? {
                out.push((mayor, Way::Tree));
            }
        }
        AgentKind::Hire => {
            let hire = hires::get(db, id)?;
            match hire.parent {
                Some(parent) => out.push((parent, Way::Tree)),
                None => out.push((super::hand::id(db)?, Way::Tree)),
            }
            for child in tree::children(db, Some(id))? {
                out.push((child, Way::Tree));
            }
            if hire.parent.is_none() {
                for mayor in tree::children(db, None)? {
                    if mayor != id {
                        out.push((mayor, Way::Mayors));
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
    let living = hires::living_in(db, hamlet)?
        .into_iter()
        .map(|hire| hire.agent)
        .collect::<Vec<_>>();
    Ok(road::table
        .filter(road::kind.eq(RoadKind::Laid))
        .filter(road::up_at.is_null())
        .filter(road::a.eq_any(&living))
        .select(Road::as_select())
        .load(db)?)
}

/// Lays a road between two living hires of one hamlet, charged to the
/// hamlet and to `task` (H3).
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
    let (Some(one), Some(other)) =
        (hires::find(cx.db, a)?, hires::find(cx.db, b)?)
    else {
        refuse!(Refusal::RoadEnds);
    };
    for id in [a, b] {
        if !agents::get(cx.db, id)?.is_living() {
            refuse!(hires::left(cx.db, id)?);
        }
    }
    if one.hamlet != other.hamlet {
        refuse!(Refusal::OtherHamlet {
            name: name_of(cx.db, b)?
        });
    }
    if reason.trim().is_empty() {
        refuse!(Refusal::NoReason);
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

fn insert(
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
    if reason.trim().is_empty() {
        refuse!(Refusal::NoReason);
    }
    diesel::update(road::table.find(found.id))
        .set((
            road::up_at.eq(cx.at),
            road::up_by.eq(cx.actor),
            road::up_reason.eq(reason),
        ))
        .execute(cx.db)?;
    let hamlet = hires::get(cx.db, a)?.hamlet;
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

/// What a mayor's agreement to a road between hamlets came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Agreed {
    /// The other hamlet's mayor has not agreed yet.
    Waiting,
    Laid(RoadId),
}

/// Keeps the agreement of `mayor` to a road between hires `a` and `b`
/// of different hamlets, one of them its own, and lays the road once
/// the other hamlet's mayor agreed too (H14).
pub(crate) fn agree(
    cx: &mut Change<'_>,
    mayor: AgentId,
    a: AgentId,
    b: AgentId,
    reason: &str,
    task: Option<TaskId>,
) -> Done<Agreed> {
    if !limits::realm(cx.db)?.roads_between {
        refuse!(Refusal::NoRoadsBetween);
    }
    let (Some(one), Some(other)) =
        (hires::find(cx.db, a)?, hires::find(cx.db, b)?)
    else {
        refuse!(Refusal::RoadEnds);
    };
    for id in [a, b] {
        if !agents::get(cx.db, id)?.is_living() {
            refuse!(hires::left(cx.db, id)?);
        }
    }
    if one.hamlet == other.hamlet {
        refuse!(Refusal::SameHamlet);
    }
    if reason.trim().is_empty() {
        refuse!(Refusal::NoReason);
    }
    let Some(own) = hires::find(cx.db, mayor)?.filter(|m| {
        m.hamlet == one.hamlet || m.hamlet == other.hamlet
    }) else {
        refuse!(Refusal::NotMayor);
    };
    if tree::mayor(cx.db, &own.hamlet)? != mayor {
        refuse!(Refusal::NotMayor);
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
            agreement::mayor.eq(mayor),
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
