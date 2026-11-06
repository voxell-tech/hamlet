//! The realm as a client sees it: composed from the database, the
//! repositories and what the agents are doing now.

use std::collections::BTreeMap;

use anyhow::Result;
use diesel::prelude::*;
use hamlet_git::{MAIN, Repository};
use hamlet_protocol::client as protocol;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, HamletId, ScholarId};
use hamlet_store::schema::{agent, message, permission_grant};

use crate::layout::Layout;
use crate::society::agents::{Agent, AgentKind, EndHow};
use crate::society::city::{self, Arrival, Scholar};
use crate::society::hamlets::{self, Hamlet, Mode};
use crate::society::limits::{
    self, Limits, Period, RealmLimits, Review,
};
use crate::society::roads::{self, RoadKind, Way};
use crate::society::{addresses, chiefs, hand, hires, workers};
use crate::talk::activity::{self, Activity};
use crate::talk::{Env, conversations, inputs};
use crate::{knowledge, spend};

/// What composing the state reads besides the database.
pub(crate) struct Sources<'a> {
    pub layout: &'a Layout,
    pub env: &'a Env,
    pub chief: String,
    pub confinement: protocol::Confinement,
    pub activity: &'a BTreeMap<AgentId, Activity>,
    pub providers: Vec<protocol::ProviderView>,
    pub deeds: Vec<protocol::Deed>,
    pub settings_view: protocol::SettingsView,
    pub waits: Vec<protocol::Wait>,
    pub library: Vec<protocol::LibrarySkill>,
    pub starters: Vec<protocol::Skill>,
}

/// The whole state at `at`.
pub(crate) fn compose(
    db: &mut Db,
    sources: &Sources<'_>,
    epoch: i64,
    revision: i64,
    at: i64,
) -> Result<protocol::State> {
    let mut agents_shown = Vec::new();
    let kept = agent::table
        .filter(agent::erased.eq(false))
        .order(agent::id)
        .select(Agent::as_select())
        .load::<Agent>(db)?;
    for found in &kept {
        agents_shown.push(agent_view(db, sources, found, at)?);
    }
    let mut hamlets_shown = Vec::new();
    let mut closed = Vec::new();
    for place in hamlets::all(db)? {
        if place.is_living() {
            hamlets_shown.push(hamlet_view(db, &place, at)?);
        } else {
            closed.push(protocol::ClosedHamlet {
                at: place.removed_at.unwrap_or_default(),
                by: addresses::actor_name(db, place.removed_by)?,
                id: place.id.to_string(),
                name: place
                    .title
                    .clone()
                    .unwrap_or_else(|| place.id.to_string()),
                reason: place
                    .removed_reason
                    .clone()
                    .unwrap_or_default(),
            });
        }
    }
    let mut scholars_shown = Vec::new();
    for scholar in city::all(db)? {
        scholars_shown.push(scholar_view(
            db,
            sources.layout,
            &scholar,
        )?);
    }
    Ok(protocol::State {
        agents: agents_shown,
        chief: sources.chief.clone(),
        closed,
        confinement: sources.confinement.clone(),
        deeds: sources.deeds.clone(),
        epoch,
        grants: grants(db)?,
        hamlets: hamlets_shown,
        hand: wire_agent(hand::id(db)?),
        library: sources.library.clone(),
        limits: realm_limits(&limits::realm(db)?),
        providers: sources.providers.clone(),
        realm: hamlet_os::fs::path::text(sources.layout.root()),
        revision,
        scholars: scholars_shown,
        settings: sources.settings_view.clone(),
        starters: sources.starters.clone(),
        waits: sources.waits.clone(),
    })
}

pub(crate) fn wire_agent(id: AgentId) -> protocol::AgentId {
    protocol::AgentId(u64::try_from(id.0).unwrap_or_default())
}

pub(crate) fn wire_scholar(id: ScholarId) -> protocol::ScholarId {
    protocol::ScholarId(u64::try_from(id.0).unwrap_or_default())
}

fn wire_time(at: i64) -> u64 {
    u64::try_from(at).unwrap_or_default()
}

fn agent_view(
    db: &mut Db,
    sources: &Sources<'_>,
    found: &Agent,
    at: i64,
) -> Result<protocol::Agent> {
    let id = found.id;
    let name = addresses::name_of(db, id)?;
    let (hamlet, number, scholar, parent, purpose, mayor) =
        match found.kind {
            AgentKind::Hand => {
                let scholar = hand::scholar(db)?;
                (
                    None,
                    None,
                    Some(scholar),
                    None,
                    String::new(),
                    false,
                )
            }
            AgentKind::Hire => {
                let hire = hires::get(db, id)?;
                let mayor = hires::is_mayor(db, id)?;
                let parent = match hire.parent {
                    Some(parent) => Some(parent),
                    None => Some(hand::id(db)?),
                };
                (
                    Some(hire.hamlet.to_string()),
                    Some(hire.number),
                    Some(hire.scholar),
                    parent,
                    hire.purpose,
                    mayor && found.is_living(),
                )
            }
            AgentKind::Worker => {
                let worker = workers::get(db, id)?;
                (
                    worker.hamlet.map(|h| h.to_string()),
                    None,
                    None,
                    worker.starter,
                    worker.brief,
                    false,
                )
            }
        };
    let model = crate::talk::models::choose(
        db,
        &sources.env.settings,
        &sources.env.overrides,
        id,
    )
    .map(|choice| {
        format!("{}::{}", choice.model.provider, choice.model.id)
    })
    .unwrap_or_default();
    let shown = sources.activity.get(&id);
    let doing =
        shown.map_or(protocol::Doing::Rest, |a| a.doing.clone());
    let spent = spend::of_agent(db, id, at)?;
    let left = match (found.ended_at, found.ended_how) {
        (Some(ended), Some(how)) => Some(protocol::Left {
            at: wire_time(ended),
            by: addresses::actor_name(db, found.ended_by)?,
            how: match how {
                EndHow::Ended => protocol::LeftHow::Ended,
                EndHow::Merged => protocol::LeftHow::Merged,
                EndHow::Scholar => protocol::LeftHow::Scholar,
                EndHow::Hamlet => protocol::LeftHow::Hamlet,
                EndHow::Answered => protocol::LeftHow::Answered,
                EndHow::Failed => protocol::LeftHow::Failed,
                EndHow::Stopped => protocol::LeftHow::Stopped,
            },
            into: hires::find(db, id)?
                .and_then(|h| h.merged_into)
                .map(wire_agent),
            reason: found.ended_reason.clone().unwrap_or_default(),
            words: how.words().into(),
        }),
        _ => None,
    };
    let (pin, nickname) = match found.kind {
        AgentKind::Hire => {
            let hire = hires::get(db, id)?;
            (hire.pin, hire.nickname)
        }
        _ => (None, None),
    };
    let skills = match found.kind {
        AgentKind::Worker => Vec::new(),
        _ => knowledge::lodging(db, sources.layout, id)
            .map(|lodging| lodge_skills(&lodging.path))
            .unwrap_or_default(),
    };
    Ok(protocol::Agent {
        activity: protocol::AgentActivity {
            doing_words: activity::words(&doing).into(),
            doing,
            failures: shown.map_or(0, |a| {
                u64::try_from(a.failures).unwrap_or(0)
            }),
            step: shown
                .map_or(0, |a| u64::try_from(a.step).unwrap_or(0)),
        },
        busy: found.is_living() && inputs::is_agent_busy(db, id)?,
        consulting: found.is_living()
            && crate::talk::errands::is_waited_on(
                db,
                id,
                crate::calls::LIBRARIAN,
            )?,
        conversations: peers(db, id)?,
        faulted: shown.is_some_and(|a| a.faulted),
        hamlet,
        id: wire_agent(id),
        kind: match found.kind {
            AgentKind::Hand => protocol::AgentKind::Hand,
            AgentKind::Hire => protocol::AgentKind::Hire,
            AgentKind::Worker => protocol::AgentKind::Worker,
        },
        left,
        mayor,
        model,
        name,
        nickname,
        number: number.and_then(|n| u64::try_from(n).ok()),
        parent: parent.map(wire_agent),
        pin,
        purpose,
        saying: protocol::AgentSaying {
            text: shown.map(|a| a.saying.clone()).unwrap_or_default(),
            thought: shown.is_some_and(|a| a.thought),
        },
        scholar: scholar.map(wire_scholar),
        since: wire_time(found.since),
        skills,
        spent: protocol::AgentSpent {
            cost: spend::money(spent.micros),
            micros: spent.micros,
            ms: spent.ms,
            tokens: spent.tokens,
        },
    })
}

/// The names of the skills in a lodge's `skills/`.
fn lodge_skills(lodge: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(lodge.join("skills")) else {
        return Vec::new();
    };
    let mut names = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .filter_map(|entry| {
            entry.file_name().to_str().map(str::to_owned)
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// The conversations other agents keep with `id`, by sender and kind.
fn peers(
    db: &mut Db,
    id: AgentId,
) -> Result<Vec<protocol::ConversationView>> {
    let mut out = Vec::new();
    for kept in conversations::of_agent(db, id)? {
        let (Some(peer), kind) = (kept.peer, kept.kind) else {
            continue;
        };
        let kind = match kind {
            conversations::Kind::Letters => {
                protocol::ConversationViewKind::Letters
            }
            conversations::Kind::Questions => {
                protocol::ConversationViewKind::Questions
            }
            _ => continue,
        };
        let sent = message::table
            .filter(message::to_conversation.eq(kept.id))
            .select((
                diesel::dsl::count_star(),
                diesel::dsl::max(message::at),
            ))
            .first::<(i64, Option<i64>)>(db)?;
        out.push(protocol::ConversationView {
            count: sent.0,
            kind,
            last: sent.1,
            with: wire_agent(peer),
        });
    }
    Ok(out)
}

fn hamlet_view(
    db: &mut Db,
    place: &Hamlet,
    at: i64,
) -> Result<protocol::Hamlet> {
    let own = limits::of(db, &place.id)?;
    let mut roads_shown = Vec::new();
    let living = hires::living_in(db, &place.id)?;
    let mut seen = std::collections::BTreeSet::new();
    for hire in &living {
        for (other, way) in roads::ways(db, hire.agent)? {
            let pair = (hire.agent.min(other), hire.agent.max(other));
            if !seen.insert(pair) {
                continue;
            }
            let kind = match way {
                Way::Tree => protocol::RoadKind::Tree,
                Way::Mayors => protocol::RoadKind::Mayors,
                Way::Road(id) => {
                    let found = roads::open(db, pair.0, pair.1)?;
                    match found.map(|r| r.kind) {
                        Some(RoadKind::Agreed) => {
                            protocol::RoadKind::Agreed
                        }
                        _ => {
                            let _ = id;
                            protocol::RoadKind::Laid
                        }
                    }
                }
            };
            let used = message::table
                .filter(
                    message::sender
                        .eq(pair.0)
                        .and(message::receiver.eq(pair.1))
                        .or(message::sender
                            .eq(pair.1)
                            .and(message::receiver.eq(pair.0))),
                )
                .order(message::at.desc())
                .select((message::sender, message::at))
                .load::<(AgentId, i64)>(db)?;
            let level = road_level(
                used.len(),
                used.first().map(|(_, at)| *at),
                &own,
                at,
            );
            roads_shown.push(protocol::Road {
                a: wire_agent(pair.0),
                b: wire_agent(pair.1),
                kind,
                last_from: used
                    .first()
                    .map(|(sender, _)| wire_agent(*sender)),
                level,
                messages: u64::try_from(used.len())
                    .unwrap_or_default(),
            });
        }
    }
    let mut chiefs_shown = Vec::new();
    for chief in hamlets::chiefs(db, &place.id)? {
        if let Some(found) = chiefs::find(db, chief)? {
            chiefs_shown.push(found.name);
        }
    }
    let before = hamlets::folders(db, &place.id)?
        .into_iter()
        .filter(|folder| Some(folder) != place.folder.as_ref())
        .collect();
    Ok(protocol::Hamlet {
        at: match (place.site_x, place.site_z) {
            (Some(x), Some(z)) => Some(protocol::TownAt { x, z }),
            _ => None,
        },
        before,
        budget: spend::report(db, Some(&place.id), at)?,
        chiefs: chiefs_shown,
        folder: place.folder.clone(),
        id: place.id.to_string(),
        limits: hamlet_limits(&own),
        mayor: wire_agent(place.mayor),
        mode: match place.mode {
            Mode::Manual => protocol::HamletMode::Manual,
            Mode::Auto => protocol::HamletMode::Auto,
        },
        mode_words: match (place.mode, place.folder.is_some()) {
            (_, false) => "Hires work in their lodges; no folder is \
                           chosen."
                .into(),
            (Mode::Manual, true) => "Hires ask you before each edit \
                                     and command in the folder."
                .into(),
            (Mode::Auto, true) => "Hires edit the folder and run \
                                   commands without asking."
                .into(),
        },
        name: place
            .title
            .clone()
            .unwrap_or_else(|| place.id.to_string()),
        roads: roads_shown,
        tier: u64::try_from(place.tier).unwrap_or_default(),
    })
}

/// How worn a road is: by how much it carries, grown over once unused
/// for its days (H3).
fn road_level(
    messages: usize,
    last: Option<i64>,
    limits: &Limits,
    at: i64,
) -> protocol::RoadLevel {
    let grown = last.is_none_or(|last| {
        at - last > crate::clock::days(limits.road.grows_over_days)
    });
    match messages {
        _ if grown && messages == 0 => protocol::RoadLevel::Dirt,
        _ if grown => protocol::RoadLevel::Overgrown,
        0..=4 => protocol::RoadLevel::Dirt,
        5..=24 => protocol::RoadLevel::Paved,
        _ => protocol::RoadLevel::Rail,
    }
}

fn scholar_view(
    db: &mut Db,
    layout: &Layout,
    scholar: &Scholar,
) -> Result<protocol::Scholar> {
    let mut hires_shown = Vec::new();
    for hire in hires_of(db, scholar)? {
        hires_shown.push(wire_agent(hire));
    }
    let (skills, versions) =
        match Repository::open(&layout.repository(scholar.id)) {
            Ok(repo) => {
                let skills = match repo.tip(MAIN)? {
                    Some(tip) => {
                        let mut names = repo
                            .entries(tip)?
                            .into_iter()
                            .filter_map(|e| {
                                let rest =
                                    e.path.strip_prefix("skills/")?;
                                let (name, file) =
                                    rest.split_once('/')?;
                                (file == "SKILL.md")
                                    .then(|| name.to_owned())
                            })
                            .collect::<Vec<_>>();
                        names.sort();
                        names.dedup();
                        names
                    }
                    None => Vec::new(),
                };
                let versions = repo
                    .versions()?
                    .into_iter()
                    .map(|v| protocol::VersionView {
                        at: wire_time(v.at),
                        name: v.name,
                        note: v.note,
                    })
                    .collect();
                (skills, versions)
            }
            Err(_) => (Vec::new(), Vec::new()),
        };
    let chief = match scholar.chief {
        Some(chief) => chiefs::find(db, chief)?
            .map(|c| c.name)
            .unwrap_or_default(),
        None => String::new(),
    };
    Ok(protocol::Scholar {
        arrival: match scholar.arrival {
            Arrival::Housed => protocol::Arrival::Housed,
            Arrival::Waiting => protocol::Arrival::Waiting,
            Arrival::Turned => protocol::Arrival::Turned,
        },
        arrival_by: match scholar.arrival_by {
            Some(by) => Some(addresses::actor_name(db, Some(by))?),
            None => None,
        },
        chief,
        hires: hires_shown,
        house: match (scholar.house_x, scholar.house_z) {
            (Some(x), Some(z)) => Some(protocol::HouseAt {
                turn: scholar.house_turn.unwrap_or_default(),
                x,
                z,
            }),
            _ => None,
        },
        id: wire_scholar(scholar.id),
        left: match scholar.left_at {
            Some(left) => Some(protocol::ScholarLeft {
                at: wire_time(left),
                by: addresses::actor_name(db, scholar.left_by)?,
                reason: scholar
                    .left_reason
                    .clone()
                    .unwrap_or_default(),
            }),
            None => None,
        },
        name: scholar.name.clone(),
        since: wire_time(scholar.since),
        skills,
        versions,
    })
}

/// The hires of `scholar` not erased, living ones and ended ones that
/// may come back.
fn hires_of(db: &mut Db, scholar: &Scholar) -> Result<Vec<AgentId>> {
    use hamlet_store::schema::hire;
    Ok(hire::table
        .inner_join(agent::table)
        .filter(hire::scholar.eq(scholar.id))
        .filter(agent::erased.eq(false))
        .order(hire::agent)
        .select(hire::agent)
        .load(db)?)
}

fn grants(db: &mut Db) -> Result<Vec<protocol::Grant>> {
    let open = permission_grant::table
        .filter(permission_grant::revoked_at.is_null())
        .order(permission_grant::id)
        .select((
            permission_grant::agent,
            permission_grant::permission,
            permission_grant::chief,
        ))
        .load::<(AgentId, String, hamlet_store::ids::ChiefId)>(db)?;
    let mut out = Vec::new();
    for (agent, permission, chief) in open {
        out.push(protocol::Grant {
            agent: Some(wire_agent(agent)),
            by: chiefs::find(db, chief)?.map(|c| c.name),
            name: addresses::name_of(db, agent)?,
            permission: if permission == "network" {
                protocol::Permission::Network
            } else {
                protocol::Permission::Unconfined
            },
            source: protocol::GrantSource::Grant,
        });
    }
    Ok(out)
}

fn budget_view(budget: &limits::Budget) -> protocol::LimitsBudget {
    protocol::LimitsBudget {
        cost: budget.cost.map(|micros| micros as f64 / 1_000_000.0),
        minutes: budget.minutes,
        period: match budget.period {
            Period::Day => protocol::Period::Day,
            Period::Month => protocol::Period::Month,
        },
    }
}

pub(crate) fn hamlet_limits(own: &Limits) -> protocol::Limits {
    protocol::Limits {
        asking: protocol::LimitsAsking {
            questions: own.asking.questions,
            refusals: own.asking.refusals,
            review: match own.asking.review {
                Review::Always => protocol::LimitsReview::Always,
                Review::Remote => protocol::LimitsReview::Remote,
                Review::Never => protocol::LimitsReview::Never,
            },
        },
        budget: budget_view(&own.budget),
        depth: own.depth,
        model: own.model.clone(),
        models: own.models.clone(),
        protected_minutes: own.protected_minutes,
        reserved: protocol::LimitsReserved {
            end: own.reserved.end,
            hire: own.reserved.hire,
            merge: own.reserved.merge,
            roads: own.reserved.roads,
        },
        road: protocol::LimitsRoad {
            grows_over_days: own.road.grows_over_days,
            most: own.road.most,
            price: own.road.price as f64 / 1_000_000.0,
        },
        task: protocol::LimitsTask {
            cost: own
                .task
                .cost
                .map(|micros| micros as f64 / 1_000_000.0),
            messages: own.task.messages,
            minutes: own.task.minutes,
        },
        told_days: own.told_days,
    }
}

pub(crate) fn realm_limits(
    own: &RealmLimits,
) -> protocol::RealmLimits {
    protocol::RealmLimits {
        budget: budget_view(&own.budget),
        roads_between: own.roads_between,
    }
}

/// The hamlet `id` of a protocol message.
pub(crate) fn hamlet_id(id: &str) -> HamletId {
    HamletId(id.to_owned())
}
