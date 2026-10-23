//! The home-wide merge and split lane, separate from client dispatch.
//! Every merge and split runs here, its file work in the background
//! on the houses' backend.

use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{Context, Result};
use tokio::task::JoinSet;

use super::{
    Applied, Applying, Merge, Plan, Planned, Unplanned, preparing,
};
use crate::OperationId;
use crate::harness::Harnesses;
use crate::papers::tree;
use crate::protocol::{Refusal, reply};
use crate::record::{Actor, AgentRef};
use crate::runtime::Asked;
use crate::service::Service;
use crate::society::{self, Caller, Deed};

#[derive(Clone)]
enum Origin {
    Call(OperationId),
    Chief { owner: u64, id: String },
}

impl Origin {
    /// The call that waits for the lane, when a call asked.
    fn call(&self) -> Option<&OperationId> {
        match self {
            Self::Call(id) => Some(id),
            Self::Chief { .. } => None,
        }
    }

    /// Lets the call that asked, if one did, stop waiting.
    fn release(&self, service: &mut Service) {
        if let Some(id) = self.call() {
            service.house_work.release(id);
        }
    }
}

enum Action {
    Merge {
        actor: Actor,
        args: Merge,
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
    ready: HashMap<OperationId, Planned>,
}

pub(crate) struct Completed {
    origin: Origin,
    result: Finished,
}

enum Finished {
    Prepared(Planned),
    Applied {
        work: Applied,
        answer: Deed,
        result: Result<()>,
    },
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
    ) -> Option<Planned> {
        self.ready.remove(id)
    }

    pub(super) fn owns(&self, id: &OperationId) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.origin.call() == Some(id))
    }

    /// The request the lane works on: set when a request begins and
    /// held until it is released, once it is answered or the call or
    /// chief that asked is gone. So each step of a begun request
    /// finds it.
    fn active_mut(&mut self) -> &mut Active {
        self.active.as_mut().expect(
            "a begun request holds the lane until its release",
        )
    }

    fn release(&mut self) {
        self.active = None;
    }
}

pub(super) fn offer(
    service: &mut Service,
    caller: &Caller<'_>,
    args: Merge,
) -> Result<()> {
    let origin = Origin::Call(caller.id.clone());
    if service.house_work.hold(caller.id.clone()) {
        let actor =
            society::actor(service.db(), &caller.place.agent)?;
        service.merges.queue.push_back(Request {
            origin,
            hamlet: caller.place.agent.hamlet.clone(),
            action: Action::Merge {
                actor,
                args,
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
            args: Merge {
                absorb: words[0].into(),
                keep: words[2].into(),
                purpose: None,
                reason: match words[3..].join(" ") {
                    reason if reason.is_empty() => {
                        "a chief asked".into()
                    }
                    reason => reason,
                },
            },
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

/// Releases the active request once its call ended, then begins the
/// next. False when it did neither.
pub(crate) async fn advance(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<bool> {
    let released = service.merges.active.is_some();
    if let Some(active) = &service.merges.active {
        if let Origin::Call(id) = &active.origin
            && !active.applying
            && !service.house_work.pending(id)
            && !crate::oversight::held::unfinished(
                service.db(),
                id.runtime.as_str(),
                &id.key,
            )?
        {
            if let Some(Ok(plan)) = service.merges.ready.remove(id) {
                discard(service, &plan)?;
            }
            service.merges.release();
        } else {
            return Ok(false);
        }
    }
    let Some(request) = service.merges.queue.pop_front() else {
        return Ok(released);
    };
    begin(service, harnesses, request).await?;
    Ok(true)
}

async fn begin(
    service: &mut Service,
    harnesses: &mut Harnesses,
    request: Request,
) -> Result<()> {
    let allowed = match &request.origin {
        Origin::Call(id) => crate::oversight::held::unfinished(
            service.db(),
            id.runtime.as_str(),
            &id.key,
        )?,
        Origin::Chief { owner, id } => {
            service.merges.clients.contains(&(*owner, id.clone()))
        }
    };
    if !allowed {
        request.origin.release(service);
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
                    service.db(),
                    "merge-plan",
                    &key,
                )? {
                    let held =
                        vec![plan.kept.clone(), plan.gone.clone()];
                    service
                        .merges
                        .active_mut()
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
                    .await?;
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
                        .active_mut()
                        .held
                        .clone_from(&held);
                    for agent in &held {
                        crate::chats::stop_agent(
                            service, harnesses, agent,
                        )
                        .await?;
                    }
                    service.merges.tasks.spawn(async move {
                        let result = prepared
                            .run()
                            .await
                            .unwrap_or_else(|error| {
                                Err(Unplanned::Failed(
                                    error.to_string(),
                                ))
                            });
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
                    service.db(),
                    "remote-merge-attempt",
                    &format!("{}:{}", request.hamlet, entry.id),
                )? == Some(true)
            {
                return respond(
                    service,
                    harnesses,
                    request.origin,
                    Err(Refusal::Invalid(
                        "The previous merge or split has unknown \
                         effects. Its record and recovery plan \
                         remain. This server cannot resume or split \
                         an unconfirmed batch; inspect both houses \
                         and the preserved plan."
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
                service.merges.active_mut().held.clone_from(&held);
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
    tree::erase_snapshot(service.houses.root(), &plan.snapshot)
}

pub(super) fn release(service: &mut Service, id: &OperationId) {
    service.house_work.release(id);
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
        service.db(),
        "merge",
        &format!("{hamlet}:{archive}"),
    )?
    .context("merge plan")?;
    let work = Applying::new(service, hamlet, entry, plan.snapshot)?;
    if let Some(id) = origin.call() {
        service.house_work.hold(id.clone());
    }
    if !service.houses.confirms() {
        society::keep(
            service.db(),
            "remote-merge-attempt",
            &format!("{hamlet}:{archive}"),
            &true,
        )?;
    }
    service.merges.active_mut().applying = true;
    service.merges.tasks.spawn(async move {
        let result = work.run().await;
        Completed {
            origin,
            result: Finished::Applied {
                work: work.done(),
                answer: Ok(answer),
                result,
            },
        }
    });
    Ok(true)
}

/// Takes in the lane's finished background work. When that fails
/// for a chief, it lets the lane and the chief's request go, and
/// names the request so the serve loop faults it and frees the
/// connection: no later step would answer it.
pub(crate) async fn complete(
    service: &mut Service,
    harnesses: &mut Harnesses,
    completed: Completed,
) -> Result<()> {
    let origin = completed.origin.clone();
    let done = finish(service, harnesses, completed).await;
    match (done, origin) {
        (Err(error), Origin::Chief { owner, id }) => {
            service.merges.release();
            service.merges.clients.remove(&(owner, id.clone()));
            Err(error.context(Asked {
                client: owner,
                request: Some(id),
            }))
        }
        (done, _) => done,
    }
}

async fn finish(
    service: &mut Service,
    harnesses: &mut Harnesses,
    completed: Completed,
) -> Result<()> {
    match completed.result {
        Finished::Prepared(result) => match &completed.origin {
            Origin::Call(id) => {
                service.house_work.release(id);
                if crate::oversight::held::unfinished(
                    service.db(),
                    id.runtime.as_str(),
                    &id.key,
                )? {
                    service.merges.ready.insert(id.clone(), result);
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
                            Err(Refusal::Invalid(error.text())),
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
                    &crate::operation::Request::new("merge", &plan)?,
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
            let key = work.key();
            let result = result.and_then(|()| work.applied(service));
            let answer = match result {
                Ok(()) => answer,
                Err(error) => {
                    // Only a backend that cannot confirm its changes
                    // leaves them unknown, unless it refused.
                    let uncertain = !service.houses.confirms()
                        && error
                            .downcast_ref::<crate::workspace::Failure>()
                            .is_none_or(|failure| failure.uncertain);
                    if !service.houses.confirms() {
                        society::keep(
                            service.db(),
                            "remote-merge-attempt",
                            &key,
                            &uncertain,
                        )?;
                    }
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
                        "The merge or split {status}: {error}. The \
                         record and recovery plan remain. Earlier \
                         files may already have changed. No \
                         automatic retry will be made. {recovery}"
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
    origin.release(service);
    service.merges.release();
    match origin {
        Origin::Call(id) => {
            if let Err(error) = answer {
                let mut outcome =
                    society::outcome(&error.text(), true);
                service.credentials.scrub(&mut outcome);
                crate::operation::settle(
                    service.db(),
                    &id,
                    &outcome,
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

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::record::{Change, RecordOperation};
    use crate::{Home, Overrides};

    #[tokio::test]
    async fn a_chief_merge_that_fails_to_complete_frees_the_lane() {
        let scratch = Scratch::new(&format!(
            "failed-merge-{}",
            uuid::Uuid::new_v4()
        ));
        let mut service = Service::new(
            Home::open(&scratch).unwrap(),
            Overrides::none(true),
        )
        .unwrap();
        let layout = service.home.layout().clone();
        let (mut harnesses, _) = Harnesses::start(
            &layout,
            service.home.store(),
            crate::testing::config(Overrides::none(true)),
        )
        .unwrap();
        let origin = Origin::Chief {
            owner: 7,
            id: "merge".into(),
        };
        service.merges.clients.insert((7, "merge".into()));
        service.merges.active = Some(Active {
            origin: origin.clone(),
            held: Vec::new(),
            applying: false,
        });
        let limits = service
            .home
            .store()
            .record("home")
            .unwrap()
            .unwrap()
            .limits;
        let mayor = AgentRef {
            hamlet: "home".into(),
            number: 0,
        };
        // A plan whose change is no departure, so its commit fails.
        let plan = Plan {
            operation: RecordOperation {
                hamlet: "home".into(),
                actor: Actor::Chief(service.chief.clone()),
                at: 0,
                reason: "a test".into(),
                task: None,
                change: Change::SetLimits {
                    limits,
                    runs: Vec::new(),
                },
            },
            snapshot: "none".into(),
            gone: mayor.clone(),
            kept: mayor,
            prompt: None,
        };
        let error = complete(
            &mut service,
            &mut harnesses,
            Completed {
                origin,
                result: Finished::Prepared(Ok(plan)),
            },
        )
        .await
        .unwrap_err();
        let asked = error.downcast_ref::<Asked>().unwrap();
        assert_eq!(
            (asked.client, asked.request.as_deref()),
            (7, Some("merge"))
        );
        assert!(service.merges.active.is_none());
        assert!(!service.merges.busy(7));
    }
}
