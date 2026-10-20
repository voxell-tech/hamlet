//! The home-wide merge and split lane, separate from client dispatch.

use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::task::JoinSet;

use super::{CAP, Plan, preparing};
use crate::OperationId;
use crate::harness::Harnesses;
use crate::papers::files::Files;
use crate::papers::tree;
use crate::protocol::{Refusal, reply};
use crate::record::{self, Actor, AgentRef};
use crate::service::Service;
use crate::society::{self, Caller, Deed};

#[derive(Clone)]
enum Origin {
    Call(OperationId),
    Chief { owner: u64, id: String },
}

impl Origin {
    fn key(&self) -> (String, String) {
        match self {
            Self::Call(id) => {
                (id.runtime.as_str().into(), id.key.clone())
            }
            Self::Chief { owner, id } => {
                (format!("client:{owner}"), id.clone())
            }
        }
    }
}

enum Action {
    Merge {
        actor: Actor,
        args: Value,
        task: Option<String>,
    },
    Restore {
        deed: String,
        rest: String,
    },
}

struct Request {
    origin: Origin,
    hamlet: String,
    action: Action,
}

struct Active {
    origin: Origin,
    held: Vec<AgentRef>,
    applying: bool,
}

#[derive(Default)]
pub(crate) struct Merges {
    pub tasks: JoinSet<Completed>,
    pub finished: Vec<u64>,
    queue: VecDeque<Request>,
    active: Option<Active>,
    clients: HashSet<(u64, String)>,
    ready:
        HashMap<(String, String), std::result::Result<Plan, String>>,
}

pub(crate) struct Completed {
    origin: Origin,
    result: Finished,
}

enum Finished {
    Prepared(std::result::Result<Plan, String>),
    Applied {
        work: Applied,
        answer: Deed,
        result: Result<()>,
    },
}

struct Applying {
    hamlet: String,
    entry: record::Archived,
    snapshot: String,
    root: std::fs::File,
    house: Files,
    notes: crate::workspace::Remote,
}

struct Applied {
    hamlet: String,
    entry: record::Archived,
    snapshot: String,
}

impl Merges {
    pub fn held(&self, agent: &AgentRef) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.held.contains(agent))
    }

    pub fn busy(&self, client: u64) -> bool {
        self.clients.iter().any(|(owner, _)| *owner == client)
    }

    pub fn gone(&mut self, client: u64) {
        self.clients.retain(|(owner, _)| *owner != client);
    }

    pub(super) fn take(
        &mut self,
        id: &OperationId,
    ) -> Option<std::result::Result<Plan, String>> {
        self.ready
            .remove(&(id.runtime.as_str().into(), id.key.clone()))
    }

    pub(super) fn owns(&self, id: &OperationId) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active.origin.key()
                == (id.runtime.as_str().into(), id.key.clone())
        })
    }

    fn release(&mut self) {
        self.active = None;
    }
}

pub(super) fn offer(
    service: &mut Service,
    caller: &Caller<'_>,
    args: &Value,
) -> Result<()> {
    let origin = Origin::Call(caller.id.clone());
    if service.house_work.pending.insert(origin.key()) {
        let actor = society::actor(
            service.home.store().connection(),
            &caller.place.agent,
        )?;
        service.merges.queue.push_back(Request {
            origin,
            hamlet: caller.place.agent.hamlet.clone(),
            action: Action::Merge {
                actor,
                args: args.clone(),
                task: caller.task.clone(),
            },
        });
    }
    Ok(())
}

pub(crate) fn chief(
    service: &mut Service,
    owner: u64,
    id: &str,
    hamlet: &str,
    deed: &str,
    rest: &str,
) -> Result<()> {
    let action = if deed == "merge" {
        let words = rest.split_whitespace().collect::<Vec<_>>();
        if words.len() < 3 || words[1] != "into" {
            service.chats.outbox.push((
                owner,
                reply(
                    id,
                    Err(Refusal::Invalid(
                        "Use /merge <absorbed> into <kept> [reason]."
                            .into(),
                    )),
                ),
            ));
            return Ok(());
        }
        Action::Merge {
            actor: Actor::Chief(service.chief.clone()),
            args: json!({"absorb":words[0],"keep":words[2],"reason":words[3..].join(" ")}),
            task: None,
        }
    } else {
        Action::Restore {
            deed: deed.into(),
            rest: rest.into(),
        }
    };
    service.merges.clients.insert((owner, id.into()));
    service.merges.queue.push_back(Request {
        origin: Origin::Chief {
            owner,
            id: id.into(),
        },
        hamlet: hamlet.into(),
        action,
    });
    Ok(())
}

pub(crate) async fn advance(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    if let Some(active) = &service.merges.active {
        if let Origin::Call(id) = &active.origin
            && !active.applying
            && !service
                .house_work
                .pending
                .contains(&active.origin.key())
            && !society::library::active(
                service.home.store().connection(),
                id.runtime.as_str(),
                &id.key,
            )?
        {
            if let Some(Ok(plan)) =
                service.merges.ready.remove(&active.origin.key())
            {
                discard(service, &plan)?;
            }
            service.merges.release();
        } else {
            return Ok(());
        }
    }
    let Some(request) = service.merges.queue.pop_front() else {
        return Ok(());
    };
    let allowed = match &request.origin {
        Origin::Call(id) => society::library::active(
            service.home.store().connection(),
            id.runtime.as_str(),
            &id.key,
        )?,
        Origin::Chief { owner, id } => {
            service.merges.clients.contains(&(*owner, id.clone()))
        }
    };
    if !allowed {
        service.house_work.pending.remove(&request.origin.key());
        return Ok(());
    }
    service.merges.active = Some(Active {
        origin: request.origin.clone(),
        held: Vec::new(),
        applying: false,
    });
    match request.action {
        Action::Merge { actor, args, task } => {
            if let Origin::Call(id) = &request.origin {
                let key = serde_json::to_string(&(
                    id.runtime.as_str(),
                    &id.key,
                ))?;
                if let Some(plan) = society::document::<Option<Plan>>(
                    service.home.store().connection(),
                    "merge-plan",
                    &key,
                )? {
                    let held =
                        vec![plan.kept.clone(), plan.gone.clone()];
                    service
                        .merges
                        .active
                        .as_mut()
                        .expect("merge lane")
                        .held
                        .clone_from(&held);
                    for agent in &held {
                        crate::chats::stop_agent(
                            service, harnesses, agent,
                        )
                        .await?;
                    }
                    return complete(
                        service,
                        harnesses,
                        Completed {
                            origin: request.origin,
                            result: Finished::Prepared(Ok(plan)),
                        },
                    )
                    .await;
                }
            }
            let prepared = preparing(
                service,
                &request.hamlet,
                actor,
                &args,
                task,
            )?;
            match prepared {
                Err(error) => {
                    complete(
                        service,
                        harnesses,
                        Completed {
                            origin: request.origin,
                            result: Finished::Prepared(Err(error)),
                        },
                    )
                    .await?
                }
                Ok(prepared) => {
                    let held =
                        [prepared.kept.number, prepared.gone.number]
                            .into_iter()
                            .map(|number| AgentRef {
                                hamlet: request.hamlet.clone(),
                                number,
                            })
                            .collect::<Vec<_>>();
                    service
                        .merges
                        .active
                        .as_mut()
                        .expect("merge lane")
                        .held
                        .clone_from(&held);
                    for agent in &held {
                        crate::chats::stop_agent(
                            service, harnesses, agent,
                        )
                        .await?;
                    }
                    service.merges.tasks.spawn(async move {
                        let result =
                            prepared.run().await.unwrap_or_else(
                                |error| Err(error.to_string()),
                            );
                        Completed {
                            origin: request.origin,
                            result: Finished::Prepared(result),
                        }
                    });
                }
            }
        }
        Action::Restore { deed, rest } => {
            let record =
                service.home.store().record(&request.hamlet)?;
            let which =
                rest.split_whitespace().next().unwrap_or_default();
            let entry = record
                .as_ref()
                .and_then(|r| {
                    r.archive.iter().rev().find(|entry| {
                        entry.back.is_none()
                            && (deed == "undo"
                                || entry.id == which
                                || entry.agent.name == which)
                    })
                })
                .cloned();
            if let Some(entry) = &entry
                && society::document::<Option<bool>>(
                    service.home.store().connection(),
                    "remote-merge-attempt",
                    &format!("{}:{}", request.hamlet, entry.id),
                )? == Some(true)
            {
                return respond(
                    service,
                    harnesses,
                    request.origin,
                    Err(Refusal::Invalid(
                        "The previous remote merge or split has \
                         unknown effects. Its record and recovery \
                         plan remain. This server cannot resume or \
                         split an unconfirmed batch; inspect both \
                         houses and the preserved plan."
                            .into(),
                    )),
                )
                .await;
            }
            if let Some(entry) = &entry
                && let Some(merge) = &entry.merge
            {
                let held = [entry.agent.number, merge.kept.number]
                    .into_iter()
                    .map(|number| AgentRef {
                        hamlet: request.hamlet.clone(),
                        number,
                    })
                    .collect::<Vec<_>>();
                service
                    .merges
                    .active
                    .as_mut()
                    .expect("split lane")
                    .held
                    .clone_from(&held);
                for agent in &held {
                    crate::chats::stop_agent(
                        service, harnesses, agent,
                    )
                    .await?;
                }
            }
            let answer = society::leaving::deed(
                service,
                harnesses,
                &request.hamlet,
                &deed,
                &rest,
            )
            .await?
            .expect("restore deed");
            if let Ok(text) = &answer
                && let Some(entry) = entry
                && start_apply(
                    service,
                    request.origin.clone(),
                    &request.hamlet,
                    &entry.id,
                    text.clone(),
                )?
            {
                return Ok(());
            }
            respond(service, harnesses, request.origin, answer)
                .await?;
        }
    }
    Ok(())
}

fn discard(service: &Service, plan: &Plan) -> Result<()> {
    tree::erase(
        &tree::directory(
            service.houses.root(),
            ".society-files",
            false,
        )?,
        &plan.snapshot,
    )
}

pub(super) fn release(service: &mut Service, id: &OperationId) {
    service
        .house_work
        .pending
        .remove(&(id.runtime.as_str().into(), id.key.clone()));
    service.merges.release();
}

pub(super) fn apply_call(
    service: &mut Service,
    id: &OperationId,
    hamlet: &str,
    archive: &str,
) -> Result<bool> {
    start_apply(
        service,
        Origin::Call(id.clone()),
        hamlet,
        archive,
        String::new(),
    )
}

fn start_apply(
    service: &mut Service,
    origin: Origin,
    hamlet: &str,
    archive: &str,
    answer: String,
) -> Result<bool> {
    let record = service
        .home
        .store()
        .record(hamlet)?
        .context("merge record")?;
    let Some(entry) = record
        .archive
        .iter()
        .find(|entry| {
            entry.id == archive
                && entry
                    .merge
                    .as_ref()
                    .is_some_and(|merge| !merge.applied)
        })
        .cloned()
    else {
        return Ok(false);
    };
    let plan = society::document::<Option<Plan>>(
        service.home.store().connection(),
        "merge",
        &format!("{hamlet}:{archive}"),
    )?
    .context("merge plan")?;
    let number = entry.merge.as_ref().expect("merge").kept.number;
    let work = Applying {
        hamlet: hamlet.into(),
        entry,
        snapshot: plan.snapshot,
        root: service.houses.root().try_clone()?,
        house: service.houses.files(hamlet, number)?,
        notes: service
            .houses
            .remote(hamlet, number, true)?
            .expect("remote merge"),
    };
    if let Origin::Call(_) = &origin {
        service.house_work.pending.insert(origin.key());
    }
    society::keep(
        service.home.store().connection(),
        "remote-merge-attempt",
        &format!("{hamlet}:{archive}"),
        &true,
    )?;
    service.merges.active.as_mut().expect("merge lane").applying =
        true;
    service.merges.tasks.spawn(async move {
        let result = work.run().await;
        Completed {
            origin,
            result: Finished::Applied {
                work: Applied {
                    hamlet: work.hamlet,
                    entry: work.entry,
                    snapshot: work.snapshot,
                },
                answer: Ok(answer),
                result,
            },
        }
    });
    Ok(true)
}

impl Applying {
    async fn notes(&self) -> Result<String> {
        let bytes =
            self.notes.read("/house/NOTES.md", true).await.map_err(
                |error| anyhow::anyhow!("{}", error.message),
            )?;
        Ok(String::from_utf8(bytes)?)
    }

    async fn run(&self) -> Result<()> {
        let merge =
            self.entry.merge.as_ref().expect("merge manifest");
        crate::fault::point("after_merge_record");
        if self.entry.back.is_none() {
            let snapshots =
                tree::directory(&self.root, ".society-files", false)?;
            let files = tree::read(
                &tree::directory(&snapshots, &self.snapshot, false)?,
                CAP * 2,
                4096,
            )?;
            for (path, file) in &files {
                self.house.put(path, file).await?;
                crate::fault::point("during_merge_files");
            }
            let current = self.notes().await?;
            if current == merge.files.notes_before {
                self.house
                    .replace(
                        "NOTES.md",
                        merge.files.notes_after.as_bytes(),
                    )
                    .await?;
                crate::fault::point("after_merge_notes");
            } else {
                anyhow::ensure!(
                    current == merge.files.notes_after,
                    "The kept notes changed before the merge \
                     finished; inspect both houses."
                );
            }
        } else {
            let present = self.house.read(CAP, 4096).await?;
            for file in &merge.files.brought {
                if present.get(&file.path).is_some_and(|entry| {
                    format!("{:x}", Sha256::digest(&entry.bytes))
                        == file.sha256
                }) {
                    self.house.remove(&file.path, false).await?;
                }
            }
            if self.notes().await? == merge.files.notes_after {
                self.house
                    .replace(
                        "NOTES.md",
                        merge.files.notes_before.as_bytes(),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

impl Applied {
    fn keep(self, service: &mut Service) -> Result<()> {
        let mut record = service
            .home
            .store()
            .record(&self.hamlet)?
            .context("merge record")?;
        let entry = record
            .archive
            .iter_mut()
            .find(|entry| entry.id == self.entry.id)
            .context("merge archive")?;
        anyhow::ensure!(
            entry.back == self.entry.back
                && entry.merge == self.entry.merge,
            "The merge changed while its files were being applied; \
             inspect both houses."
        );
        entry.merge.as_mut().expect("merge manifest").applied = true;
        record::write(service.home.store().connection(), &record)?;
        service.home.store().connection().execute(
            "DELETE FROM society_document WHERE \
             kind='remote-merge-attempt' AND key=?1",
            [format!("{}:{}", self.hamlet, self.entry.id)],
        )?;
        tree::erase(
            &tree::directory(
                service.houses.root(),
                ".society-files",
                false,
            )?,
            &self.snapshot,
        )
    }
}

pub(crate) async fn complete(
    service: &mut Service,
    harnesses: &mut Harnesses,
    completed: Completed,
) -> Result<()> {
    match completed.result {
        Finished::Prepared(result) => match &completed.origin {
            Origin::Call(id) => {
                service
                    .house_work
                    .pending
                    .remove(&completed.origin.key());
                if society::library::active(
                    service.home.store().connection(),
                    id.runtime.as_str(),
                    &id.key,
                )? {
                    service
                        .merges
                        .ready
                        .insert(completed.origin.key(), result);
                    harnesses.wake(&id.runtime, &id.key).await;
                } else {
                    if let Ok(plan) = result {
                        discard(service, &plan)?;
                    }
                    service.merges.release();
                }
            }
            Origin::Chief { owner, id } => {
                if !service
                    .merges
                    .clients
                    .contains(&(*owner, id.clone()))
                {
                    if let Ok(plan) = result {
                        discard(service, &plan)?;
                    }
                    service.merges.release();
                    return Ok(());
                }
                let plan = match result {
                    Ok(plan) => plan,
                    Err(error) => {
                        return respond(
                            service,
                            harnesses,
                            completed.origin,
                            Err(Refusal::Invalid(error)),
                        )
                        .await;
                    }
                };
                let result = super::commit(
                    service,
                    &OperationId {
                        runtime: crate::RuntimeInstance::new(
                            "chief",
                        )?,
                        key: uuid::Uuid::new_v4().to_string(),
                    },
                    &plan,
                )?;
                let answer = if result["error"] == true {
                    Err(Refusal::Invalid(
                        result["text"]
                            .as_str()
                            .unwrap_or_default()
                            .into(),
                    ))
                } else {
                    Ok(result["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned())
                };
                if let Ok(text) = &answer
                    && start_apply(
                        service,
                        completed.origin.clone(),
                        &plan.kept.hamlet,
                        result["archive"]
                            .as_str()
                            .context("merge archive")?,
                        text.clone(),
                    )?
                {
                    return Ok(());
                }
                discard(service, &plan)?;
                respond(service, harnesses, completed.origin, answer)
                    .await?;
            }
        },
        Finished::Applied {
            work,
            answer,
            result,
        } => {
            let key = format!("{}:{}", work.hamlet, work.entry.id);
            let result = result.and_then(|()| work.keep(service));
            let answer = match result {
                Ok(()) => answer,
                Err(error) => {
                    let uncertain = error
                        .downcast_ref::<crate::workspace::Failure>()
                        .is_none_or(|failure| failure.uncertain);
                    society::keep(
                        service.home.store().connection(),
                        "remote-merge-attempt",
                        &key,
                        &uncertain,
                    )?;
                    let status = if uncertain {
                        "has unknown effects"
                    } else {
                        "was refused"
                    };
                    let recovery = if uncertain {
                        "This server cannot resume or split an \
                         unconfirmed batch. Inspect both houses and \
                         the preserved plan."
                    } else {
                        "Inspect both houses before explicitly \
                         restoring the archived agent."
                    };
                    Err(Refusal::Invalid(format!(
                        "The remote merge or split {status}: \
                         {error}. The record and recovery plan \
                         remain. Earlier files may already have \
                         changed. No automatic retry will be made. \
                         {recovery}"
                    )))
                }
            };
            respond(service, harnesses, completed.origin, answer)
                .await?;
        }
    }
    Ok(())
}

async fn respond(
    service: &mut Service,
    harnesses: &mut Harnesses,
    origin: Origin,
    answer: Deed,
) -> Result<()> {
    service.house_work.pending.remove(&origin.key());
    service.merges.release();
    match origin {
        Origin::Call(id) => {
            if let Err(error) = answer {
                let mut outcome =
                    society::outcome(&error.text(), true);
                service.credentials.scrub(&mut outcome);
                service.home.store().connection().execute(
                    "UPDATE marker SET outcome=?3 WHERE runtime=?1 \
                     AND key=?2 AND kind='tool'",
                    rusqlite::params![
                        id.runtime.as_str(),
                        id.key,
                        outcome.to_string()
                    ],
                )?;
            }
            harnesses.wake(&id.runtime, &id.key).await;
        }
        Origin::Chief { owner, id } => {
            if service.merges.clients.remove(&(owner, id.clone())) {
                service.merges.finished.push(owner);
                service
                    .chats
                    .outbox
                    .push((owner, reply(&id, answer)));
            }
        }
    }
    Ok(())
}
