//! Protocol views of the record and the chief's settings.

use anyhow::Result;
use hamlet_protocol::v0::{
    Changes, Doing, ServerMessage, State, Update,
};
use serde_json::json;

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
    let closed = realm.hamlets.iter().filter(|h| !h.erased).filter_map(|h| h.removed.as_ref().map(|removed| json!({"id":h.id,"name":h.title.as_ref().unwrap_or(&h.id),"at":removed.at,"by":removed.by,"reason":removed.reason}))).collect::<Vec<_>>();
    let mut hamlets = Vec::new();
    let mut agents = Vec::new();
    let held =
        crate::gate::grants(service.home.store().connection())?;
    let mut grants = held
        .iter()
        .map(|grant| {
            json!({"agent":{"hamlet":grant.agent.hamlet,"number":grant.agent.number},"name":grant.name,"permission":grant.permission,"by":grant.by,"source":"grant"})
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
        let reference =
            |number| json!({"hamlet":record.id,"number":number});
        let end = |end| {
            reference(match end {
                End::Office => record.mayor,
                End::Agent(number) => number,
            })
        };
        let mut roads = record.roads_of().iter().map(|r| json!({
            "a":end(r.a), "b":end(r.b), "messages":r.messages,
            "lastFrom":r.last_from.map(end), "level":record::level(r.last_used, r.messages, now, grows)
        })).collect::<Vec<_>>();
        roads.extend(crate::realm::roads(service.home.store().connection())?.into_iter().filter(|r| r.a.hamlet == record.id || r.b.hamlet == record.id).map(|r| {
            let level = record::level(r.last_used, r.messages, now, grows);
            json!({"a":r.a,"b":r.b,"messages":r.messages,"lastFrom":r.last_from,"level":level})
        }));
        let period = &limits.budget.period;
        let books = crate::spend::books(
            service.home.store().connection(),
            &record.id,
            period,
            None,
            chrono::Utc::now().timestamp_millis(),
        )?;
        hamlets.push(json!({"id":record.id, "name":place.title.as_ref().unwrap_or(&record.id), "at":place.at, "roads":roads, "votes":crate::society::votes::views(service.home.store().connection(), &record)?,
            "folder":work.folder, "before":work.before, "mode":work.mode, "modeWords":work.mode.words(), "limits":limits,
            "budget":service.budget(&record.id)?}));
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
            let doing = if left.is_some() {
                Doing::Rest
            } else {
                work.doing
            };
            let consulting = crate::society::library::consulting(
                service.home.store().connection(),
                &reference_of,
            )?;
            let mut view = json!({"ref":reference(agent.number), "name":agent.name,"purpose":agent.purpose,"since":agent.since,"mayor":mayor,
                "model":limits.model_of(agent, mayor, &settings.model),"busy":busy,"faulted":!busy && work.faulted,
                "activity":{"doing":doing,"doingWords":crate::chats::words(&doing),"step":if left.is_some() {0} else {work.step},"failures":if left.is_some() {0} else {work.failures}},
                "saying":{"text":work.saying,"thought":work.thought},"spent":{"tokens":0,"micros":0,"ms":0,"cost":"$0.00"},"skills":[],"consulting":consulting,"conversations":[]});
            let spent = books
                .agents
                .get(&agent.number)
                .copied()
                .unwrap_or_default();
            view["spent"] = serde_json::to_value(spent)?;
            view["spent"]["cost"] =
                crate::spend::money(spent.micros).into();
            if left.is_none() {
                let house = root
                    .join("houses")
                    .join(&record.id)
                    .join(agent.number.to_string());
                view["skills"] = service
                    .houses
                    .skill_names(&record.id, agent.number, &house)?
                    .into();
            }
            view["conversations"] = crate::society::letters::views(
                service.home.store().connection(),
                &reference_of,
            )?
            .into();
            if let Some(left) = left {
                view["left"] = json!({"how":if left.merge.is_some(){"merged"}else{"removed"}, "into":left.merge.as_ref().map(|m|reference(m.kept.number)), "at":left.at,"by":left.by.words(),"reason":left.reason,"words":record.farewell(left)});
            }
            agents.push(view);
        }
        kept.push(record.clone());
        for (permission, names) in [
            ("network", &limits.permissions.network),
            ("unconfined", &limits.permissions.unconfined),
        ] {
            for name in names {
                let agent = record.agents.iter().find(|a| {
                    if name == hamlet_protocol::MAYOR {
                        a.number == record.mayor
                    } else {
                        name == &a.name
                    }
                });
                grants.push(json!({"agent":agent.map(|a|reference(a.number)),"name":name,"permission":permission,"by":null,"source":"limits"}));
            }
        }
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
    let mut view = json!({"epoch":epoch,"revision":revision,"chief":service.chief,"sandbox":service.chats.sandbox.name(), "houses":service.home.layout().root().join("houses"),
        "hamlets":hamlets,"agents":agents,"grants":grants,"waits":waits,"providers":service.provider_views()?,"library":crate::society::library::views(service)?,"closed":closed,"starters":crate::gifts::starters(),"deeds":crate::chats::advertised_deeds(),
        "settings":{"model":settings.model,"reflectEvery":settings.reflect_every,"allowNet":settings.allow_net,"unset":!settings.saved,"link":settings.link,
            "endpoints":settings.endpoints.iter().map(|e|json!({"name":e.name,"baseUrl":e.base_url,"key":service.credentials.configured(&e.name)})).collect::<Vec<_>>(),
            "workspace":{"url":service.credentials.workspace_url(),"running":service.houses.workspace().map(|connection| &connection.url)}}});
    let moving = service.moving.view();
    if !moving.is_null() {
        view["settings"]["workspace"]["move"] = moving;
    }
    service.credentials.scrub(&mut view);
    Ok(serde_json::from_value(view)?)
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
