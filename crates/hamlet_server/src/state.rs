//! Protocol views of the record and the chief's settings.

use anyhow::{Context, Result};
use chrono::{
    Datelike, Local, LocalResult, NaiveDateTime, NaiveTime,
    TimeDelta, TimeZone,
};
use hamlet_protocol::client::{
    Agent, AgentActivity, AgentRef, AgentSaying, AgentSpent, Changes,
    ClosedHamlet, Doing, Grant, GrantSource, Hamlet, Left, LeftHow,
    Period, Permission, Road, ServerMessage, SettingsView,
    SettingsViewEndpoints, SettingsViewWorkspace, State, TownAt,
    Update,
};

use crate::record::{self, End};
use crate::service::{Service, records};

/// A state, with the first instant after it was built when one of
/// its views changes with the clock alone: a road overgrows, or a
/// budget's share gone moves on a step, the last of which starts its
/// next period and its books.
pub(crate) struct Built {
    pub(crate) state: State,
    pub(crate) turns: i64,
}

pub(crate) fn state(
    service: &mut Service,
    epoch: i64,
    revision: i64,
) -> Result<State> {
    Ok(build(service, epoch, revision)?.state)
}

pub(crate) fn build(
    service: &mut Service,
    epoch: i64,
    revision: i64,
) -> Result<Built> {
    let now = crate::clock::now();
    let mut turns = i64::MAX;
    let settings = service.settings()?;
    let records = records(service.db())?;
    let realm = crate::realm::read(service.db())?;
    let closed = realm
        .hamlets
        .iter()
        .filter(|h| !h.erased)
        .filter_map(|h| {
            h.removed.as_ref().map(|removed| ClosedHamlet {
                id: h.id.clone(),
                name: h.title.clone().unwrap_or_else(|| h.id.clone()),
                at: removed.at,
                by: removed.by.clone(),
                reason: removed.reason.clone(),
            })
        })
        .collect();
    let mut hamlets = Vec::new();
    let mut agents = Vec::new();
    let mut grants = crate::gate::grants(service.db())?
        .into_iter()
        .map(|grant| Grant {
            agent: Some(grant.agent.into()),
            name: grant.name,
            permission: grant.permission,
            by: Some(grant.by),
            source: GrantSource::Grant,
        })
        .collect::<Vec<_>>();
    let root = service.home.layout().root().to_owned();
    let mut kept = Vec::new();
    for record in records {
        let Some(place) = realm.live().find(|h| h.id == record.id)
        else {
            continue;
        };
        let limits = &record.limits.chief;
        let grows = limits.road.grows_over_days;
        turns = turns.min(budget_step(now, &limits.budget.period)?);
        let work = service.work(&record.id)?;
        let mut overgrows = |last_used: i64| {
            let at = last_used
                .saturating_add(crate::clock::days(grows))
                .saturating_add(1);
            if last_used > 0 && at > now {
                turns = turns.min(at);
            }
        };
        let reference = |number| AgentRef {
            hamlet: record.id.clone(),
            number,
        };
        let end = |end| {
            reference(match end {
                End::Office => record.mayor,
                End::Agent(number) => number,
            })
        };
        let mut roads = record
            .roads_of()
            .iter()
            .inspect(|r| overgrows(r.last_used))
            .map(|r| Road {
                a: end(r.a),
                b: end(r.b),
                messages: r.messages,
                last_from: r.last_from.map(end),
                level: record::level(
                    r.last_used,
                    r.messages,
                    now,
                    grows,
                ),
            })
            .collect::<Vec<_>>();
        roads.extend(
            crate::realm::roads(service.db())?
                .into_iter()
                .filter(|r| {
                    r.a.hamlet == record.id || r.b.hamlet == record.id
                })
                .inspect(|r| overgrows(r.last_used))
                .map(|r| Road {
                    level: record::level(
                        r.last_used,
                        r.messages,
                        now,
                        grows,
                    ),
                    a: r.a.into(),
                    b: r.b.into(),
                    messages: r.messages,
                    last_from: r.last_from.map(Into::into),
                }),
        );
        let books = crate::spend::books(
            service.db(),
            &record.id,
            &limits.budget.period,
            None,
            now,
        )?;
        hamlets.push(Hamlet {
            id: record.id.clone(),
            name: place
                .title
                .clone()
                .unwrap_or_else(|| record.id.clone()),
            at: place.at.map(|at| TownAt { x: at.x, z: at.z }),
            roads,
            votes: crate::society::votes::views(
                service.db(),
                &record,
            )?,
            folder: work.folder,
            before: work.before,
            mode: work.mode.into(),
            mode_words: work.mode.words().into(),
            limits: (**limits).clone().into(),
            budget: service.budget(&record.id)?,
        });
        for agent in record.agents.iter().chain(
            record
                .archive
                .iter()
                .filter(|a| {
                    a.back.is_none()
                        && !record
                            .agents
                            .iter()
                            .any(|live| live.number == a.agent.number)
                })
                .map(|a| &a.agent),
        ) {
            let left = record.archive.iter().rev().find(|a| {
                a.back.is_none()
                    && a.agent.number == agent.number
                    && !record
                        .agents
                        .iter()
                        .any(|live| live.number == agent.number)
            });
            let mayor =
                left.is_none() && agent.number == record.mayor;
            let reference_of = record::AgentRef {
                hamlet: record.id.clone(),
                number: agent.number,
            };
            let work = crate::chats::work(service, &reference_of)?;
            let busy = left.is_none() && work.busy;
            let (doing, step, failures) = match left {
                Some(_) => (Doing::Rest, 0, 0),
                None => (work.doing, work.step, work.failures),
            };
            let spent = books
                .agents
                .get(&agent.number)
                .copied()
                .unwrap_or_default();
            let skills = match left {
                Some(_) => Vec::new(),
                None => skills(service, &record.id, agent.number)?,
            };
            let left = left
                .map(|left| {
                    Ok::<_, anyhow::Error>(Left {
                        how: match left.merge {
                            Some(_) => LeftHow::Merged,
                            None => LeftHow::Removed,
                        },
                        into: left
                            .merge
                            .as_ref()
                            .map(|m| reference(m.kept.number)),
                        at: u64::try_from(left.at).with_context(
                            || format!("{} left at", left.id),
                        )?,
                        by: left.by.words(),
                        reason: left.reason.clone(),
                        words: record.farewell(left),
                    })
                })
                .transpose()?;
            agents.push(Agent {
                agent: reference(agent.number),
                name: agent.name.clone(),
                purpose: agent.purpose.clone(),
                since: u64::try_from(agent.since).with_context(
                    || format!("{}'s start", agent.name),
                )?,
                mayor,
                model: limits.model_of(agent, mayor, &settings.model),
                busy,
                faulted: !busy && work.faulted,
                activity: AgentActivity {
                    doing_words: crate::chats::words(&doing).into(),
                    doing,
                    step: u64::try_from(step).with_context(|| {
                        format!("{}'s step", agent.name)
                    })?,
                    failures: u64::try_from(failures).with_context(
                        || format!("{}'s failures", agent.name),
                    )?,
                },
                saying: AgentSaying {
                    text: work.saying,
                    thought: work.thought,
                },
                spent: AgentSpent {
                    cost: crate::spend::money(spent.micros),
                    tokens: spent.tokens,
                    micros: spent.micros,
                    ms: spent.ms,
                },
                skills,
                consulting: crate::society::library::consulting(
                    service.db(),
                    &reference_of,
                )?,
                conversations: crate::society::letters::views(
                    service.db(),
                    &reference_of,
                )?,
                left,
            });
        }
        for (permission, names) in [
            (Permission::Network, &limits.permissions.network),
            (Permission::Unconfined, &limits.permissions.unconfined),
        ] {
            for name in names {
                let agent = record.agents.iter().find(|a| {
                    if name == hamlet_protocol::MAYOR {
                        a.number == record.mayor
                    } else {
                        name == &a.name
                    }
                });
                grants.push(Grant {
                    agent: agent.map(|a| reference(a.number)),
                    name: name.clone(),
                    permission: permission.clone(),
                    by: None,
                    source: GrantSource::Limits,
                });
            }
        }
        kept.push(record);
    }
    let mut waits = Vec::new();
    for wait in crate::oversight::open(service.db())? {
        let record = wait.chat.as_ref().and_then(|chat| {
            kept.iter().find(|r| r.id == chat.hamlet)
        });
        let current = match &wait.chat {
            Some(chat) => crate::chats::records::current_number(
                service.db(),
                &record::AgentRef {
                    hamlet: chat.hamlet.clone(),
                    number: chat.number,
                },
                &chat.chief,
            )?,
            None => None,
        };
        waits.push(crate::oversight::view(&wait, record, current)?);
    }
    let endpoints = settings
        .endpoints
        .iter()
        .map(|e| SettingsViewEndpoints {
            name: e.name.clone(),
            base_url: e.base_url.clone(),
            key: service.credentials.is_configured(&e.name),
        })
        .collect();
    let state = State {
        epoch,
        revision,
        chief: service.chief.clone(),
        confinement: service.houses.confinement(),
        houses: root
            .join("houses")
            .to_str()
            .context("the houses folder's path is not UTF-8")?
            .into(),
        hamlets,
        agents,
        grants,
        waits,
        providers: service.provider_views()?,
        library: crate::society::library::views(service)?,
        closed,
        starters: crate::gifts::starters(),
        deeds: crate::chats::advertised_deeds(),
        settings: SettingsView {
            model: settings.model,
            reflect_every: i64::try_from(settings.reflect_every)
                .context("the settings' reflectEvery")?,
            allow_net: settings.allow_net,
            unset: !settings.saved,
            link: settings.link,
            endpoints,
            workspace: SettingsViewWorkspace {
                url: service
                    .credentials
                    .workspace_url()
                    .map(Into::into),
                running: service
                    .houses
                    .workspace()
                    .map(|connection| connection.url.clone()),
                moving: service.moving.view(),
            },
        },
    };
    Ok(Built { state, turns })
}

/// The first instant after `now` at the local wall-clock time `at`.
/// A time the clock skips is looked at again in a minute.
fn instant(at: NaiveDateTime, now: i64) -> i64 {
    match Local.from_local_datetime(&at) {
        LocalResult::Single(at) => at.timestamp_millis(),
        LocalResult::Ambiguous(first, second) => {
            let first = first.timestamp_millis();
            if first > now {
                first
            } else {
                second.timestamp_millis()
            }
        }
        LocalResult::None => now.saturating_add(60_000),
    }
}

/// The next instant after `now` when the share gone of a budget of
/// `period` may move, the period's end among them. `spend`'s report
/// counts it in steps of 1/200 of the period's wall-clock seconds;
/// its division can show a step a second late, so the step's own
/// second is followed by the next.
fn budget_step(now: i64, period: &Period) -> Result<i64> {
    let at = crate::clock::local(now)?.naive_local();
    let (first, step) = match period {
        Period::Day => (at.date(), 432),
        Period::Month => (
            at.date().with_day(1).context("the month's first day")?,
            i64::from(at.date().num_days_in_month()) * 432,
        ),
    };
    let start = first.and_time(NaiveTime::MIN);
    let gone = (at - start).num_seconds();
    let next = if gone % step == 0 {
        gone + 1
    } else {
        (gone / step + 1) * step
    };
    Ok(instant(start + TimeDelta::seconds(next), now))
}

/// The skills in the house of agent `number` of `hamlet`.
fn skills(
    service: &mut Service,
    hamlet: &str,
    number: u64,
) -> Result<Vec<String>> {
    service.houses.skill_names(hamlet, number)
}

/// Whether the skills in the library or in a living agent's house
/// differ from those `state` shows. A chief may put them there by
/// hand, which no event tells.
pub(crate) fn skills_moved(
    service: &mut Service,
    state: &State,
) -> Result<bool> {
    let library = crate::prompt::shelf::shelf(
        &service.home.layout().root().join("library"),
    );
    if library.len() != state.library.len()
        || library.iter().zip(&state.library).any(|(found, shown)| {
            found.folder != shown.name
                || found.description != shown.description
        })
    {
        return Ok(true);
    }
    for agent in state.agents.iter().filter(|a| a.left.is_none()) {
        let found =
            skills(service, &agent.agent.hamlet, agent.agent.number)?;
        if found != agent.skills {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn changed(
    before: &State,
    after: &mut State,
    endings: impl Fn(&str) -> String,
) -> Option<ServerMessage> {
    let mut changes = Vec::new();
    for hamlet in &after.hamlets {
        if before.hamlets.iter().find(|h| h.id == hamlet.id)
            != Some(hamlet)
        {
            changes.push(Update::Hamlet {
                hamlet: hamlet.clone(),
            });
        }
    }
    for agent in &after.agents {
        if before.agents.iter().find(|a| a.agent == agent.agent)
            != Some(agent)
        {
            changes.push(Update::Agent {
                agent: agent.clone(),
            });
        }
    }
    for agent in &before.agents {
        if !after.agents.iter().any(|a| a.agent == agent.agent) {
            changes.push(Update::AgentGone {
                agent: agent.agent.clone(),
            });
        }
    }
    for wait in &after.waits {
        if before.waits.iter().find(|w| w.id == wait.id) != Some(wait)
        {
            changes.push(Update::Wait { wait: wait.clone() });
        }
    }
    for wait in &before.waits {
        if !after.waits.iter().any(|w| w.id == wait.id) {
            changes.push(Update::WaitClosed {
                how: endings(&wait.id),
                id: wait.id.clone(),
            });
        }
    }
    if before.grants != after.grants {
        changes.push(Update::Grants {
            grants: after.grants.clone(),
        });
    }
    if before.settings != after.settings {
        changes.push(Update::Settings {
            settings: after.settings.clone(),
        });
    }
    if before.providers != after.providers {
        changes.push(Update::Providers {
            providers: after.providers.clone(),
        });
    }
    let whole = before
        .hamlets
        .iter()
        .any(|h| !after.hamlets.iter().any(|n| n.id == h.id))
        || before.closed != after.closed
        || before.library != after.library;
    if changes.is_empty() && !whole {
        return None;
    }
    after.revision = before.revision + 1;
    Some(if whole {
        ServerMessage::State(after.clone())
    } else {
        ServerMessage::Change(Changes {
            epoch: after.epoch,
            revision: after.revision,
            changes,
        })
    })
}

#[cfg(test)]
mod tests {
    use chrono::{Local, NaiveDateTime, TimeZone};
    use hamlet_protocol::client::Period;

    use super::budget_step;

    /// The instant of the local wall-clock time `text`, on a day the
    /// clock neither skips nor repeats.
    fn at(text: &str) -> i64 {
        let naive =
            NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
                .unwrap();
        Local
            .from_local_datetime(&naive)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn a_day_budget_steps_every_two_hundredth_of_the_day() {
        // 10:03:00 is 83.75 steps of 432 seconds into the day.
        assert_eq!(
            budget_step(at("2026-06-10 10:03:00"), &Period::Day)
                .unwrap(),
            at("2026-06-10 10:04:48")
        );
        assert_eq!(
            budget_step(at("2026-06-10 23:59:00"), &Period::Day)
                .unwrap(),
            at("2026-06-11 00:00:00")
        );
    }

    #[test]
    fn a_month_budget_steps_every_two_hundredth_of_the_month() {
        // A step of June is 30 * 432 seconds, 3.6 hours.
        assert_eq!(
            budget_step(at("2026-06-10 10:03:00"), &Period::Month)
                .unwrap(),
            at("2026-06-10 10:48:00")
        );
        assert_eq!(
            budget_step(at("2026-06-30 23:00:00"), &Period::Month)
                .unwrap(),
            at("2026-07-01 00:00:00")
        );
    }

    #[test]
    fn a_budget_on_its_step_is_looked_at_again_a_second_later() {
        assert_eq!(
            budget_step(at("2026-06-10 10:04:48"), &Period::Day)
                .unwrap(),
            at("2026-06-10 10:04:49")
        );
    }
}
