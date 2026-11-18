//! The Hand: the realm's own agent, above every lead.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ProfileId};
use hamlet_store::schema::hand;

use super::agents::{self, AgentKind};
use super::city;
use crate::realm::change::Change;
use crate::realm::jobs::{self, Job};

/// Makes the Hand, once, with the realm, starting from `character`.
pub(crate) fn make(
    cx: &mut Change<'_>,
    character: &str,
) -> Result<AgentId> {
    let profile = city::make_hand(cx, character)?;
    let agent = agents::make(cx, AgentKind::Hand)?;
    diesel::insert_into(hand::table)
        .values((hand::agent.eq(agent), hand::profile.eq(profile)))
        .execute(cx.db)?;
    jobs::add(cx, &Job::Lodge { hire: agent })?;
    Ok(agent)
}

/// The Hand's id.
pub(crate) fn id(db: &mut Db) -> Result<AgentId> {
    Ok(hand::table.select(hand::agent).first(db)?)
}

/// The model chosen for the Hand, if any.
pub(crate) fn model(db: &mut Db) -> Result<Option<String>> {
    Ok(hand::table.select(hand::model).first(db)?)
}

/// The Hand's own knowledge.
pub(crate) fn profile(db: &mut Db) -> Result<ProfileId> {
    Ok(hand::table.select(hand::profile).first(db)?)
}
