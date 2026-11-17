//! Addresses: how chiefs and agents name the Hand, mayors, hires,
//! workers and scholars, by ids and never by a scholar's name (C30,
//! H14).

use anyhow::Result;
use diesel::prelude::*;
use hamlet_protocol::address::{Address, bare};
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, HamletId, ScholarId};
use hamlet_store::schema::{agent, hire, scholar, worker};

use super::agents::{self, AgentKind};
use super::names::hamlet_id;
use super::refusal::Refusal;
use super::{chiefs, city, hamlets, hand, tree};
use crate::clients::state::{wire_agent, wire_scholar};
use crate::realm::change::{Done, done, refuse};

/// Which agents or scholars an address may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    Living,
    /// The ended agents and the archived scholars too, but not the
    /// erased.
    Kept,
}

/// Reads an agent's address, with or without the name shown beside
/// it.
pub(crate) fn parse(text: &str) -> Result<Address, Refusal> {
    let text = bare(text);
    match text.parse::<Address>() {
        Ok(Address::Mayor {
            hamlet: Some(hamlet),
        }) if hamlet_id(&hamlet).is_none() => {}
        Ok(Address::Scholar(_)) | Err(_) => {}
        Ok(address) => return Ok(address),
    }
    Err(Refusal::BadAddress {
        address: text.into(),
    })
}

/// Reads a scholar's address, `scholar#<id>`, with or without the
/// name shown beside it.
pub(crate) fn parse_scholar(
    text: &str,
) -> Result<ScholarId, Refusal> {
    let text = bare(text);
    match text.parse::<Address>() {
        Ok(Address::Scholar(id)) => i64::try_from(id.0).ok(),
        _ => None,
    }
    .map(ScholarId)
    .ok_or_else(|| Refusal::BadAddress {
        address: text.into(),
    })
}

/// The scholar `text` addresses, among those `reach` allows.
pub(crate) fn scholar_at(
    db: &mut Db,
    text: &str,
    reach: Reach,
) -> Done<ScholarId> {
    let id = match parse_scholar(text) {
        Ok(id) => id,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let found = city::find(db, id)?.filter(|found| {
        found.kind == city::ScholarKind::Scholar
            && (reach == Reach::Kept || found.is_living())
    });
    Ok(match found {
        Some(found) => Ok(found.id),
        None => Err(Refusal::NoScholar {
            address: text.into(),
        }),
    })
}

/// The living scholar `text` addresses, `scholar#<id>` with
/// `@<version>` to pin one, and that version.
pub(crate) fn scholar_pinned(
    db: &mut Db,
    text: &str,
) -> Done<(ScholarId, Option<String>)> {
    let (address, version) = match text.split_once('@') {
        Some((address, version)) => {
            (address, Some(version.to_owned()))
        }
        None => (text, None),
    };
    Ok(scholar_at(db, address, Reach::Living)?
        .map(|scholar| (scholar, version)))
}

/// The address of the scholar `id`.
pub(crate) fn scholar_address(id: ScholarId) -> String {
    Address::Scholar(wire_scholar(id)).to_string()
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
    let stored = |id: hamlet_protocol::client::AgentId| {
        i64::try_from(id.0).ok().map(AgentId)
    };
    match address {
        Address::Hand => Ok(Ok(hand::id(db)?)),
        Address::Mayor { hamlet } => {
            let named = hamlet.as_deref().map(hamlet_id);
            let hamlet = match named {
                Some(Some(hamlet)) => hamlet,
                Some(None) => refuse!(none()),
                None => match here {
                    Some(here) => here.clone(),
                    None => refuse!(none()),
                },
            };
            done!(hamlets::living(db, &hamlet));
            Ok(Ok(tree::mayor(db, &hamlet)?))
        }
        Address::Scholar(_) => refuse!(none()),
        Address::Worker(id) => {
            let Some(id) = stored(*id) else {
                refuse!(none());
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
                _ => refuse!(none()),
            }
        }
        Address::Hire(id) => {
            let Some(id) = stored(*id) else {
                refuse!(none());
            };
            let mut query = hire::table
                .inner_join(agent::table)
                .inner_join(scholar::table)
                .filter(hire::agent.eq(id))
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

/// The address of `id`: `hand`, `hire#id` or `worker#id` (C30).
pub(crate) fn address_of(db: &mut Db, id: AgentId) -> Result<String> {
    let kind = agents::get(db, id)?.kind;
    Ok(match kind {
        AgentKind::Hand => Address::Hand,
        AgentKind::Hire => Address::Hire(wire_agent(id)),
        AgentKind::Worker => Address::Worker(wire_agent(id)),
    }
    .to_string())
}

/// How `id` is shown to chiefs and agents: its address, and beside it
/// the nickname of a hire, or else its scholar's name, as in
/// `hire#3 (Mira)`, or a worker's nickname. An erased hire shows its
/// address only.
pub(crate) fn name_of(db: &mut Db, id: AgentId) -> Result<String> {
    let address = address_of(db, id)?;
    let found = agents::get(db, id)?;
    if found.kind == AgentKind::Worker
        && let Some(nickname) = worker::table
            .find(id)
            .select(worker::nickname)
            .first::<Option<String>>(db)?
    {
        return Ok(format!("{address} ({nickname})"));
    }
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
