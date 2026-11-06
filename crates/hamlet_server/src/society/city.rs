//! The city: every scholar of the realm, with its house. A scholar a
//! chief or the Hand makes moves in at once; one another agent makes
//! waits at the harbor until it is let in, as the realm's settings
//! say (H45).

use std::f64::consts::PI;

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_protocol::client::{Choice, HouseAt, Newcomers};
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, ChiefId, ScholarId};
use hamlet_store::schema::{agent, hamlet, hire, scholar, wait};
use serde_json::json;

use super::addresses::scholar_address;
use super::agents::EndHow;
use super::names::check_name;
use super::record::{self, Decision, DecisionKind};
use super::refusal::Refusal;
use super::{hand, hires};
use crate::change::{Change, Done, done, refuse};
use crate::jobs::{self, Fork, Job};
use crate::oversight::{self, Raise, Ruling, WaitKind};
use crate::text::text_enum;

/// The most characters a character may have (C11, C13).
pub(crate) const CHARACTER_MOST: usize = 4000;

/// The round at the city's middle that its landmarks keep.
pub(crate) const MIDDLE: f64 = HouseAt::MIDDLE;

/// The least distance between the middles of two houses.
pub(crate) const APART: f64 = HouseAt::APART;

text_enum! {
    pub(crate) enum ScholarKind {
        Scholar = "scholar",
        /// The Hand's own knowledge, never hired or listed.
        Hand = "hand",
    }
}

text_enum! {
    /// Where a scholar stands with the city.
    pub(crate) enum Arrival {
        /// It lives in its house in the city.
        Housed = "housed",
        /// It waits at the harbor to be let in.
        Waiting = "waiting",
        /// It was turned away, and waits at the harbor until a chief
        /// lets it in.
        Turned = "turned",
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = scholar)]
pub(crate) struct Scholar {
    pub id: ScholarId,
    pub kind: ScholarKind,
    pub name: String,
    pub chief: Option<ChiefId>,
    pub since: i64,
    pub made_by: Actor,
    pub planted: bool,
    pub arrival: Arrival,
    pub arrival_by: Option<Actor>,
    pub house_x: Option<f64>,
    pub house_z: Option<f64>,
    pub house_turn: Option<f64>,
    pub fork_of: Option<ScholarId>,
    pub fork_at: Option<String>,
    pub left_at: Option<i64>,
    pub left_by: Option<Actor>,
    pub left_reason: Option<String>,
    pub erased: bool,
}

impl Scholar {
    pub fn is_living(&self) -> bool {
        self.left_at.is_none()
    }

    /// The scholar's address with its name beside it.
    pub fn label(&self) -> String {
        format!("{} ({})", scholar_address(self.id), self.name)
    }

    /// The refusal of a call that names this scholar when it has left
    /// the city or is the Hand's.
    pub fn gone(&self) -> Option<Refusal> {
        (!self.is_living() || self.kind != ScholarKind::Scholar)
            .then(|| Refusal::ScholarGone { name: self.label() })
    }
}

/// Makes a scholar named `name` for `chief`, starting from
/// `character`, or from a commit of another scholar when it is a
/// fork.
pub(crate) fn make(
    cx: &mut Change<'_>,
    name: &str,
    chief: ChiefId,
    character: &str,
    fork: Option<Fork>,
) -> Done<ScholarId> {
    let name = match check_name(name) {
        Ok(name) => name,
        Err(refusal) => refuse!(refusal),
    };
    if character.chars().count() > CHARACTER_MOST {
        refuse!(Refusal::LongCharacter);
    }
    let id = diesel::insert_into(scholar::table)
        .values((
            scholar::kind.eq(ScholarKind::Scholar),
            scholar::name.eq(name),
            scholar::chief.eq(chief),
            scholar::since.eq(cx.at),
            scholar::made_by.eq(cx.actor),
            scholar::fork_of.eq(fork.as_ref().map(|f| f.from)),
            scholar::fork_at
                .eq(fork.as_ref().map(|f| f.commit.clone())),
        ))
        .returning(scholar::id)
        .get_result::<ScholarId>(cx.db)?;
    let kind = if fork.is_some() {
        DecisionKind::Forked
    } else {
        DecisionKind::ScholarMade
    };
    jobs::add(
        cx,
        &Job::Plant {
            scholar: id,
            character: character.to_owned(),
            fork,
        },
    )?;
    record::keep(cx, Decision::new(kind, "").scholar(id))?;
    match newcomer(cx)? {
        Some((maker, policy)) => {
            diesel::update(scholar::table.find(id))
                .set((
                    scholar::arrival.eq(Arrival::Waiting),
                    scholar::arrival_by.eq(cx.actor),
                ))
                .execute(cx.db)?;
            // Under the Hand, its errand is begun where the settings
            // and the models are at hand.
            if policy == Newcomers::Chief {
                oversight::raise(
                    cx,
                    Raise {
                        kind: WaitKind::Newcomer,
                        danger: None,
                        agent: maker,
                        scholar: Some(id),
                        conversation: None,
                        task: None,
                        about: format!(
                            "{} ({name})",
                            scholar_address(id)
                        ),
                        change: None,
                        answers: vec![
                            Choice::Welcome,
                            Choice::Dismiss,
                        ],
                        questions: None,
                    },
                )?;
            }
        }
        None => house(cx, id)?,
    }
    cx.touched.scholars.insert(id);
    Ok(Ok(id))
}

/// The agent making a scholar now and who lets it in, when it waits
/// at the harbor: when an agent other than the Hand makes it in a
/// realm that is not open.
fn newcomer(
    cx: &mut Change<'_>,
) -> Result<Option<(AgentId, Newcomers)>> {
    let Actor::Agent(maker) = cx.actor else {
        return Ok(None);
    };
    if maker == hand::id(cx.db)? {
        return Ok(None);
    }
    let policy = crate::settings::stored(cx.db)?.newcomers;
    Ok((policy != Newcomers::Open).then_some((maker, policy)))
}

/// Whether the Hand is to judge the newcomer `id`.
pub(crate) fn awaits_hand(
    db: &mut Db,
    id: ScholarId,
) -> Result<bool> {
    Ok(get(db, id)?.arrival == Arrival::Waiting
        && crate::settings::stored(db)?.newcomers == Newcomers::Hand)
}

/// Gives the scholar `id` the next free spot in the city, its door
/// toward the middle, and lets it in.
fn house(cx: &mut Change<'_>, id: ScholarId) -> Result<()> {
    let (x, z) = free_spot(cx.db)?;
    diesel::update(scholar::table.find(id))
        .set((
            scholar::arrival.eq(Arrival::Housed),
            scholar::arrival_by.eq(cx.actor),
            scholar::house_x.eq(x),
            scholar::house_z.eq(z),
            scholar::house_turn.eq(facing_middle(x, z)),
        ))
        .execute(cx.db)?;
    cx.touched.scholars.insert(id);
    Ok(())
}

/// The turn about the vertical that faces a door at `x, z` toward the
/// city's middle.
fn facing_middle(x: f64, z: f64) -> f64 {
    (-x).atan2(-z)
}

/// The spots of every house that stands in the city, a scholar's that
/// left included, but `but`'s own.
fn houses(
    db: &mut Db,
    but: Option<ScholarId>,
) -> Result<Vec<(f64, f64)>> {
    let kept = scholar::table
        .filter(scholar::erased.eq(false))
        .filter(scholar::arrival.eq(Arrival::Housed))
        .select((scholar::id, scholar::house_x, scholar::house_z))
        .load::<(ScholarId, Option<f64>, Option<f64>)>(db)?;
    Ok(kept
        .into_iter()
        .filter(|(id, ..)| Some(*id) != but)
        .filter_map(|(_, x, z)| Some((x?, z?)))
        .collect())
}

/// Whether a house at `x, z` keeps clear of the middle and of every
/// house in `taken`.
fn is_clear(taken: &[(f64, f64)], x: f64, z: f64) -> bool {
    HouseAt::is_clear(taken.iter().copied(), x, z)
}

/// The first spot on a golden-angle spiral out from the middle that
/// keeps clear of every house, as a sunflower lays its seeds.
pub(crate) fn free_spot(db: &mut Db) -> Result<(f64, f64)> {
    let taken = houses(db, None)?;
    let golden = PI * (3.0 - 5f64.sqrt());
    let first = MIDDLE + APART * 0.5;
    // Each spot holds about this much ground, so the spiral packs
    // close without crowding.
    let share = APART * APART * 1.1 / PI;
    for k in 0..100_000_u32 {
        let k = f64::from(k);
        let radius = (first * first + k * share).sqrt();
        let (x, z) = (
            radius * (k * golden).cos(),
            radius * (k * golden).sin(),
        );
        if is_clear(&taken, x, z) {
            return Ok((x, z));
        }
    }
    anyhow::bail!("the city has no free spot left")
}

/// Lets the newcomer `id` into the city at the next free spot.
pub(crate) fn welcome(
    cx: &mut Change<'_>,
    id: ScholarId,
    reason: &str,
) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if let Some(refusal) = scholar.gone() {
        refuse!(refusal);
    }
    if scholar.arrival == Arrival::Housed {
        refuse!(Refusal::Housed {
            name: scholar.label()
        });
    }
    house(cx, id)?;
    settle_newcomer(cx, id, Ruling::Welcome)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Welcomed, reason).scholar(id),
    )?;
    Ok(Ok(()))
}

/// Keeps the scholar `id` at the harbor: a newcomer turned away, or
/// one sent back out of its house.
pub(crate) fn turn_away(
    cx: &mut Change<'_>,
    id: ScholarId,
    reason: &str,
) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if let Some(refusal) = scholar.gone() {
        refuse!(refusal);
    }
    if scholar.arrival == Arrival::Turned {
        refuse!(Refusal::TurnedAway {
            name: scholar.label()
        });
    }
    diesel::update(scholar::table.find(id))
        .set((
            scholar::arrival.eq(Arrival::Turned),
            scholar::arrival_by.eq(cx.actor),
            scholar::house_x.eq(None::<f64>),
            scholar::house_z.eq(None::<f64>),
            scholar::house_turn.eq(None::<f64>),
        ))
        .execute(cx.db)?;
    settle_newcomer(cx, id, Ruling::Dismiss)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::TurnedAway, reason).scholar(id),
    )?;
    cx.touched.scholars.insert(id);
    Ok(Ok(()))
}

/// Ends the open petition about the newcomer `id`, as `ruling`.
fn settle_newcomer(
    cx: &mut Change<'_>,
    id: ScholarId,
    ruling: Ruling,
) -> Result<()> {
    let open = wait::table
        .filter(wait::open.eq(true))
        .filter(wait::kind.eq(WaitKind::Newcomer))
        .filter(wait::scholar.eq(id))
        .select(wait::id)
        .load(cx.db)?;
    let chief = match cx.actor {
        Actor::Chief(chief) => Some(chief),
        _ => None,
    };
    for wait in open {
        oversight::settle(cx, wait, ruling, "", chief)?;
    }
    Ok(())
}

/// Makes the Hand's own knowledge, once, with the realm.
pub(crate) fn make_hand(
    cx: &mut Change<'_>,
    character: &str,
) -> Result<ScholarId> {
    let id = diesel::insert_into(scholar::table)
        .values((
            scholar::kind.eq(ScholarKind::Hand),
            scholar::name.eq("hand"),
            scholar::since.eq(cx.at),
            scholar::made_by.eq(cx.actor),
        ))
        .returning(scholar::id)
        .get_result::<ScholarId>(cx.db)?;
    jobs::add(
        cx,
        &Job::Plant {
            scholar: id,
            character: character.to_owned(),
            fork: None,
        },
    )?;
    cx.touched.scholars.insert(id);
    Ok(id)
}

pub(crate) fn find(
    db: &mut Db,
    id: ScholarId,
) -> Result<Option<Scholar>> {
    Ok(scholar::table
        .find(id)
        .select(Scholar::as_select())
        .first(db)
        .optional()?)
}

pub(crate) fn get(db: &mut Db, id: ScholarId) -> Result<Scholar> {
    find(db, id)?.with_context(|| format!("no scholar {id}"))
}

/// The living scholar `id`, if it is one and not the Hand's.
pub(crate) fn living(
    db: &mut Db,
    id: ScholarId,
) -> Result<Option<Scholar>> {
    Ok(find(db, id)?.filter(|found| {
        found.is_living() && found.kind == ScholarKind::Scholar
    }))
}

/// Gives the living scholar `id` a new name (C10).
pub(crate) fn rename(
    cx: &mut Change<'_>,
    id: ScholarId,
    name: &str,
) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if let Some(refusal) = scholar.gone() {
        refuse!(refusal);
    }
    let name = match check_name(name) {
        Ok(name) => name,
        Err(refusal) => refuse!(refusal),
    };
    diesel::update(scholar::table.find(id))
        .set(scholar::name.eq(name))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::ScholarNamed, "")
            .scholar(id)
            .detail(json!({ "name": name })),
    )?;
    cx.touched.scholars.insert(id);
    for hire in hires::living_of(cx.db, id)? {
        cx.touched.agents.insert(hire.agent);
    }
    Ok(Ok(()))
}

/// Every scholar of the city, living and archived, but none erased.
pub(crate) fn all(db: &mut Db) -> Result<Vec<Scholar>> {
    Ok(scholar::table
        .filter(scholar::kind.eq(ScholarKind::Scholar))
        .filter(scholar::erased.eq(false))
        .order(scholar::id)
        .select(Scholar::as_select())
        .load(db)?)
}

/// Sets where the house of `id` stands in the city, and the way its
/// door faces: toward the middle unless `turn` says.
pub(crate) fn place(
    cx: &mut Change<'_>,
    id: ScholarId,
    x: f64,
    z: f64,
    turn: Option<f64>,
) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if let Some(refusal) = scholar.gone() {
        refuse!(refusal);
    }
    if scholar.arrival != Arrival::Housed {
        refuse!(Refusal::AtHarbor {
            name: scholar.label()
        });
    }
    if !(x.is_finite()
        && z.is_finite()
        && turn.is_none_or(f64::is_finite))
    {
        refuse!(Refusal::BadAddress {
            address: format!("{x}, {z}")
        });
    }
    let taken = houses(cx.db, Some(id))?;
    if !is_clear(&taken, x, z) {
        refuse!(Refusal::Crowded {
            name: scholar.label()
        });
    }
    diesel::update(scholar::table.find(id))
        .set((
            scholar::house_x.eq(x),
            scholar::house_z.eq(z),
            scholar::house_turn
                .eq(turn.unwrap_or_else(|| facing_middle(x, z))),
        ))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::HouseMoved, "").scholar(id),
    )?;
    cx.touched.scholars.insert(id);
    Ok(Ok(()))
}

/// Archives the scholar `id`, ending its living hires first. Refused
/// while one of its hires holds a hamlet's office.
pub(crate) fn archive(
    cx: &mut Change<'_>,
    id: ScholarId,
    reason: &str,
) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if let Some(refusal) = scholar.gone() {
        refuse!(refusal);
    }
    if reason.trim().is_empty() {
        refuse!(Refusal::NoReason);
    }
    for hire in hires::living_of(cx.db, id)? {
        done!(hires::end(cx, hire.agent, EndHow::Scholar, reason));
    }
    diesel::update(scholar::table.find(id))
        .set((
            scholar::left_at.eq(cx.at),
            scholar::left_by.eq(cx.actor),
            scholar::left_reason.eq(reason),
        ))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::ScholarArchived, reason)
            .scholar(id),
    )?;
    cx.touched.scholars.insert(id);
    Ok(Ok(()))
}

/// Brings back the archived scholar `id`, with the hires its
/// archiving ended in hamlets that still live.
pub(crate) fn restore(
    cx: &mut Change<'_>,
    id: ScholarId,
    reason: &str,
) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if scholar.erased {
        refuse!(Refusal::ScholarErased {
            name: scholar.label()
        });
    }
    if scholar.is_living() {
        refuse!(Refusal::ScholarLiving {
            name: scholar.label()
        });
    }
    diesel::update(scholar::table.find(id))
        .set((
            scholar::left_at.eq(None::<i64>),
            scholar::left_by.eq(None::<Actor>),
            scholar::left_reason.eq(None::<String>),
        ))
        .execute(cx.db)?;
    let ended_with = hire::table
        .inner_join(agent::table)
        .inner_join(hamlet::table)
        .filter(hire::scholar.eq(id))
        .filter(agent::ended_how.eq(EndHow::Scholar))
        .filter(agent::erased.eq(false))
        .filter(hamlet::removed_at.is_null())
        .order(hire::agent)
        .select(hire::agent)
        .load(cx.db)?;
    for hire in ended_with {
        done!(hires::come_back(cx, hire, reason));
    }
    record::keep(
        cx,
        Decision::new(DecisionKind::ScholarRestored, reason)
            .scholar(id),
    )?;
    cx.touched.scholars.insert(id);
    Ok(Ok(()))
}

/// Erases the archived scholar `id` for good, with every hire of it.
pub(crate) fn erase(cx: &mut Change<'_>, id: ScholarId) -> Done<()> {
    let scholar = get(cx.db, id)?;
    if scholar.erased {
        refuse!(Refusal::ScholarErased {
            name: scholar.label()
        });
    }
    if scholar.is_living() {
        refuse!(Refusal::ScholarLiving {
            name: scholar.label()
        });
    }
    let kept = hire::table
        .inner_join(agent::table)
        .filter(hire::scholar.eq(id))
        .filter(agent::erased.eq(false))
        .select(hire::agent)
        .load(cx.db)?;
    for hire in kept {
        done!(hires::erase(cx, hire));
    }
    diesel::update(scholar::table.find(id))
        .set(scholar::erased.eq(true))
        .execute(cx.db)?;
    jobs::add(cx, &Job::EraseScholar { scholar: id })?;
    record::keep(
        cx,
        Decision::new(DecisionKind::ScholarErased, "").scholar(id),
    )?;
    cx.touched.scholars.insert(id);
    Ok(Ok(()))
}

/// Marks the repository of `id` made.
pub(crate) fn planted(db: &mut Db, id: ScholarId) -> Result<()> {
    diesel::update(scholar::table.find(id))
        .set(scholar::planted.eq(true))
        .execute(db)?;
    Ok(())
}
