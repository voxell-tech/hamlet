//! Agents of hamlets: profiles working in a hamlet, each with a
//! branch, a worktree and a place in the hamlet's tree.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{
    Actor, AgentId, ChiefId, HamletId, ProfileId,
};
use hamlet_store::schema::{
    agent, agent_tool_off, hamlet, hamlet_agent,
};
use serde_json::json;

use super::agents::{self, AgentKind, EndHow};
use super::limits::{self, Micros};
use super::record::{self, Decision, DecisionKind};
use super::rejection::Rejection;
use super::{city, hamlets, tree};
use crate::realm::change::{Change, Done, checked, done, reject};
use crate::realm::jobs::{self, Job};

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = hamlet_agent)]
pub(crate) struct HamletAgent {
    pub agent: AgentId,
    pub hamlet: HamletId,
    pub profile: ProfileId,
    pub parent: Option<AgentId>,
    pub purpose: String,
    pub name: Option<String>,
    pub pin: Option<String>,
    pub base: Option<String>,
    pub model: Option<String>,
    pub model_by: Option<Actor>,
    pub model_reason: Option<String>,
    pub allowance: Option<Micros>,
    pub allowance_by: Option<Actor>,
    pub merged_into: Option<AgentId>,
    pub ending_dream: bool,
}

/// Whether an ended agent dreams once more.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LastDream {
    Without,
    With,
}

/// The profile an agent is of.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ProfileChoice {
    /// A blank agent: a profile made for this agent alone, apart from
    /// the city, so it has no house and never waits at the harbor.
    /// With no name it is named for its place: Lead, or Agent and
    /// its number.
    Blank { name: String, character: String },
    /// A living profile, at its `main` or pinned to a version.
    Existing {
        profile: ProfileId,
        pin: Option<String>,
    },
}

/// An agent to add.
#[derive(Clone, Debug)]
pub(crate) struct Spec {
    pub hamlet: HamletId,
    /// The agent it answers to; `None` makes the hamlet's lead.
    pub parent: Option<AgentId>,
    pub profile: ProfileChoice,
    pub purpose: String,
    pub reason: String,
    /// The chief a new profile belongs to.
    pub owner: ChiefId,
}

/// An agent just added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NewAgent {
    pub agent: AgentId,
    pub profile: ProfileId,
    pub hamlet: HamletId,
}

/// Adds an agent of a profile to a hamlet under a parent, or as the
/// lead of a hamlet being founded.
pub(crate) fn add(cx: &mut Change<'_>, spec: Spec) -> Done<NewAgent> {
    let place = hamlets::get(cx.db, &spec.hamlet)?;
    if !place.is_living() {
        reject!(Rejection::HamletRemoved {
            id: spec.hamlet.to_string()
        });
    }
    if count_living(cx.db, &spec.hamlet)? >= place.tier {
        reject!(Rejection::HamletFull {
            id: spec.hamlet.to_string(),
            most: place.tier,
        });
    }
    if let Some(parent) = spec.parent {
        let above = get(cx.db, parent)?;
        if above.hamlet != spec.hamlet {
            reject!(Rejection::OtherHamlet {
                name: super::addresses::name_of(cx.db, parent)?
            });
        }
        done!(require_living(cx.db, parent));
    }
    let (profile, pin, new) = match spec.profile {
        ProfileChoice::Blank { name, character } => {
            let name = match name.trim() {
                "" if spec.parent.is_none() => "Lead".to_owned(),
                // Counted among every agent the hamlet has had, so
                // the name is not one a living agent of it goes by.
                "" => {
                    let had = hamlet_agent::table
                        .filter(hamlet_agent::hamlet.eq(&spec.hamlet))
                        .count()
                        .get_result::<i64>(cx.db)?;
                    format!("Agent {had}")
                }
                given => given.to_owned(),
            };
            let made = done!(city::make_apart(
                cx, &name, spec.owner, &character
            ));
            (made, None, true)
        }
        ProfileChoice::Existing { profile, pin } => {
            let found = city::get(cx.db, profile)?;
            if let Some(rejection) =
                found.gone().or_else(|| found.apart())
            {
                reject!(rejection);
            }
            (profile, pin, false)
        }
    };
    let agent = agents::make(cx, AgentKind::Hamlet)?;
    diesel::insert_into(hamlet_agent::table)
        .values((
            hamlet_agent::agent.eq(agent),
            hamlet_agent::hamlet.eq(&spec.hamlet),
            hamlet_agent::profile.eq(profile),
            hamlet_agent::parent.eq(spec.parent),
            hamlet_agent::purpose.eq(spec.purpose.trim()),
            hamlet_agent::pin.eq(&pin),
        ))
        .execute(cx.db)?;
    jobs::add(cx, &Job::Worktree { agent })?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Added, &spec.reason)
            .hamlet(&spec.hamlet)
            .subject(agent)
            .profile(profile)
            .detail(json!({
                "new": new,
                "pin": pin,
                "purpose": spec.purpose.trim(),
            })),
    )?;
    cx.touched.hamlets.insert(spec.hamlet.clone());
    cx.touched.profiles.insert(profile);
    Ok(Ok(NewAgent {
        agent,
        profile,
        hamlet: spec.hamlet,
    }))
}

pub(crate) fn find(
    db: &mut Db,
    id: AgentId,
) -> Result<Option<HamletAgent>> {
    Ok(hamlet_agent::table
        .find(id)
        .select(HamletAgent::as_select())
        .first(db)
        .optional()?)
}

pub(crate) fn get(db: &mut Db, id: AgentId) -> Result<HamletAgent> {
    find(db, id)?
        .with_context(|| format!("no agent {id} in a hamlet"))
}

/// The most letters of a first message a purpose keeps.
const PURPOSE_KEPT: usize = 200;

/// Gives an agent with no purpose the first line of `text` as its
/// purpose.
pub(crate) fn take_purpose(
    cx: &mut Change<'_>,
    agent: AgentId,
    text: &str,
) -> Result<()> {
    let Some(found) = find(cx.db, agent)? else {
        return Ok(());
    };
    if !found.purpose.trim().is_empty() {
        return Ok(());
    }
    let line =
        text.lines().map(str::trim).find(|line| !line.is_empty());
    let Some(line) = line else {
        return Ok(());
    };
    let purpose = line.chars().take(PURPOSE_KEPT).collect::<String>();
    diesel::update(hamlet_agent::table.find(agent))
        .set(hamlet_agent::purpose.eq(purpose))
        .execute(cx.db)?;
    cx.touched.agents.insert(agent);
    Ok(())
}

/// The living agents of `hamlet`, in the order they were added.
pub(crate) fn living_in(
    db: &mut Db,
    hamlet: &HamletId,
) -> Result<Vec<HamletAgent>> {
    Ok(hamlet_agent::table
        .inner_join(agent::table)
        .filter(hamlet_agent::hamlet.eq(hamlet))
        .filter(agent::ended_at.is_null())
        .order(hamlet_agent::agent)
        .select(HamletAgent::as_select())
        .load(db)?)
}

/// The living agents of the profile `profile`.
pub(crate) fn living_of(
    db: &mut Db,
    profile: ProfileId,
) -> Result<Vec<HamletAgent>> {
    Ok(hamlet_agent::table
        .inner_join(agent::table)
        .filter(hamlet_agent::profile.eq(profile))
        .filter(agent::ended_at.is_null())
        .order(hamlet_agent::agent)
        .select(HamletAgent::as_select())
        .load(db)?)
}

/// The agents ended with a last dream since `since`, and when each
/// ended.
pub(crate) fn ended_to_dream(
    db: &mut Db,
    since: i64,
) -> Result<Vec<(AgentId, i64)>> {
    Ok(hamlet_agent::table
        .inner_join(agent::table)
        .filter(hamlet_agent::ending_dream.eq(true))
        .filter(agent::ended_at.ge(since))
        .filter(agent::erased.eq(false))
        .order(agent::ended_at)
        .select((
            hamlet_agent::agent,
            agent::ended_at.assume_not_null(),
        ))
        .load(db)?)
}

pub(crate) fn count_living(
    db: &mut Db,
    hamlet: &HamletId,
) -> Result<i64> {
    Ok(hamlet_agent::table
        .inner_join(agent::table)
        .filter(hamlet_agent::hamlet.eq(hamlet))
        .filter(agent::ended_at.is_null())
        .count()
        .get_result(db)?)
}

/// True when `id` holds its hamlet's office.
pub(crate) fn is_lead(db: &mut Db, id: AgentId) -> Result<bool> {
    let agent = get(db, id)?;
    Ok(tree::lead(db, &agent.hamlet)? == id)
}

/// The rejection of a call that names the ended agent `id`.
pub(crate) fn left(db: &mut Db, id: AgentId) -> Result<Rejection> {
    let agent = agents::get(db, id)?;
    let name = super::addresses::name_of(db, id)?;
    Ok(match agent.ended_how {
        _ if agent.erased => Rejection::Erased { name },
        Some(how) => Rejection::Left {
            name,
            how: how.words().into(),
        },
        None => Rejection::NotLeft { name },
    })
}

/// Rejects the agent `id` unless it is living, saying how it left.
pub(crate) fn require_living(db: &mut Db, id: AgentId) -> Done<()> {
    if !agents::get(db, id)?.is_living() {
        reject!(left(db, id)?);
    }
    Ok(Ok(()))
}

/// Ends the living agent `id`.
pub(crate) fn end(
    cx: &mut Change<'_>,
    id: AgentId,
    how: EndHow,
    last: LastDream,
    reason: &str,
) -> Done<()> {
    let agent = get(cx.db, id)?;
    done!(require_living(cx.db, id));
    if how != EndHow::Hamlet {
        if is_lead(cx.db, id)? {
            reject!(Rejection::EndingLead);
        }
        done!(unprotected(cx, id));
        let children = tree::children(cx.db, Some(id))?;
        diesel::update(
            hamlet_agent::table
                .filter(hamlet_agent::agent.eq_any(&children)),
        )
        .set(hamlet_agent::parent.eq(agent.parent))
        .execute(cx.db)?;
        cx.touched.agents.extend(children);
        record::keep(
            cx,
            Decision::new(DecisionKind::Ended, reason)
                .hamlet(&agent.hamlet)
                .subject(id)
                .profile(agent.profile)
                .detail(json!({ "how": how })),
        )?;
    }
    agents::end(cx, id, how, reason)?;
    diesel::update(hamlet_agent::table.find(id))
        .set(hamlet_agent::ending_dream.eq(last == LastDream::With))
        .execute(cx.db)?;
    if last == LastDream::With {
        crate::conversations::reflections::before_last_dream(cx, id)?;
    }
    jobs::add(cx, &Job::Commit { agent: id })?;
    cx.touched.hamlets.insert(agent.hamlet);
    cx.touched.profiles.insert(agent.profile);
    Ok(Ok(()))
}

/// Rejects an agent that would end or merge `agent` while it is on a
/// chief's task (H7); a chief may.
pub(crate) fn unprotected(
    cx: &mut Change<'_>,
    agent: AgentId,
) -> Done<()> {
    if !matches!(cx.actor, Actor::Agent(_)) {
        return Ok(Ok(()));
    }
    let hamlet = get(cx.db, agent)?.hamlet;
    let minutes = limits::of(cx.db, &hamlet)?.protected_minutes;
    if crate::conversations::tasks::protects(
        cx.db, agent, cx.at, minutes,
    )? {
        reject!(Rejection::OnChiefsTask {
            name: super::addresses::name_of(cx.db, agent)?,
        });
    }
    Ok(Ok(()))
}

/// Brings back the ended agent `id` under its old parent, or the lead
/// when that has gone.
pub(crate) fn come_back(
    cx: &mut Change<'_>,
    id: AgentId,
    reason: &str,
) -> Done<()> {
    let agent = get(cx.db, id)?;
    let ended = agents::get(cx.db, id)?;
    if ended.is_living() || ended.erased {
        reject!(left(cx.db, id)?);
    }
    let place = hamlets::get(cx.db, &agent.hamlet)?;
    if !place.is_living() {
        reject!(Rejection::HamletRemoved {
            id: agent.hamlet.to_string()
        });
    }
    let profile = city::get(cx.db, agent.profile)?;
    if let Some(rejection) = profile.gone() {
        reject!(rejection);
    }
    if count_living(cx.db, &agent.hamlet)? >= place.tier {
        reject!(Rejection::HamletFull {
            id: agent.hamlet.to_string(),
            most: place.tier,
        });
    }
    let lead = tree::lead(cx.db, &agent.hamlet)?;
    let parent = match agent.parent {
        Some(parent)
            if parent != id
                && agents::get(cx.db, parent)?.is_living() =>
        {
            parent
        }
        _ => lead,
    };
    diesel::update(hamlet_agent::table.find(id))
        .set((
            hamlet_agent::parent.eq(Some(parent)),
            hamlet_agent::merged_into.eq(None::<AgentId>),
        ))
        .execute(cx.db)?;
    agents::come_back(cx, id)?;
    jobs::add(cx, &Job::Worktree { agent: id })?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Restored, reason)
            .hamlet(&agent.hamlet)
            .subject(id)
            .profile(agent.profile),
    )?;
    cx.touched.hamlets.insert(agent.hamlet);
    cx.touched.profiles.insert(agent.profile);
    Ok(Ok(()))
}

/// Erases the ended agent `id` for good: its worktree and branch go.
pub(crate) fn erase(cx: &mut Change<'_>, id: AgentId) -> Done<()> {
    let agent = get(cx.db, id)?;
    let ended = agents::get(cx.db, id)?;
    if ended.is_living() || ended.erased {
        reject!(left(cx.db, id)?);
    }
    agents::erase(cx, id)?;
    jobs::add(cx, &Job::EraseAgent { agent: id })?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Erased, "")
            .hamlet(&agent.hamlet)
            .subject(id)
            .profile(agent.profile),
    )?;
    cx.touched.hamlets.insert(agent.hamlet);
    Ok(Ok(()))
}

/// Moves the living agent `id`, and everything under it, under the
/// living agent `parent` of its hamlet (H41, ADR 0016).
pub(crate) fn reparent(
    cx: &mut Change<'_>,
    id: AgentId,
    parent: AgentId,
    reason: &str,
) -> Done<()> {
    // A worker or the Hand has no place in a hamlet's tree.
    for one in [id, parent] {
        if find(cx.db, one)?.is_none() {
            reject!(Rejection::NotHamletAgent {
                name: super::addresses::name_of(cx.db, one)?
            });
        }
    }
    let agent = get(cx.db, id)?;
    let target = get(cx.db, parent)?;
    done!(require_living(cx.db, id));
    done!(require_living(cx.db, parent));
    if agent.hamlet != target.hamlet {
        reject!(Rejection::OtherHamlet {
            name: super::addresses::name_of(cx.db, parent)?
        });
    }
    if agent.parent.is_none() {
        reject!(Rejection::MovingLead);
    }
    let name = super::addresses::name_of(cx.db, id)?;
    let new = super::addresses::name_of(cx.db, parent)?;
    if agent.parent == Some(parent) {
        reject!(Rejection::AlreadyUnder { name, parent: new });
    }
    if parent == id || tree::is_above(cx.db, id, parent)? {
        reject!(Rejection::UnderItself { name, parent: new });
    }
    let allowed = match cx.actor {
        // An agent moves only inside its own subtree.
        Actor::Agent(from) => {
            super::authority::may_direct(cx.db, cx.actor, id)?
                && (from == parent
                    || super::authority::may_direct(
                        cx.db, cx.actor, parent,
                    )?)
        }
        _ => super::authority::may_direct(cx.db, cx.actor, id)?,
    };
    if !allowed {
        reject!(Rejection::NotParent { name });
    }
    diesel::update(hamlet_agent::table.find(id))
        .set(hamlet_agent::parent.eq(Some(parent)))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Reparented, reason)
            .hamlet(&agent.hamlet)
            .subject(id)
            .other(parent)
            .detail(json!({ "from": agent.parent })),
    )?;
    cx.touched.agents.extend([id, parent]);
    cx.touched.agents.extend(agent.parent);
    cx.touched.hamlets.insert(agent.hamlet);
    Ok(Ok(()))
}

/// Gives the office of `hamlet` to its agent `id`, the old lead going
/// under it.
pub(crate) fn set_lead(
    cx: &mut Change<'_>,
    hamlet_id: &HamletId,
    id: AgentId,
    reason: &str,
) -> Done<()> {
    let agent = get(cx.db, id)?;
    if &agent.hamlet != hamlet_id {
        reject!(Rejection::OtherHamlet {
            name: super::addresses::name_of(cx.db, id)?
        });
    }
    done!(require_living(cx.db, id));
    let old = tree::lead(cx.db, hamlet_id)?;
    if old == id {
        reject!(Rejection::AlreadyLead {
            name: super::addresses::name_of(cx.db, id)?
        });
    }
    let moved = tree::children(cx.db, Some(old))?
        .into_iter()
        .filter(|child| *child != id)
        .collect::<Vec<_>>();
    diesel::update(
        hamlet_agent::table
            .filter(hamlet_agent::agent.eq_any(&moved)),
    )
    .set(hamlet_agent::parent.eq(Some(id)))
    .execute(cx.db)?;
    diesel::update(hamlet_agent::table.find(old))
        .set(hamlet_agent::parent.eq(Some(id)))
        .execute(cx.db)?;
    diesel::update(hamlet_agent::table.find(id))
        .set(hamlet_agent::parent.eq(None::<AgentId>))
        .execute(cx.db)?;
    diesel::update(hamlet::table.find(hamlet_id))
        .set(hamlet::lead.eq(id))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Seated, reason)
            .hamlet(hamlet_id)
            .subject(id)
            .other(old),
    )?;
    cx.touched.agents.extend(moved);
    cx.touched.agents.extend([old, id]);
    cx.touched.hamlets.insert(hamlet_id.clone());
    Ok(Ok(()))
}

/// Chooses the model of the living agent `id`, or its hamlet's when
/// `None` (H29).
pub(crate) fn set_model(
    cx: &mut Change<'_>,
    id: AgentId,
    model: Option<&str>,
    reason: &str,
) -> Done<()> {
    let agent = get(cx.db, id)?;
    done!(require_living(cx.db, id));
    if matches!(agent.model_by, Some(Actor::Chief(_)))
        && !matches!(cx.actor, Actor::Chief(_))
    {
        reject!(Rejection::ChiefChose {
            name: super::addresses::name_of(cx.db, id)?
        });
    }
    diesel::update(hamlet_agent::table.find(id))
        .set((
            hamlet_agent::model.eq(model),
            hamlet_agent::model_by.eq(model.map(|_| cx.actor)),
            hamlet_agent::model_reason.eq(model.map(|_| reason)),
        ))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Model, reason)
            .hamlet(&agent.hamlet)
            .subject(id)
            .detail(json!({ "model": model })),
    )?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// Turns tools of the living agent `id` off, and back on.
pub(crate) fn set_tools(
    cx: &mut Change<'_>,
    id: AgentId,
    off: &[String],
    on: &[String],
    reason: &str,
) -> Done<()> {
    let agent = get(cx.db, id)?;
    done!(require_living(cx.db, id));
    for tool in off {
        diesel::insert_into(agent_tool_off::table)
            .values((
                agent_tool_off::agent.eq(id),
                agent_tool_off::tool.eq(tool),
                agent_tool_off::at.eq(cx.at),
                agent_tool_off::by.eq(cx.actor),
            ))
            .on_conflict_do_nothing()
            .execute(cx.db)?;
    }
    diesel::delete(
        agent_tool_off::table
            .filter(agent_tool_off::agent.eq(id))
            .filter(agent_tool_off::tool.eq_any(on)),
    )
    .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Tools, reason)
            .hamlet(&agent.hamlet)
            .subject(id)
            .detail(json!({ "off": off, "on": on })),
    )?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// The tools turned off for `id`.
pub(crate) fn tools_off(
    db: &mut Db,
    id: AgentId,
) -> Result<Vec<String>> {
    Ok(agent_tool_off::table
        .filter(agent_tool_off::agent.eq(id))
        .order(agent_tool_off::tool)
        .select(agent_tool_off::tool)
        .load(db)?)
}

/// Gives the living agent `id` an allowance for its hamlet's period,
/// or none.
pub(crate) fn set_allowance(
    cx: &mut Change<'_>,
    id: AgentId,
    allowance: Option<Micros>,
    reason: &str,
) -> Done<()> {
    let agent = get(cx.db, id)?;
    done!(require_living(cx.db, id));
    diesel::update(hamlet_agent::table.find(id))
        .set((
            hamlet_agent::allowance.eq(allowance),
            hamlet_agent::allowance_by
                .eq(allowance.map(|_| cx.actor)),
        ))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Allowance, reason)
            .hamlet(&agent.hamlet)
            .subject(id)
            .detail(json!({ "micros": allowance })),
    )?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// Pins the living agent `id` to `version`, or to `main` when none.
pub(crate) fn set_pin(
    cx: &mut Change<'_>,
    id: AgentId,
    version: Option<&str>,
    reason: &str,
) -> Done<()> {
    let agent = get(cx.db, id)?;
    done!(require_living(cx.db, id));
    diesel::update(hamlet_agent::table.find(id))
        .set(hamlet_agent::pin.eq(version))
        .execute(cx.db)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::Pinned, reason)
            .hamlet(&agent.hamlet)
            .subject(id)
            .profile(agent.profile)
            .detail(json!({ "version": version })),
    )?;
    jobs::add(cx, &Job::TakeIn { agent: id })?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// The most letters the name of an agent or a worker has.
pub(crate) const NAME_LETTERS: usize = 24;

/// `name` trimmed, none when it is empty.
pub(crate) fn check_name(
    name: Option<&str>,
) -> Result<Option<&str>, Rejection> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    match name {
        Some(name)
            if name.chars().count() > NAME_LETTERS
                || name.contains(char::is_control) =>
        {
            Err(Rejection::BadAgentName { most: NAME_LETTERS })
        }
        _ => Ok(name),
    }
}

/// Writes `id`'s new name into its conversation, when `changed`.
pub(crate) fn renamed(
    cx: &mut Change<'_>,
    id: AgentId,
    changed: bool,
) -> Result<()> {
    if !changed {
        return Ok(());
    }
    let name = super::addresses::name_of(cx.db, id)?;
    crate::conversations::chats::event(
        cx,
        id,
        hamlet_protocol::client::ItemEvent::Renamed { name },
    )
}

/// Calls the living agent `id` `name` in the world, or by its
/// profile's name again when none (H46).
pub(crate) fn set_name(
    cx: &mut Change<'_>,
    id: AgentId,
    name: Option<&str>,
    reason: &str,
) -> Done<()> {
    let agent = get(cx.db, id)?;
    done!(require_living(cx.db, id));
    let name = checked!(check_name(name));
    diesel::update(hamlet_agent::table.find(id))
        .set(hamlet_agent::name.eq(name))
        .execute(cx.db)?;
    renamed(cx, id, agent.name.as_deref() != name)?;
    record::keep(
        cx,
        Decision::new(DecisionKind::AgentNamed, reason)
            .hamlet(&agent.hamlet)
            .subject(id)
            .detail(json!({ "name": name })),
    )?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// Keeps the commit of `main`, or the pin, that the branch of `id`
/// took in last.
pub(crate) fn took_in(
    db: &mut Db,
    id: AgentId,
    base: &str,
) -> Result<()> {
    diesel::update(hamlet_agent::table.find(id))
        .set(hamlet_agent::base.eq(base))
        .execute(db)?;
    Ok(())
}
