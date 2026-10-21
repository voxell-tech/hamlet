//! Protocol views of the record and the chief's settings.

use anyhow::{Context, Result};
use hamlet_protocol::v0::{
    Agent, AgentActivity, AgentRef, AgentSaying, AgentSpent, Changes,
    ClosedHamlet, Doing, Grant, GrantSource, Hamlet, Left, LeftHow,
    Permission, Road, ServerMessage, SettingsView,
    SettingsViewEndpoints, SettingsViewWorkspace, State, TownAt,
    Update,
};

use crate::record::{self, End};
use crate::service::{Service, records};

pub(crate) fn state(
    service: &mut Service,
    epoch: i64,
    revision: i64,
) -> Result<State> {
    let settings = service.settings()?;
    let records = records(service.home.store().connection())?;
    let realm =
        crate::realm::read(service.home.store().connection())?;
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
    let mut grants =
        crate::gate::grants(service.home.store().connection())?
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
        let grows =
            u64::try_from(limits.road.grows_over_days).unwrap_or(0);
        let work = service.work(&record.id)?;
        let now = crate::society::now();
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
            crate::realm::roads(service.home.store().connection())?
                .into_iter()
                .filter(|r| {
                    r.a.hamlet == record.id || r.b.hamlet == record.id
                })
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
            service.home.store().connection(),
            &record.id,
            &limits.budget.period,
            None,
            chrono::Utc::now().timestamp_millis(),
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
                service.home.store().connection(),
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
            let reference_of = crate::record::AgentRef {
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
                None => service.houses.skill_names(
                    &record.id,
                    agent.number,
                    &root
                        .join("houses")
                        .join(&record.id)
                        .join(agent.number.to_string()),
                )?,
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
                ref_: reference(agent.number),
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
                    service.home.store().connection(),
                    &reference_of,
                )?,
                conversations: crate::society::letters::views(
                    service.home.store().connection(),
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
    for wait in
        crate::oversight::open(service.home.store().connection())?
    {
        let record = wait.chat.as_ref().and_then(|chat| {
            kept.iter().find(|r| r.id == chat.hamlet)
        });
        let current = match &wait.chat {
            Some(chat) => crate::chats::records::current_number(
                service.home.store().connection(),
                &crate::record::AgentRef {
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
            key: service.credentials.configured(&e.name),
        })
        .collect();
    Ok(State {
        epoch,
        revision,
        chief: service.chief.clone(),
        sandbox: service.chats.sandbox.name().into(),
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
                move_: service.moving.view(),
            },
        },
    })
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
        if before.agents.iter().find(|a| a.ref_ == agent.ref_)
            != Some(agent)
        {
            changes.push(Update::Agent {
                agent: agent.clone(),
            });
        }
    }
    for agent in &before.agents {
        if !after.agents.iter().any(|a| a.ref_ == agent.ref_) {
            changes.push(Update::AgentGone {
                ref_: agent.ref_.clone(),
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
