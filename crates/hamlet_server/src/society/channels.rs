//! Channels, along which agents send messages (H3, H14, H30).

use anyhow::Result;
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::sql_types::Text;
use hamlet_store::Db;
use hamlet_store::ids::{
    Actor, AgentId, ChannelId, HamletId, TaskId,
};
use hamlet_store::schema::{agreement, channel, channel_charge};
use serde::{Deserialize, Serialize};
use serde_json::json;
use strum::{Display, EnumString, IntoStaticStr};

use super::addresses::name_of;
use super::agents::{self, AgentKind};
use super::hamlet_agents::{self, HamletAgent};
use super::record::{self, Decision, DecisionKind};
use super::rejection::Rejection;
use super::{hamlets, limits, tree};
use crate::realm::change::{Change, Done, done, reject};

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
pub(crate) enum ChannelKind {
    /// Between two agents of a hamlet.
    Opened,
    /// Between agents of two hamlets, once both leads agreed.
    Agreed,
}
hamlet_store::named_text!(ChannelKind);

/// How two agents are joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    /// A parent and its child, or the Hand and a lead.
    Tree,
    /// Two leads.
    Leads,
    Channel(ChannelId),
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = channel)]
pub(crate) struct Channel {
    pub id: ChannelId,
    pub a: AgentId,
    pub b: AgentId,
    pub kind: ChannelKind,
    pub opened_at: i64,
    pub opened_by: Actor,
    pub reason: String,
    pub task: Option<TaskId>,
    pub inherited_from: Option<ChannelId>,
    pub closed_at: Option<i64>,
    pub closed_by: Option<Actor>,
    pub closed_reason: Option<String>,
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
            let Some(found) = between(db, from, to)? else {
                return Ok(None);
            };
            if found.kind == ChannelKind::Agreed
                && !limits::realm(db)?.channels_between
            {
                return Ok(None);
            }
            Ok(Some(Way::Channel(found.id)))
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
            let channels = channel::table
                .filter(channel::a.eq(id).or(channel::b.eq(id)))
                .filter(channel::closed_at.is_null())
                .select(Channel::as_select())
                .load(db)?;
            for found in channels {
                let other =
                    if found.a == id { found.b } else { found.a };
                if way(db, id, other)? == Some(Way::Channel(found.id))
                {
                    out.push((other, Way::Channel(found.id)));
                }
            }
        }
        AgentKind::Worker => {}
    }
    Ok(out)
}

/// The channel between `a` and `b` that is still open.
pub(crate) fn between(
    db: &mut Db,
    a: AgentId,
    b: AgentId,
) -> Result<Option<Channel>> {
    let (a, b) = (a.min(b), a.max(b));
    Ok(channel::table
        .filter(channel::a.eq(a))
        .filter(channel::b.eq(b))
        .filter(channel::closed_at.is_null())
        .select(Channel::as_select())
        .first(db)
        .optional()?)
}

/// The channels of `hamlet` that were opened and are still open.
pub(crate) fn opened_in(
    db: &mut Db,
    hamlet: &HamletId,
) -> Result<Vec<Channel>> {
    let living = hamlet_agents::living_in(db, hamlet)?
        .into_iter()
        .map(|agent| agent.agent)
        .collect::<Vec<_>>();
    Ok(channel::table
        .filter(channel::kind.eq(ChannelKind::Opened))
        .filter(channel::closed_at.is_null())
        .filter(channel::a.eq_any(&living))
        .select(Channel::as_select())
        .load(db)?)
}

/// Opens a channel between two living agents of one hamlet, charged
/// to the hamlet and to `task` (H3).
pub(crate) fn open(
    cx: &mut Change<'_>,
    a: AgentId,
    b: AgentId,
    reason: &str,
    task: Option<TaskId>,
) -> Done<ChannelId> {
    if a == b {
        reject!(Rejection::SameEnds);
    }
    let (Some(one), Some(other)) = (
        hamlet_agents::find(cx.db, a)?,
        hamlet_agents::find(cx.db, b)?,
    ) else {
        reject!(Rejection::ChannelEnds);
    };
    for id in [a, b] {
        done!(hamlet_agents::require_living(cx.db, id));
    }
    if one.hamlet != other.hamlet {
        reject!(Rejection::OtherHamlet {
            name: name_of(cx.db, b)?
        });
    }
    if way(cx.db, a, b)?.is_some() {
        reject!(Rejection::ChannelExists {
            a: name_of(cx.db, a)?,
            b: name_of(cx.db, b)?,
        });
    }
    let rules = limits::of(cx.db, &one.hamlet)?.channel;
    if opened_in(cx.db, &one.hamlet)?.len() as i64 >= rules.most {
        reject!(Rejection::TooManyChannels {
            hamlet: one.hamlet.to_string(),
            most: rules.most,
        });
    }
    let id = insert(cx, a, b, ChannelKind::Opened, reason, task)?;
    charge(cx, id, &one.hamlet, task, rules.price)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Channel, reason)
            .hamlet(&one.hamlet)
            .subject(a)
            .other(b),
    )?;
    Ok(Ok(id))
}

/// Opens a channel of `kind` between `a` and `b`.
pub(crate) fn insert(
    cx: &mut Change<'_>,
    a: AgentId,
    b: AgentId,
    kind: ChannelKind,
    reason: &str,
    task: Option<TaskId>,
) -> Result<ChannelId> {
    let (a, b) = (a.min(b), a.max(b));
    let id = diesel::insert_into(channel::table)
        .values((
            channel::a.eq(a),
            channel::b.eq(b),
            channel::kind.eq(kind),
            channel::opened_at.eq(cx.at),
            channel::opened_by.eq(cx.actor),
            channel::reason.eq(reason),
            channel::task.eq(task),
        ))
        .returning(channel::id)
        .get_result(cx.db)?;
    cx.touched.agents.extend([a, b]);
    Ok(id)
}

fn charge(
    cx: &mut Change<'_>,
    channel: ChannelId,
    hamlet: &HamletId,
    task: Option<TaskId>,
    micros: i64,
) -> Result<()> {
    let agent = match cx.actor {
        Actor::Agent(agent) => Some(agent),
        _ => None,
    };
    diesel::insert_into(channel_charge::table)
        .values((
            channel_charge::channel.eq(channel),
            channel_charge::hamlet.eq(hamlet),
            channel_charge::agent.eq(agent),
            channel_charge::task.eq(task),
            channel_charge::at.eq(cx.at),
            channel_charge::micros.eq(micros),
        ))
        .execute(cx.db)?;
    cx.touched.hamlets.insert(hamlet.clone());
    Ok(())
}

/// Closes the opened or agreed channel between `a` and `b`.
pub(crate) fn close(
    cx: &mut Change<'_>,
    a: AgentId,
    b: AgentId,
    reason: &str,
) -> Done<()> {
    let Some(found) = between(cx.db, a, b)? else {
        reject!(Rejection::NoChannel {
            a: name_of(cx.db, a)?,
            b: name_of(cx.db, b)?,
        });
    };
    diesel::update(channel::table.find(found.id))
        .set((
            channel::closed_at.eq(cx.at),
            channel::closed_by.eq(cx.actor),
            channel::closed_reason.eq(reason),
        ))
        .execute(cx.db)?;
    let hamlet = hamlet_agents::get(cx.db, a)?.hamlet;
    record::keep(
        cx,
        Decision::new(DecisionKind::Unchannel, reason)
            .hamlet(&hamlet)
            .subject(a)
            .other(b),
    )?;
    cx.touched.agents.extend([a, b]);
    Ok(Ok(()))
}

/// What a lead's agreement to a channel between hamlets came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Agreed {
    /// The other hamlet's lead has not agreed yet.
    Waiting,
    Opened(ChannelId),
}

/// Keeps the agreement of `lead` to a channel between agents of two
/// hamlets, opened once both leads agree (H14).
pub(crate) fn agree(
    cx: &mut Change<'_>,
    lead: AgentId,
    a: AgentId,
    b: AgentId,
    reason: &str,
    task: Option<TaskId>,
) -> Done<Agreed> {
    if !limits::realm(cx.db)?.channels_between {
        reject!(Rejection::NoChannelsBetween);
    }
    let (Some(one), Some(other)) = (
        hamlet_agents::find(cx.db, a)?,
        hamlet_agents::find(cx.db, b)?,
    ) else {
        reject!(Rejection::ChannelEnds);
    };
    for id in [a, b] {
        done!(hamlet_agents::require_living(cx.db, id));
    }
    if one.hamlet == other.hamlet {
        reject!(Rejection::SameHamlet);
    }
    let Some(own) = hamlet_agents::find(cx.db, lead)?.filter(|m| {
        m.hamlet == one.hamlet || m.hamlet == other.hamlet
    }) else {
        reject!(Rejection::NotLead);
    };
    if tree::lead(cx.db, &own.hamlet)? != lead {
        reject!(Rejection::NotLead);
    }
    if way(cx.db, a, b)?.is_some() {
        reject!(Rejection::ChannelExists {
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
        .filter(agreement::channel.is_null())
        .count()
        .get_result::<i64>(cx.db)?
        > 0;
    if !agreed_there {
        return Ok(Ok(Agreed::Waiting));
    }
    let id = insert(cx, a, b, ChannelKind::Agreed, reason, task)?;
    diesel::update(
        agreement::table
            .filter(agreement::a.eq(low))
            .filter(agreement::b.eq(high))
            .filter(agreement::channel.is_null()),
    )
    .set(agreement::channel.eq(id))
    .execute(cx.db)?;
    for hamlet in [&one.hamlet, &other.hamlet] {
        let price = limits::of(cx.db, hamlet)?.channel.price;
        charge(cx, id, hamlet, task, price)?;
        record::keep(
            cx,
            Decision::new(DecisionKind::ChannelAgreed, reason)
                .hamlet(hamlet)
                .subject(a)
                .other(b)
                .detail(json!({ "channel": id })),
        )?;
    }
    Ok(Ok(Agreed::Opened(id)))
}
