//! Protocol views of the record and the chief's settings.

use anyhow::Result;
use hamlet_protocol::v0::{Changes, ServerMessage, State, Update};
use serde_json::{Value, json};

use crate::documents::mode_words;
use crate::record::{self, End};
use crate::service::{Service, records};

pub(crate) fn state(
    service: &mut Service,
    epoch: i64,
    revision: i64,
) -> Result<State> {
    let settings = service.settings()?;
    let records = records(service.home.store().connection())?;
    let mut hamlets = Vec::new();
    let mut agents = Vec::new();
    let mut grants = Vec::new();
    for record in records {
        let limits = service.limits_of(&record.id)?;
        let work = service.work(&record.id)?;
        let reference =
            |number| json!({"hamlet":record.id,"number":number});
        let end = |end| {
            reference(match end {
                End::Office => record.mayor,
                End::Agent(number) => number,
            })
        };
        let roads = record.roads_of().iter().map(|r| json!({
            "a":end(r.a), "b":end(r.b), "messages":r.messages,
            "lastFrom":r.last_from.map(end), "level":record::level(r, chrono::Utc::now().timestamp_millis(), limits["road"]["growsOverDays"].as_u64().unwrap_or(7))
        })).collect::<Vec<_>>();
        let period =
            limits["budget"]["period"].as_str().unwrap_or("day");
        let books = crate::spend::books(
            service.home.store().connection(),
            &record.id,
            period,
            None,
            chrono::Utc::now().timestamp_millis(),
        )?;
        hamlets.push(json!({"id":record.id, "name":record.id, "at":null, "roads":roads, "votes":[],
            "folder":work["folder"], "before":work["before"], "mode":work["mode"], "modeWords":mode_words(work["mode"].as_str().unwrap_or("manual")), "limits":limits,
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
            let mut view = json!({"ref":reference(agent.number), "name":agent.name,"purpose":agent.purpose,"since":agent.since,"mayor":mayor,
                "model":model(agent,&limits,&settings,mayor),"busy":false,"faulted":false,"activity":{"doing":"rest","doingWords":"Resting","step":0,"failures":0},
                "saying":{"text":"","thought":false},"spent":{"tokens":0,"micros":0,"ms":0,"cost":"$0.00"},"skills":[],"consulting":false,"conversations":[]});
            let spent = books
                .agents
                .get(&agent.number)
                .copied()
                .unwrap_or_default();
            view["spent"] = serde_json::to_value(spent)?;
            view["spent"]["cost"] =
                crate::spend::money(spent.micros).into();
            if let Some(left) = left {
                view["left"] = json!({"how":if left.merge.is_some(){"merged"}else{"removed"}, "into":left.merge.as_ref().map(|m|reference(m.kept.number)), "at":left.at,"by":by_words(&left.by),"reason":left.reason,"words":record.farewell(left)});
            }
            agents.push(view);
        }
        for permission in ["network", "unconfined"] {
            for name in limits["permissions"][permission]
                .as_array()
                .into_iter()
                .flatten()
            {
                let agent = record.agents.iter().find(|a| {
                    if name == "mayor" {
                        a.number == record.mayor
                    } else {
                        name == &a.name
                    }
                });
                grants.push(json!({"agent":agent.map(|a|reference(a.number)),"name":name,"permission":permission,"by":null,"source":"limits"}));
            }
        }
    }
    let mut view = json!({"epoch":epoch,"revision":revision,"chief":service.chief,"sandbox":"unavailable", "houses":service.home.layout().root().join("houses"),
        "hamlets":hamlets,"agents":agents,"grants":grants,"waits":[],"providers":service.provider_views()?,"library":[],"closed":[],"starters":[],"deeds":["folder","mode","queue"],
        "settings":{"model":settings["model"],"reflectEvery":settings["reflectEvery"],"allowNet":settings["allowNet"],"unset":settings["saved"] != true,"link":settings["link"],
            "endpoints":settings["endpoints"].as_array().into_iter().flatten().map(|e|json!({"name":e["name"],"baseUrl":e["baseUrl"],"key":service.credentials.configured(e["name"].as_str().unwrap_or_default())})).collect::<Vec<_>>(),
            "workspace":{"url":service.credentials.workspace_url(),"running":null}}});
    service.credentials.scrub(&mut view);
    Ok(serde_json::from_value(view)?)
}

pub(crate) fn model(
    agent: &record::Agent,
    limits: &Value,
    settings: &Value,
    mayor: bool,
) -> String {
    let name = if mayor { "mayor" } else { &agent.name };
    limits["pinned"][name]
        .as_str()
        .or_else(|| {
            agent.model.as_deref().filter(|model| {
                limits["models"].as_array().is_some_and(|models| {
                    models.iter().any(|m| m == model)
                })
            })
        })
        .or_else(|| limits["model"].as_str())
        .or_else(|| settings["model"].as_str())
        .unwrap_or_default()
        .into()
}

fn by_words(by: &str) -> String {
    if by == "mayor" {
        return "the mayor".into();
    }
    by.strip_prefix("chief:").map_or_else(
        || by.into(),
        |chief| format!("the chief {chief}"),
    )
}

pub(crate) fn changed(
    before: &State,
    after: &mut State,
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
    if before.settings != after.settings {
        changes.push(Update::Settings {
            settings: after.settings.clone(),
        });
    }
    if before.grants != after.grants {
        changes.push(Update::Grants {
            grants: after.grants.clone(),
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
        .any(|h| !after.hamlets.iter().any(|n| n.id == h.id));
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
