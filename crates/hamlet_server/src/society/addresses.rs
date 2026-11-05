//! Addresses: how chiefs and agents name the Hand, mayors, hires and
//! workers, and how an agent is named back (C30, H14).

use std::fmt;

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, HamletId};
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
    /// A scholar's hire: its one hire where the address leaves it
    /// open, or the hire of that number.
    Hire {
        name: String,
        number: Option<i64>,
        hamlet: Option<HamletId>,
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
            Self::Hire {
                name,
                number,
                hamlet,
            } => {
                f.write_str(name)?;
                if let Some(number) = number {
                    write!(f, "#{number}")?;
                }
                if let Some(hamlet) = hamlet {
                    write!(f, "@{hamlet}")?;
                }
                Ok(())
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
/// `<name>[#<number>][@<hamlet>]` or `worker#<id>`.
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
    let (left, hamlet) = match text.split_once('@') {
        Some((left, hamlet)) => {
            check_hamlet_id(hamlet).map_err(|_| bad())?;
            (left, Some(HamletId(hamlet.into())))
        }
        None => (text, None),
    };
    if left == "mayor" {
        return Ok(Address::Mayor { hamlet });
    }
    let (name, number) = match left.split_once('#') {
        Some((name, number)) => {
            (name, Some(number.parse::<i64>().map_err(|_| bad())?))
        }
        None => (left, None),
    };
    let shaped = !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
    if !shaped {
        return Err(bad());
    }
    Ok(Address::Hire {
        name: name.into(),
        number,
        hamlet,
    })
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
        Address::Hire {
            name,
            number,
            hamlet,
        } => {
            let mut query = hire::table
                .inner_join(agent::table)
                .inner_join(scholar::table)
                .filter(scholar::name.eq(name))
                .filter(agent::erased.eq(false))
                .select((hire::agent, hire::hamlet))
                .into_boxed();
            if reach == Reach::Living {
                query = query
                    .filter(agent::ended_at.is_null())
                    .filter(scholar::left_at.is_null());
            }
            if let Some(number) = number {
                query = query.filter(hire::number.eq(number));
            }
            if let Some(hamlet) = hamlet {
                query = query.filter(hire::hamlet.eq(hamlet));
            }
            let mut found = query.load::<(AgentId, HamletId)>(db)?;
            // A name alone means the hire in the speaker's hamlet
            // when it has one there.
            if hamlet.is_none()
                && let Some(here) = here
                && found.iter().any(|(_, h)| h == here)
            {
                found.retain(|(_, h)| h == here);
            }
            match found[..] {
                [] => refuse!(none()),
                [(one, _)] => Ok(Ok(one)),
                _ => {
                    let mut names = Vec::new();
                    for (id, _) in &found {
                        names.push(format!(
                            "`{}`",
                            full_name(db, *id)?
                        ));
                    }
                    refuse!(Refusal::Ambiguous {
                        address: address.to_string(),
                        hires: names.join(", "),
                    })
                }
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

/// How `id` is named to chiefs and agents: `hand`, `name@hamlet`,
/// `name#number@hamlet` where its scholar has several hires in that
/// hamlet, `worker#id`, and `hamlet#number` once erased (C30).
pub(crate) fn name_of(db: &mut Db, id: AgentId) -> Result<String> {
    let found = agents::get(db, id)?;
    match found.kind {
        AgentKind::Hand => Ok("hand".into()),
        AgentKind::Worker => Ok(format!("worker#{id}")),
        AgentKind::Hire => {
            let (hamlet, number, scholar, name) = hire::table
                .inner_join(scholar::table)
                .filter(hire::agent.eq(id))
                .select((
                    hire::hamlet,
                    hire::number,
                    hire::scholar,
                    scholar::name,
                ))
                .first::<(
                    HamletId,
                    i64,
                    hamlet_store::ids::ScholarId,
                    String,
                )>(db)?;
            if found.erased {
                return Ok(format!("{hamlet}#{number}"));
            }
            let siblings = hire::table
                .inner_join(agent::table)
                .filter(hire::hamlet.eq(&hamlet))
                .filter(hire::scholar.eq(scholar))
                .filter(agent::erased.eq(false))
                .count()
                .get_result::<i64>(db)?;
            if siblings > 1 {
                Ok(format!("{name}#{number}@{hamlet}"))
            } else {
                Ok(format!("{name}@{hamlet}"))
            }
        }
    }
}

/// The name of hire `id` with its number, which names it whatever
/// else is hired.
fn full_name(db: &mut Db, id: AgentId) -> Result<String> {
    let (hamlet, number, name) = hire::table
        .inner_join(scholar::table)
        .filter(hire::agent.eq(id))
        .select((hire::hamlet, hire::number, scholar::name))
        .first::<(HamletId, i64, String)>(db)?;
    Ok(format!("{name}#{number}@{hamlet}"))
}
