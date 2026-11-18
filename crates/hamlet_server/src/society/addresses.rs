//! Addresses of agents and profiles, by ids (C30, H14).

use anyhow::Result;
use diesel::prelude::*;
use hamlet_protocol::address::{Address, bare};
use hamlet_protocol::client as protocol;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, HamletId, ProfileId};
use hamlet_store::schema::{agent, hamlet_agent, profile, worker};

use super::agents::{self, AgentKind};
use super::names::hamlet_id;
use super::rejection::Rejection;
use super::{chiefs, city, hamlets, hand, tree};
use crate::realm::change::{Done, checked, done, reject};

/// Which agents or profiles an address may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    Living,
    /// The ended agents and the archived profiles too.
    Kept,
}

/// Reads an agent's address, with or without the name shown beside
/// it.
pub(crate) fn parse(text: &str) -> Result<Address, Rejection> {
    let text = bare(text);
    match text.parse::<Address>() {
        Ok(Address::Lead {
            hamlet: Some(hamlet),
        }) if hamlet_id(&hamlet).is_none() => {}
        Ok(Address::Profile(_)) | Err(_) => {}
        Ok(address) => return Ok(address),
    }
    Err(Rejection::BadAddress {
        address: text.into(),
    })
}

/// Reads a profile's address, `profile#<id>`, with or without the
/// name shown beside it.
pub(crate) fn parse_profile(
    text: &str,
) -> Result<ProfileId, Rejection> {
    let text = bare(text);
    match text.parse::<Address>() {
        Ok(Address::Profile(id)) => i64::try_from(id.0).ok(),
        _ => None,
    }
    .map(ProfileId)
    .ok_or_else(|| Rejection::BadAddress {
        address: text.into(),
    })
}

/// The profile `text` addresses, among those `reach` allows.
pub(crate) fn profile_at(
    db: &mut Db,
    text: &str,
    reach: Reach,
) -> Done<ProfileId> {
    let id = checked!(parse_profile(text));
    let found = city::find(db, id)?.filter(|found| {
        found.kind == city::ProfileKind::Profile
            && (reach == Reach::Kept || found.is_living())
    });
    Ok(match found {
        Some(found) => Ok(found.id),
        None => Err(Rejection::NoProfile {
            address: text.into(),
        }),
    })
}

/// The living profile `text` addresses, and the `@<version>` it pins.
pub(crate) fn profile_pinned(
    db: &mut Db,
    text: &str,
) -> Done<(ProfileId, Option<String>)> {
    let (address, version) = match text.split_once('@') {
        Some((address, version)) => {
            (address, Some(version.to_owned()))
        }
        None => (text, None),
    };
    Ok(profile_at(db, address, Reach::Living)?
        .map(|profile| (profile, version)))
}

/// The protocols' form of the agent `id`.
pub(crate) fn wire_agent(id: AgentId) -> protocol::AgentId {
    protocol::AgentId(u64::try_from(id.0).unwrap_or_default())
}

/// The protocols' form of the profile `id`.
pub(crate) fn wire_profile(id: ProfileId) -> protocol::ProfileId {
    protocol::ProfileId(u64::try_from(id.0).unwrap_or_default())
}

/// The address of the profile `id`.
pub(crate) fn profile_address(id: ProfileId) -> String {
    Address::Profile(wire_profile(id)).to_string()
}

/// The agent `address` names, said in `here`, the speaker's hamlet.
pub(crate) fn resolve(
    db: &mut Db,
    address: &Address,
    here: Option<&HamletId>,
    reach: Reach,
) -> Done<AgentId> {
    let none = || Rejection::NoOne {
        address: address.to_string(),
    };
    let stored =
        |id: protocol::AgentId| i64::try_from(id.0).ok().map(AgentId);
    match address {
        Address::Hand => Ok(Ok(hand::id(db)?)),
        Address::Lead { hamlet } => {
            let named = hamlet.as_deref().map(hamlet_id);
            let hamlet = match named {
                Some(Some(hamlet)) => hamlet,
                Some(None) => reject!(none()),
                None => match here {
                    Some(here) => here.clone(),
                    None => reject!(none()),
                },
            };
            done!(hamlets::living(db, &hamlet));
            Ok(Ok(tree::lead(db, &hamlet)?))
        }
        Address::Profile(_) => reject!(none()),
        Address::Worker(id) => {
            let Some(id) = stored(*id) else {
                reject!(none());
            };
            let found = agents::find(db, id)?;
            match found {
                Some(found)
                    if found.kind == AgentKind::Worker
                        && (reach == Reach::Kept
                            || found.is_living()) =>
                {
                    Ok(Ok(id))
                }
                _ => reject!(none()),
            }
        }
        Address::Agent(id) => {
            let Some(id) = stored(*id) else {
                reject!(none());
            };
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
                None => reject!(none()),
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
        AgentKind::Hamlet => Address::Agent(wire_agent(id)),
        AgentKind::Worker => Address::Worker(wire_agent(id)),
    }
    .to_string())
}

/// How `id` is shown to chiefs and agents, as in `agent#3 (Mira)`.
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
