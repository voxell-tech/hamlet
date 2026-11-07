//! Hamlets: each with a mayor, a tree of hires, a folder it works on
//! and the chief's limits.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{
    Actor, AgentId, ChiefId, HamletId, ScholarId,
};
use hamlet_store::schema::{
    agent, hamlet, hamlet_chief, hamlet_folder, hire,
};
use serde_json::json;

use super::agents::{self, EndHow};
use super::city;
use super::hires::{self, Hired, Hiring, Spec};
use super::limits::Limits;
use super::names::check_hamlet_id;
use super::record::{self, Decision, DecisionKind};
use super::refusal::Refusal;
use crate::change::{Change, Done, done, refuse};
use crate::jobs::{self, Job};
use crate::text::text_enum;

/// The tiers a hamlet's cap of hires may take.
pub(crate) const TIERS: [i64; 4] = [50, 100, 200, 500];

text_enum! {
    /// How a hamlet's work on its folder is overseen (C21).
    pub(crate) enum Mode {
        Manual = "manual",
        Auto = "auto",
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = hamlet)]
pub(crate) struct Hamlet {
    pub id: HamletId,
    pub title: Option<String>,
    pub founded_at: i64,
    pub founded_by: Actor,
    pub mayor: AgentId,
    pub folder: Option<String>,
    pub mode: Mode,
    pub tier: i64,
    pub site_x: Option<f64>,
    pub site_z: Option<f64>,
    pub next_number: i64,
    pub limits: String,
    pub removed_at: Option<i64>,
    pub removed_by: Option<Actor>,
    pub removed_reason: Option<String>,
    pub erased: bool,
}

impl Hamlet {
    pub fn is_living(&self) -> bool {
        self.removed_at.is_none()
    }
}

/// A hamlet to found.
#[derive(Clone, Debug)]
pub(crate) struct Founding {
    pub id: HamletId,
    /// The chief it answers to.
    pub chief: ChiefId,
    pub mayor: Hiring,
    pub purpose: String,
}

/// Founds a hamlet with its mayor, who answers to the Hand (H6).
pub(crate) fn found(
    cx: &mut Change<'_>,
    founding: Founding,
) -> Done<Hired> {
    let id = founding.id;
    if let Err(refusal) = check_hamlet_id(id.as_str()) {
        refuse!(refusal);
    }
    if find(cx.db, &id)?.is_some() {
        refuse!(Refusal::HamletUsed { id: id.to_string() });
    }
    if founding.purpose.trim().is_empty() {
        refuse!(Refusal::NoPurpose);
    }
    // The mayor is set once its hire is made; the key is checked at
    // the commit.
    diesel::insert_into(hamlet::table)
        .values((
            hamlet::id.eq(&id),
            hamlet::founded_at.eq(cx.at),
            hamlet::founded_by.eq(cx.actor),
            hamlet::mayor.eq(AgentId(0)),
            hamlet::limits
                .eq(serde_json::to_string(&Limits::default())?),
        ))
        .execute(cx.db)?;
    diesel::insert_into(hamlet_chief::table)
        .values((
            hamlet_chief::hamlet.eq(&id),
            hamlet_chief::chief.eq(founding.chief),
        ))
        .execute(cx.db)?;
    let hired = done!(hires::hire(
        cx,
        Spec {
            hamlet: id.clone(),
            parent: None,
            hiring: founding.mayor,
            purpose: founding.purpose.clone(),
            reason: String::new(),
            owner: founding.chief,
        }
    ));
    diesel::update(hamlet::table.find(&id))
        .set(hamlet::mayor.eq(hired.agent))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Founded, &founding.purpose)
            .hamlet(&id)
            .subject(hired.agent),
    )?;
    cx.touched.hamlets.insert(id);
    Ok(Ok(hired))
}

pub(crate) fn find(
    db: &mut Db,
    id: &HamletId,
) -> Result<Option<Hamlet>> {
    Ok(hamlet::table
        .find(id)
        .select(Hamlet::as_select())
        .first(db)
        .optional()?)
}

pub(crate) fn get(db: &mut Db, id: &HamletId) -> Result<Hamlet> {
    find(db, id)?.with_context(|| format!("no hamlet {id}"))
}

/// The hamlet `id`, refused when it is missing, removed or erased.
pub(crate) fn living(db: &mut Db, id: &HamletId) -> Done<Hamlet> {
    let Some(found) = find(db, id)? else {
        refuse!(Refusal::NoHamlet { id: id.to_string() });
    };
    if found.erased {
        refuse!(Refusal::HamletErased { id: id.to_string() });
    }
    if !found.is_living() {
        refuse!(Refusal::HamletRemoved { id: id.to_string() });
    }
    Ok(Ok(found))
}

/// Every hamlet not erased, living and removed.
pub(crate) fn all(db: &mut Db) -> Result<Vec<Hamlet>> {
    Ok(hamlet::table
        .filter(hamlet::erased.eq(false))
        .order(hamlet::id)
        .select(Hamlet::as_select())
        .load(db)?)
}

/// The chiefs `id` answers to (C7).
pub(crate) fn chiefs(
    db: &mut Db,
    id: &HamletId,
) -> Result<Vec<ChiefId>> {
    Ok(hamlet_chief::table
        .filter(hamlet_chief::hamlet.eq(id))
        .order(hamlet_chief::chief)
        .select(hamlet_chief::chief)
        .load(db)?)
}

/// Gives `id` a name to be shown by, or takes it away (H32).
pub(crate) fn rename(
    cx: &mut Change<'_>,
    id: &HamletId,
    title: Option<&str>,
) -> Done<()> {
    done!(living(cx.db, id));
    let title = title.map(str::trim).filter(|t| !t.is_empty());
    if title.is_some_and(|t| t.chars().count() > 32) {
        refuse!(Refusal::LongTitle);
    }
    diesel::update(hamlet::table.find(id))
        .set(hamlet::title.eq(title))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::NamedHamlet, "")
            .hamlet(id)
            .detail(json!({ "title": title })),
    )?;
    Ok(Ok(()))
}

/// Sets where the hamlet `id` stands on the hamlet island (H32).
pub(crate) fn place(
    cx: &mut Change<'_>,
    id: &HamletId,
    x: f64,
    z: f64,
) -> Done<()> {
    done!(living(cx.db, id));
    if !(x.is_finite() && z.is_finite()) {
        refuse!(Refusal::BadAddress {
            address: format!("{x}, {z}")
        });
    }
    diesel::update(hamlet::table.find(id))
        .set((hamlet::site_x.eq(x), hamlet::site_z.eq(z)))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::PlacedHamlet, "").hamlet(id),
    )?;
    Ok(Ok(()))
}

/// Sets the tier of hires `id` may hold.
pub(crate) fn set_tier(
    cx: &mut Change<'_>,
    id: &HamletId,
    tier: i64,
) -> Done<()> {
    done!(living(cx.db, id));
    if !TIERS.contains(&tier) {
        refuse!(Refusal::BadTier);
    }
    let count = hires::count_living(cx.db, id)?;
    if count > tier {
        refuse!(Refusal::TierTooSmall {
            id: id.to_string(),
            count,
            tier,
        });
    }
    diesel::update(hamlet::table.find(id))
        .set(hamlet::tier.eq(tier))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Tier, "")
            .hamlet(id)
            .detail(json!({ "tier": tier })),
    )?;
    Ok(Ok(()))
}

/// Points `id` at `folder`, or at none (C20).
pub(crate) fn set_folder(
    cx: &mut Change<'_>,
    id: &HamletId,
    folder: Option<&str>,
) -> Done<()> {
    done!(living(cx.db, id));
    diesel::update(hamlet::table.find(id))
        .set(hamlet::folder.eq(folder))
        .execute(cx.db)?;
    if let Some(folder) = folder {
        diesel::insert_into(hamlet_folder::table)
            .values((
                hamlet_folder::hamlet.eq(id),
                hamlet_folder::folder.eq(folder),
                hamlet_folder::at.eq(cx.at),
            ))
            .on_conflict((
                hamlet_folder::hamlet,
                hamlet_folder::folder,
            ))
            .do_update()
            .set(hamlet_folder::at.eq(cx.at))
            .execute(cx.db)?;
    }
    record::keep(
        cx,
        Decision::new(DecisionKind::Folder, "")
            .hamlet(id)
            .detail(json!({ "folder": folder })),
    )?;
    Ok(Ok(()))
}

/// The folders `id` worked on, the latest first.
pub(crate) fn folders(
    db: &mut Db,
    id: &HamletId,
) -> Result<Vec<String>> {
    Ok(hamlet_folder::table
        .filter(hamlet_folder::hamlet.eq(id))
        .order(hamlet_folder::at.desc())
        .select(hamlet_folder::folder)
        .load(db)?)
}

/// Sets how the work of `id` on its folder is overseen (C21).
pub(crate) fn set_mode(
    cx: &mut Change<'_>,
    id: &HamletId,
    mode: Mode,
) -> Done<()> {
    done!(living(cx.db, id));
    diesel::update(hamlet::table.find(id))
        .set(hamlet::mode.eq(mode))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Mode, "")
            .hamlet(id)
            .detail(json!({ "mode": mode })),
    )?;
    Ok(Ok(()))
}

/// Replaces the limits of `id` (H22).
pub(crate) fn set_limits(
    cx: &mut Change<'_>,
    id: &HamletId,
    limits: &Limits,
) -> Done<()> {
    done!(living(cx.db, id));
    diesel::update(hamlet::table.find(id))
        .set(hamlet::limits.eq(serde_json::to_string(limits)?))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Limits, "").hamlet(id),
    )?;
    Ok(Ok(()))
}

/// Removes `id`: every hire of it ends, with the tree kept for when
/// it comes back (H19).
pub(crate) fn remove(
    cx: &mut Change<'_>,
    id: &HamletId,
    reason: &str,
) -> Done<()> {
    done!(living(cx.db, id));
    if reason.trim().is_empty() {
        refuse!(Refusal::NoReason);
    }
    for hire in hires::living_in(cx.db, id)? {
        done!(hires::end(cx, hire.agent, EndHow::Hamlet, reason));
    }
    diesel::update(hamlet::table.find(id))
        .set((
            hamlet::removed_at.eq(cx.at),
            hamlet::removed_by.eq(cx.actor),
            hamlet::removed_reason.eq(reason),
        ))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::RemovedHamlet, reason).hamlet(id),
    )?;
    Ok(Ok(()))
}

/// Brings back the removed hamlet `id` with the hires its removal
/// ended, refused while any of their scholars has left the city
/// (H20).
pub(crate) fn restore(
    cx: &mut Change<'_>,
    id: &HamletId,
    reason: &str,
) -> Done<()> {
    let Some(found) = find(cx.db, id)? else {
        refuse!(Refusal::NoHamlet { id: id.to_string() });
    };
    if found.erased {
        refuse!(Refusal::HamletErased { id: id.to_string() });
    }
    if found.is_living() {
        refuse!(Refusal::HamletLiving { id: id.to_string() });
    }
    let ended = hire::table
        .inner_join(agent::table)
        .filter(hire::hamlet.eq(id))
        .filter(agent::ended_how.eq(EndHow::Hamlet))
        .filter(agent::erased.eq(false))
        .select((hire::agent, hire::scholar))
        .load::<(AgentId, ScholarId)>(cx.db)?;
    for (_, scholar) in &ended {
        let scholar = city::get(cx.db, *scholar)?;
        if let Some(refusal) = scholar.gone() {
            refuse!(refusal);
        }
    }
    diesel::update(hamlet::table.find(id))
        .set((
            hamlet::removed_at.eq(None::<i64>),
            hamlet::removed_by.eq(None::<Actor>),
            hamlet::removed_reason.eq(None::<String>),
        ))
        .execute(cx.db)?;
    for (agent, _) in ended {
        agents::come_back(cx, agent)?;
        jobs::add(cx, &Job::Lodge { hire: agent })?;
    }
    record::keep(
        cx,
        Decision::new(DecisionKind::RestoredHamlet, reason)
            .hamlet(id),
    )?;
    Ok(Ok(()))
}

/// Erases the removed hamlet `id` for good, with its hires' lodges
/// and branches.
pub(crate) fn erase(cx: &mut Change<'_>, id: &HamletId) -> Done<()> {
    let Some(found) = find(cx.db, id)? else {
        refuse!(Refusal::NoHamlet { id: id.to_string() });
    };
    if found.erased {
        refuse!(Refusal::HamletErased { id: id.to_string() });
    }
    if found.is_living() {
        refuse!(Refusal::HamletLiving { id: id.to_string() });
    }
    let kept = hire::table
        .inner_join(agent::table)
        .filter(hire::hamlet.eq(id))
        .filter(agent::erased.eq(false))
        .select(hire::agent)
        .load::<AgentId>(cx.db)?;
    for hire in kept {
        done!(hires::erase(cx, hire));
    }
    diesel::update(hamlet::table.find(id))
        .set(hamlet::erased.eq(true))
        .execute(cx.db)?;
    jobs::add(cx, &Job::EraseHamlet { hamlet: id.clone() })?;
    record::keep(
        cx,
        Decision::new(DecisionKind::ErasedHamlet, "").hamlet(id),
    )?;
    Ok(Ok(()))
}
