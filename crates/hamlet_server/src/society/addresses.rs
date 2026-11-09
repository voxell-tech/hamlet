//! Addresses: how chiefs and agents name the Hand, mayors, hires,
//! workers and scholars, by ids and never by a scholar's name (C30,
//! H14).

use std::fmt;

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, HamletId, ScholarId};
use hamlet_store::schema::{agent, hire, scholar};

use super::agents::{self, AgentKind};
use super::names::check_hamlet_id;
use super::refusal::Refusal;
use super::{chiefs, hamlets, hand, tree};
use crate::change::{Done, done, refuse};

/// A name for an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Address {
    Hand,
    /// A hamlet's mayor, or the speaker's own.
    Mayor {
        hamlet: Option<HamletId>,
    },
    /// The hire of that number in a hamlet.
    Hire {
        hamlet: HamletId,
        number: i64,
    },
    Worker(AgentId),
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hand => f.write_str("hand"),
            Self::Mayor { hamlet: None } => f.write_str("mayor"),
            Self::Mayor {
                hamlet: Some(hamlet),
            } => write!(f, "mayor@{hamlet}"),
            Self::Hire { hamlet, number } => {
                write!(f, "{hamlet}#{number}")
            }
            Self::Worker(id) => write!(f, "worker#{id}"),
        }
    }
}

/// Which agents an address may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    Living,
    /// The ended too, but not the erased.
    Kept,
}

/// Reads an address: `hand`, `mayor`, `mayor@<hamlet>`,
/// `<hamlet>#<number>` or `worker#<id>`.
pub(crate) fn parse(text: &str) -> Result<Address, Refusal> {
    let text = text.trim();
    let bad = || Refusal::BadAddress {
        address: text.into(),
    };
    if text == "hand" {
        return Ok(Address::Hand);
    }
    if let Some(id) = text.strip_prefix("worker#") {
        let id = id.parse::<i64>().map_err(|_| bad())?;
        return Ok(Address::Worker(AgentId(id)));
    }
    if text == "mayor" {
        return Ok(Address::Mayor { hamlet: None });
    }
    if let Some(hamlet) = text.strip_prefix("mayor@") {
        check_hamlet_id(hamlet).map_err(|_| bad())?;
        return Ok(Address::Mayor {
            hamlet: Some(HamletId(hamlet.into())),
        });
    }
    let (hamlet, number) = text.split_once('#').ok_or_else(bad)?;
    check_hamlet_id(hamlet).map_err(|_| bad())?;
    let number = number.parse::<i64>().map_err(|_| bad())?;
    if number < 0 {
        return Err(bad());
    }
    Ok(Address::Hire {
        hamlet: HamletId(hamlet.into()),
        number,
    })
}

/// Reads a scholar's address, `scholar#<id>`.
pub(crate) fn parse_scholar(
    text: &str,
) -> Result<ScholarId, Refusal> {
    let text = text.trim();
    text.strip_prefix("scholar#")
        .and_then(|id| id.parse::<i64>().ok())
        .map(ScholarId)
        .ok_or_else(|| Refusal::BadAddress {
            address: text.into(),
        })
}

/// The address of the scholar `id`.
pub(crate) fn scholar_address(id: ScholarId) -> String {
    format!("scholar#{id}")
}

/// The agent `address` names, said in `here`, the speaker's hamlet.
pub(crate) fn resolve(
    db: &mut Db,
    address: &Address,
    here: Option<&HamletId>,
    reach: Reach,
) -> Done<AgentId> {
    let none = || Refusal::NoOne {
        address: address.to_string(),
    };
    match address {
        Address::Hand => Ok(Ok(hand::id(db)?)),
        Address::Mayor { hamlet } => {
            let Some(hamlet) = hamlet.as_ref().or(here) else {
                refuse!(none());
            };
            done!(hamlets::living(db, hamlet));
            Ok(Ok(tree::mayor(db, hamlet)?))
        }
        Address::Worker(id) => {
            let found = agents::find(db, *id)?;
            match found {
                Some(found)
                    if found.kind == AgentKind::Worker
                        && (reach == Reach::Kept
                            || found.is_living()) =>
                {
                    Ok(Ok(*id))
                }
                _ => refuse!(none()),
            }
        }
        Address::Hire { hamlet, number } => {
            let mut query = hire::table
                .inner_join(agent::table)
                .inner_join(scholar::table)
                .filter(hire::hamlet.eq(hamlet))
                .filter(hire::number.eq(number))
                .filter(agent::erased.eq(false))
                .select(hire::agent)
                .into_boxed();
            if reach == Reach::Living {
                query = query
                    .filter(agent::ended_at.is_null())
                    .filter(scholar::left_at.is_null());
            }
            match query.first::<AgentId>(db).optional()? {
                Some(found) => Ok(Ok(found)),
                None => refuse!(none()),
            }
        }
    }
}

/// Who `actor` is, as chiefs and agents are told.
pub(crate) fn actor_name(
    db: &mut Db,
    actor: Option<Actor>,
) -> Result<String> {
    Ok(match actor {
        Some(Actor::Chief(chief)) => chiefs::find(db, chief)?
            .map(|c| c.name)
            .unwrap_or_default(),
        Some(Actor::Agent(agent)) => name_of(db, agent)?,
        Some(Actor::Realm) | None => "the realm".into(),
    })
}

/// Who `actor` is, as the address that names it when it is an agent.
pub(crate) fn actor_address(
    db: &mut Db,
    actor: Option<Actor>,
) -> Result<String> {
    Ok(match actor {
        Some(Actor::Agent(agent)) => address_of(db, agent)?,
        other => actor_name(db, other)?,
    })
}

/// The address of `id`: `hand`, `hamlet#number` or `worker#id` (C30).
pub(crate) fn address_of(db: &mut Db, id: AgentId) -> Result<String> {
    Ok(match agents::get(db, id)?.kind {
        AgentKind::Hand => "hand".into(),
        AgentKind::Worker => format!("worker#{id}"),
        AgentKind::Hire => {
            let (hamlet, number) = hire::table
                .find(id)
                .select((hire::hamlet, hire::number))
                .first::<(HamletId, i64)>(db)?;
            format!("{hamlet}#{number}")
        }
    })
}

/// How `id` is shown to chiefs and agents: its address, and beside it
/// the nickname of a hire, or else its scholar's name, as in
/// `orchard#3 (Mira)`. An erased hire shows its address only.
pub(crate) fn name_of(db: &mut Db, id: AgentId) -> Result<String> {
    let address = address_of(db, id)?;
    let found = agents::get(db, id)?;
    if found.kind != AgentKind::Hire || found.erased {
        return Ok(address);
    }
    let (nickname, name) = hire::table
        .inner_join(scholar::table)
        .filter(hire::agent.eq(id))
        .select((hire::nickname, scholar::name))
        .first::<(Option<String>, String)>(db)?;
    Ok(format!("{address} ({})", nickname.unwrap_or(name)))
}

/// How the scholar `id` is shown to chiefs and agents, as in
/// `scholar#2 (Mira)`.
pub(crate) fn scholar_name_of(
    db: &mut Db,
    id: ScholarId,
) -> Result<String> {
    let name = scholar::table
        .find(id)
        .select(scholar::name)
        .first::<String>(db)?;
    Ok(format!("{} ({name})", scholar_address(id)))
}
