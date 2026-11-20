//! Hamlets: each with a lead, a tree of agents, a folder it works on
//! and the chief's limits.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{
    Actor, AgentId, ChiefId, HamletId, ProfileId,
};
use hamlet_store::schema::{
    agent, hamlet, hamlet_agent, hamlet_chief, hamlet_folder,
};
use serde_json::json;

use super::agents::{self, EndHow};
use super::city;
use super::hamlet_agents::{
    self, LastDream, NewAgent, ProfileChoice, Spec,
};
use super::limits::Limits;
use super::names::{check_name, new_hamlet_id};
use super::record::{self, Decision, DecisionKind};
use super::rejection::Rejection;
use crate::realm::change::{Change, Done, checked, done, reject};
use crate::realm::jobs::{self, Job};

/// The tiers a hamlet's cap of agents may take.
pub(crate) const TIERS: [i64; 4] = [50, 100, 200, 500];

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = hamlet)]
pub(crate) struct Hamlet {
    pub id: HamletId,
    pub name: String,
    pub founded_at: i64,
    pub founded_by: Actor,
    pub lead: AgentId,
    pub tier: i64,
    pub site_x: Option<f64>,
    pub site_z: Option<f64>,
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
    /// The name it is shown by; its id is made for it.
    pub name: String,
    /// The chief it answers to.
    pub chief: ChiefId,
    pub lead: ProfileChoice,
    pub purpose: String,
}

/// Founds a hamlet with its lead, who answers to the Hand (H6).
pub(crate) fn found(
    cx: &mut Change<'_>,
    founding: Founding,
) -> Done<NewAgent> {
    let name = checked!(check_name(&founding.name)).to_owned();
    if founding.purpose.trim().is_empty() {
        reject!(Rejection::NoPurpose);
    }
    let id = new_hamlet_id();
    // The lead is set once its agent is made; the key is checked at
    // the commit.
    diesel::insert_into(hamlet::table)
        .values((
            hamlet::id.eq(&id),
            hamlet::name.eq(&name),
            hamlet::founded_at.eq(cx.at),
            hamlet::founded_by.eq(cx.actor),
            hamlet::lead.eq(AgentId(0)),
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
    let lead = done!(hamlet_agents::add(
        cx,
        Spec {
            hamlet: id.clone(),
            parent: None,
            profile: founding.lead,
            purpose: founding.purpose.clone(),
            reason: String::new(),
            owner: founding.chief,
        }
    ));
    diesel::update(hamlet::table.find(&id))
        .set(hamlet::lead.eq(lead.agent))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Founded, &founding.purpose)
            .hamlet(&id)
            .subject(lead.agent),
    )?;
    cx.touched.hamlets.insert(id);
    Ok(Ok(lead))
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

/// The hamlet `id`, rejected when it is missing, removed or erased.
pub(crate) fn living(db: &mut Db, id: &HamletId) -> Done<Hamlet> {
    let Some(found) = find(db, id)? else {
        reject!(Rejection::NoHamlet { id: id.to_string() });
    };
    if found.erased {
        reject!(Rejection::HamletErased { id: id.to_string() });
    }
    if !found.is_living() {
        reject!(Rejection::HamletRemoved { id: id.to_string() });
    }
    Ok(Ok(found))
}

/// Every hamlet not erased, living and removed.
pub(crate) fn all(db: &mut Db) -> Result<Vec<Hamlet>> {
    Ok(hamlet::table
        .filter(hamlet::erased.eq(false))
        .order((hamlet::name, hamlet::id))
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

/// Gives `id` another name to be shown by (H32).
pub(crate) fn rename(
    cx: &mut Change<'_>,
    id: &HamletId,
    name: &str,
) -> Done<()> {
    done!(living(cx.db, id));
    let name = checked!(check_name(name));
    diesel::update(hamlet::table.find(id))
        .set(hamlet::name.eq(name))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::NamedHamlet, "")
            .hamlet(id)
            .detail(json!({ "name": name })),
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
        reject!(Rejection::BadAddress {
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

/// Sets the tier of agents `id` may hold.
pub(crate) fn set_tier(
    cx: &mut Change<'_>,
    id: &HamletId,
    tier: i64,
) -> Done<()> {
    done!(living(cx.db, id));
    if !TIERS.contains(&tier) {
        reject!(Rejection::BadTier);
    }
    let count = hamlet_agents::count_living(cx.db, id)?;
    if count > tier {
        reject!(Rejection::TierTooSmall {
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

/// Makes `folder` one `id` works on, after the ones it works on
/// already (C20). The first it worked on is where commands run.
pub(crate) fn add_folder(
    cx: &mut Change<'_>,
    id: &HamletId,
    folder: &str,
    reason: &str,
) -> Done<()> {
    done!(living(cx.db, id));
    let last = hamlet_folder::table
        .filter(hamlet_folder::hamlet.eq(id))
        .select(diesel::dsl::max(hamlet_folder::since))
        .first::<Option<i64>>(cx.db)?;
    let held = hamlet_folder::table
        .find((id, folder))
        .select(hamlet_folder::since)
        .first::<Option<i64>>(cx.db)
        .optional()?
        .flatten();
    // A folder it works on keeps its place.
    let since = held.unwrap_or_else(|| {
        last.map_or(cx.at, |last| (last + 1).max(cx.at))
    });
    diesel::insert_into(hamlet_folder::table)
        .values((
            hamlet_folder::hamlet.eq(id),
            hamlet_folder::folder.eq(folder),
            hamlet_folder::at.eq(cx.at),
            hamlet_folder::since.eq(since),
        ))
        .on_conflict((hamlet_folder::hamlet, hamlet_folder::folder))
        .do_update()
        .set((
            hamlet_folder::at.eq(cx.at),
            hamlet_folder::since.eq(since),
        ))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Folder, reason)
            .hamlet(id)
            .detail(json!({ "added": folder })),
    )?;
    Ok(Ok(()))
}

/// Stops `id` working on `folder`, which stays among the ones it
/// worked on before.
pub(crate) fn drop_folder(
    cx: &mut Change<'_>,
    id: &HamletId,
    folder: &str,
) -> Done<()> {
    done!(living(cx.db, id));
    let dropped = diesel::update(
        hamlet_folder::table
            .find((id, folder))
            .filter(hamlet_folder::since.is_not_null()),
    )
    .set(hamlet_folder::since.eq(None::<i64>))
    .execute(cx.db)?;
    if dropped == 0 {
        reject!(Rejection::NotWorkedOn {
            path: folder.into()
        });
    }
    record::keep(
        cx,
        Decision::new(DecisionKind::Folder, "")
            .hamlet(id)
            .detail(json!({ "dropped": folder })),
    )?;
    Ok(Ok(()))
}

/// Makes `folder` the only folder `id` works on, or none (C20).
pub(crate) fn set_folder(
    cx: &mut Change<'_>,
    id: &HamletId,
    folder: Option<&str>,
) -> Done<()> {
    done!(living(cx.db, id));
    diesel::update(
        hamlet_folder::table.filter(hamlet_folder::hamlet.eq(id)),
    )
    .set(hamlet_folder::since.eq(None::<i64>))
    .execute(cx.db)?;
    match folder {
        Some(folder) => add_folder(cx, id, folder, ""),
        None => {
            record::keep(
                cx,
                Decision::new(DecisionKind::Folder, "")
                    .hamlet(id)
                    .detail(json!({ "cleared": true })),
            )?;
            Ok(Ok(()))
        }
    }
}

/// Rejects `folder` unless it is a folder on this machine.
pub(crate) fn check_folder(folder: &str) -> Result<(), Rejection> {
    if std::path::Path::new(folder).is_dir() {
        Ok(())
    } else {
        Err(Rejection::NoFile {
            path: folder.into(),
        })
    }
}

/// The folders `id` works on, the one commands run in first (C20).
pub(crate) fn working(
    db: &mut Db,
    id: &HamletId,
) -> Result<Vec<String>> {
    Ok(hamlet_folder::table
        .filter(hamlet_folder::hamlet.eq(id))
        .filter(hamlet_folder::since.is_not_null())
        .order((hamlet_folder::since.asc(), hamlet_folder::folder))
        .select(hamlet_folder::folder)
        .load(db)?)
}

/// The folders `id` worked on and does not now, the latest first.
pub(crate) fn folders(
    db: &mut Db,
    id: &HamletId,
) -> Result<Vec<String>> {
    Ok(hamlet_folder::table
        .filter(hamlet_folder::hamlet.eq(id))
        .filter(hamlet_folder::since.is_null())
        .order(hamlet_folder::at.desc())
        .select(hamlet_folder::folder)
        .load(db)?)
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

/// Removes `id`: every agent of it ends, with the tree kept for when
/// it comes back (H19).
pub(crate) fn remove(
    cx: &mut Change<'_>,
    id: &HamletId,
    reason: &str,
) -> Done<()> {
    done!(living(cx.db, id));
    for agent in hamlet_agents::living_in(cx.db, id)? {
        done!(hamlet_agents::end(
            cx,
            agent.agent,
            EndHow::Hamlet,
            LastDream::Without,
            reason,
        ));
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

/// Brings back the removed hamlet `id` with the agents its removal
/// ended (H20).
pub(crate) fn restore(
    cx: &mut Change<'_>,
    id: &HamletId,
    reason: &str,
) -> Done<()> {
    let Some(found) = find(cx.db, id)? else {
        reject!(Rejection::NoHamlet { id: id.to_string() });
    };
    if found.erased {
        reject!(Rejection::HamletErased { id: id.to_string() });
    }
    if found.is_living() {
        reject!(Rejection::HamletLiving { id: id.to_string() });
    }
    let ended = hamlet_agent::table
        .inner_join(agent::table)
        .filter(hamlet_agent::hamlet.eq(id))
        .filter(agent::ended_how.eq(EndHow::Hamlet))
        .filter(agent::erased.eq(false))
        .select((hamlet_agent::agent, hamlet_agent::profile))
        .load::<(AgentId, ProfileId)>(cx.db)?;
    for (_, profile) in &ended {
        let profile = city::get(cx.db, *profile)?;
        if let Some(rejection) = profile.gone() {
            reject!(rejection);
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
        jobs::add(cx, &Job::Worktree { agent })?;
    }
    record::keep(
        cx,
        Decision::new(DecisionKind::RestoredHamlet, reason)
            .hamlet(id),
    )?;
    Ok(Ok(()))
}

/// Erases the removed hamlet `id` for good, with its agents'
/// worktrees and branches.
pub(crate) fn erase(cx: &mut Change<'_>, id: &HamletId) -> Done<()> {
    let Some(found) = find(cx.db, id)? else {
        reject!(Rejection::NoHamlet { id: id.to_string() });
    };
    if found.erased {
        reject!(Rejection::HamletErased { id: id.to_string() });
    }
    if found.is_living() {
        reject!(Rejection::HamletLiving { id: id.to_string() });
    }
    let kept = hamlet_agent::table
        .inner_join(agent::table)
        .filter(hamlet_agent::hamlet.eq(id))
        .filter(agent::erased.eq(false))
        .select(hamlet_agent::agent)
        .load::<AgentId>(cx.db)?;
    for agent in kept {
        done!(hamlet_agents::erase(cx, agent));
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
