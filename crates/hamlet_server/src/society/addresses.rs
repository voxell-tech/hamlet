//! Addresses: how chiefs and agents name the Hand, leads, agents,
//! workers and profiles, by ids and never by a profile's name (C30,
//! H14).

use std::fmt;

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, HamletId, ProfileId};
use hamlet_store::schema::{agent, hamlet_agent, profile, worker};

use super::agents::{self, AgentKind};
use super::names::hamlet_id;
use super::refusal::Refusal;
use super::{chiefs, hamlets, hand, tree};
use crate::realm::change::{Done, done, refuse};

/// A name for an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Address {
    Hand,
    /// A hamlet's lead, or the speaker's own.
    Lead {
        hamlet: Option<HamletId>,
    },
    Agent(AgentId),
    Worker(AgentId),
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hand => f.write_str("hand"),
            Self::Lead { hamlet: None } => f.write_str("lead"),
            Self::Lead {
                hamlet: Some(hamlet),
            } => write!(f, "lead@{hamlet}"),
            Self::Agent(id) => write!(f, "agent#{id}"),
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

/// Reads an address: `hand`, `lead`, `lead@<hamlet>`, `agent#<id>`
/// or `worker#<id>`.
pub(crate) fn parse(text: &str) -> Result<Address, Refusal> {
    let text = text.trim();
    let bad = || Refusal::BadAddress {
        address: text.into(),
    };
    if text == "hand" {
        return Ok(Address::Hand);
    }
    if text == "lead" {
        return Ok(Address::Lead { hamlet: None });
    }
    if let Some(hamlet) = text.strip_prefix("lead@") {
        return Ok(Address::Lead {
            hamlet: Some(hamlet_id(hamlet).ok_or_else(bad)?),
        });
    }
    let (kind, id) = text.split_once('#').ok_or_else(bad)?;
    let id = AgentId(id.parse::<i64>().map_err(|_| bad())?);
    match kind {
        "agent" => Ok(Address::Agent(id)),
        "worker" => Ok(Address::Worker(id)),
        _ => Err(bad()),
    }
}

/// Reads a profile's address, `profile#<id>`.
pub(crate) fn parse_profile(
    text: &str,
) -> Result<ProfileId, Refusal> {
    let text = text.trim();
    text.strip_prefix("profile#")
        .and_then(|id| id.parse::<i64>().ok())
        .map(ProfileId)
        .ok_or_else(|| Refusal::BadAddress {
            address: text.into(),
        })
}

/// The address of the profile `id`.
pub(crate) fn profile_address(id: ProfileId) -> String {
    format!("profile#{id}")
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
        Address::Lead { hamlet } => {
            let Some(hamlet) = hamlet.as_ref().or(here) else {
                refuse!(none());
            };
            done!(hamlets::living(db, hamlet));
            Ok(Ok(tree::lead(db, hamlet)?))
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
        Address::Agent(id) => {
            let mut query = hamlet_agent::table
                .inner_join(agent::table)
                .inner_join(profile::table)
                .filter(hamlet_agent::agent.eq(id))
                .filter(agent::erased.eq(false))
                .select(hamlet_agent::agent)
                .into_boxed();
            if reach == Reach::Living {
                query = query
                    .filter(agent::ended_at.is_null())
                    .filter(profile::left_at.is_null());
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

/// The address of `id`: `hand`, `agent#id` or `worker#id` (C30).
pub(crate) fn address_of(db: &mut Db, id: AgentId) -> Result<String> {
    let kind = agents::get(db, id)?.kind;
    Ok(match kind {
        AgentKind::Hand => Address::Hand,
        AgentKind::Hamlet => Address::Agent(id),
        AgentKind::Worker => Address::Worker(id),
    }
    .to_string())
}

/// How `id` is shown to chiefs and agents: its address, and beside it
/// the name of an agent, or else its profile's name, as in
/// `agent#3 (Mira)`, or a worker's name. An erased agent shows its
/// address only.
pub(crate) fn name_of(db: &mut Db, id: AgentId) -> Result<String> {
    let address = address_of(db, id)?;
    let found = agents::get(db, id)?;
    if found.kind == AgentKind::Worker
        && let Some(name) = worker::table
            .find(id)
            .select(worker::name)
            .first::<Option<String>>(db)?
    {
        return Ok(format!("{address} ({name})"));
    }
    if found.kind != AgentKind::Hamlet || found.erased {
        return Ok(address);
    }
    let (agent_name, profile_name) = hamlet_agent::table
        .inner_join(profile::table)
        .filter(hamlet_agent::agent.eq(id))
        .select((hamlet_agent::name, profile::name))
        .first::<(Option<String>, String)>(db)?;
    Ok(format!(
        "{address} ({})",
        agent_name.unwrap_or(profile_name)
    ))
}

/// How the profile `id` is shown to chiefs and agents, as in
/// `profile#2 (Mira)`.
pub(crate) fn profile_name_of(
    db: &mut Db,
    id: ProfileId,
) -> Result<String> {
    let name = profile::table
        .find(id)
        .select(profile::name)
        .first::<String>(db)?;
    Ok(format!("{} ({name})", profile_address(id)))
}
