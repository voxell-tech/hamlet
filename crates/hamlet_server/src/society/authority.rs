//! Who may do what (C7, H23): every check of standing, in one place.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{
    Actor, AgentId, ChiefId, HamletId, ScholarId,
};
use hamlet_store::schema::hamlet_chief;

use super::agents::{self, AgentKind};
use super::{chiefs, city, hamlets, hires, tree};

/// Whether `chief` is a chief of `hamlet` (C7).
pub(crate) fn is_chief_of(
    db: &mut Db,
    chief: ChiefId,
    hamlet: &HamletId,
) -> Result<bool> {
    Ok(hamlet_chief::table
        .find((hamlet, chief))
        .count()
        .get_result::<i64>(db)?
        > 0)
}

/// Whether `actor` is the Hand.
pub(crate) fn is_hand(db: &mut Db, actor: Actor) -> Result<bool> {
    Ok(match actor {
        Actor::Agent(id) => {
            agents::get(db, id)?.kind == AgentKind::Hand
        }
        _ => false,
    })
}

/// Whether `actor` may lead the hire `id`: a chief of its hamlet, the
/// Hand, or a hire above it in its tree.
pub(crate) fn may_lead(
    db: &mut Db,
    actor: Actor,
    id: AgentId,
) -> Result<bool> {
    let hire = hires::get(db, id)?;
    Ok(match actor {
        Actor::Chief(chief) => is_chief_of(db, chief, &hire.hamlet)?,
        Actor::Agent(agent) => {
            is_hand(db, actor)? || tree::is_above(db, agent, id)?
        }
        Actor::Realm => true,
    })
}

/// Whether `actor` may keep the scholar `id`: change it, tag, revert,
/// fork or purge it, archive, restore or erase it. Its chief may, and
/// the Hand, whose acts wait for a chief.
pub(crate) fn may_keep(
    db: &mut Db,
    actor: Actor,
    id: ScholarId,
) -> Result<bool> {
    let scholar = city::get(db, id)?;
    Ok(match actor {
        Actor::Chief(chief) => scholar.chief == Some(chief),
        Actor::Agent(_) => is_hand(db, actor)?,
        Actor::Realm => true,
    })
}

/// Whether `actor` may hire the scholar `id` into `hamlet`: a chief
/// its own scholar into its own hamlet, the Hand any, and a lead any
/// scholar of its hamlet's chiefs.
pub(crate) fn may_hire(
    db: &mut Db,
    actor: Actor,
    id: ScholarId,
    hamlet: &HamletId,
) -> Result<bool> {
    let owner = city::get(db, id)?.chief;
    Ok(match actor {
        Actor::Chief(chief) => {
            owner == Some(chief) && is_chief_of(db, chief, hamlet)?
        }
        Actor::Agent(_) if is_hand(db, actor)? => true,
        Actor::Agent(_) => match owner {
            Some(owner) => {
                hamlets::chiefs(db, hamlet)?.contains(&owner)
            }
            None => false,
        },
        Actor::Realm => true,
    })
}

/// The chief a scholar belongs to when `actor` makes it: a chief owns
/// what it makes; an agent's belongs to the chief whose task it works
/// on, else to the chief who founded its hamlet, else to the realm's
/// first chief.
pub(crate) fn owner_for(
    db: &mut Db,
    actor: Actor,
    hamlet: Option<&HamletId>,
    task_chief: Option<ChiefId>,
) -> Result<ChiefId> {
    if let Actor::Chief(chief) = actor {
        return Ok(chief);
    }
    if let Some(chief) = task_chief {
        return Ok(chief);
    }
    if let Some(hamlet) = hamlet
        && let Some(first) = hamlets::chiefs(db, hamlet)?.first()
    {
        return Ok(*first);
    }
    chiefs::first(db)?.context("the realm has no chief")
}
