//! The Hand: the realm's own agent, above every mayor.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ScholarId};
use hamlet_store::schema::hand;

use super::agents::{self, AgentKind};
use super::city;
use crate::change::Change;

/// Makes the Hand, once, with the realm, starting from `character`.
pub(crate) fn make(
    cx: &mut Change<'_>,
    character: &str,
) -> Result<AgentId> {
    let scholar = city::make_hand(cx, character)?;
    let agent = agents::make(cx, AgentKind::Hand)?;
    diesel::insert_into(hand::table)
        .values((hand::agent.eq(agent), hand::scholar.eq(scholar)))
        .execute(cx.db)?;
    Ok(agent)
}

/// The Hand's id.
pub(crate) fn id(db: &mut Db) -> Result<AgentId> {
    Ok(hand::table.select(hand::agent).first(db)?)
}

/// The Hand's own knowledge.
pub(crate) fn scholar(db: &mut Db) -> Result<ScholarId> {
    Ok(hand::table.select(hand::scholar).first(db)?)
}
