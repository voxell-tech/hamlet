//! Remote gift preparations and their service-owned receipts.

use std::fs::File;
use std::io::Write;

use anyhow::Result;
use hamlet_protocol::v0::{Address, ServerMessage};

use super::{
    MAX_BYTES, Progress, Upload, refused, remote_shelf, tree,
};
use crate::harness::Harnesses;
use crate::papers::copy::Snapshot;
use crate::papers::files::Files;
use crate::protocol::{Refusal, reply};
use crate::record::{AgentRef, Decision, DecisionKind};
use crate::service::Service;
use crate::workspace::Remote;

pub(crate) struct Skill {
    to: Address,
    target: AgentRef,
    from: Option<(Address, AgentRef)>,
    giver: Option<String>,
    name: String,
    remote: Remote,
    root: File,
    source: Option<Files>,
}

pub(crate) enum Completed {
    Began {
        owner: u64,
        id: String,
        token: String,
        result: Result<()>,
    },
    Files {
        owner: u64,
        id: String,
        original: String,
        agent: AgentRef,
        result: Result<Vec<String>>,
    },
    Prepared {
        owner: u64,
        id: String,
        gift: Skill,
        snapshot: Result<Snapshot>,
    },
    Delivered {
        owner: u64,
        id: String,
        gift: Skill,
        result: Result<()>,
    },
}

pub(super) fn begin(
    service: &mut Service,
    upload: Upload,
    remote: Remote,
    roots: Vec<String>,
) {
    let owner = upload.owner;
    let id = upload.id.clone();
    let token = uuid::Uuid::new_v4().to_string();
    service.gifts.pending.insert(token.clone(), upload);
    service.gifts.working.insert((owner, id.clone()));
    service.gifts.tasks.spawn(async move {
        let result = remote_shelf(&remote, "inbox", &roots).await;
        Completed::Began {
            owner,
            id,
            token,
            result,
        }
    });
}

pub(super) fn finish(
    service: &mut Service,
    upload: Upload,
    id: &str,
    remote: Remote,
    roots: Vec<String>,
) {
    let owner = upload.owner;
    let id = id.to_owned();
    service.gifts.working.insert((owner, id.clone()));
    service.gifts.tasks.spawn(async move {
        let result = async {
            let source = Files::Local(upload.stage.dir.try_clone()?);
            let snapshot =
                Snapshot::read(&source, MAX_BYTES, 4096, false)
                    .await?;
            drop(source);
            drop(upload.stage);
            remote_shelf(&remote, "inbox", &roots).await?;
            snapshot
                .deliver(&remote, "/house/inbox", &roots, true)
                .await
        }
        .await;
        Completed::Files {
            owner,
            id,
            original: upload.id,
            agent: upload.agent,
            result,
        }
    });
}

pub(crate) fn skill(
    service: &mut Service,
    owner: u64,
    id: &str,
    to: &Address,
    name: &str,
    from: Option<&Address>,
) -> Result<Option<ServerMessage>> {
    if name.is_empty()
        || !name.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
        })
    {
        return Ok(Some(reply(
            id,
            Err(refused(
                "a skill is named in letters, digits and hyphens",
            )),
        )));
    }
    let target = match service.agent(to, "give")? {
        Ok(agent) => AgentRef {
            hamlet: to.hamlet.clone(),
            number: agent.number,
        },
        Err(error) => return Ok(Some(reply(id, Err(error)))),
    };
    let mut giver = None;
    let mut source = None;
    let from = if let Some(from) = from {
        let agent = match service.agent(from, "give")? {
            Ok(agent) => agent,
            Err(error) => return Ok(Some(reply(id, Err(error)))),
        };
        giver = Some(format!(
            "{}{}",
            agent.name,
            if from.hamlet == to.hamlet {
                String::new()
            } else {
                format!("@{}", from.hamlet)
            }
        ));
        source =
            Some(service.houses.files(&from.hamlet, agent.number)?);
        Some((
            from.clone(),
            AgentRef {
                hamlet: from.hamlet.clone(),
                number: agent.number,
            },
        ))
    } else {
        if crate::starters::files(name).is_none() {
            return Ok(Some(reply(
                id,
                Err(refused(format!(
                    "no starter skill named `{name}`. Say whose it \
                     is: /give {} {name} from <agent>",
                    to.agent
                ))),
            )));
        }
        None
    };
    let gift = Skill {
        remote: service
            .houses
            .remote(&target.hamlet, target.number, false)?
            .expect("remote gift"),
        root: service.houses.root().try_clone()?,
        target,
        to: to.clone(),
        from,
        giver,
        name: name.into(),
        source,
    };
    let id = id.to_owned();
    service.gifts.working.insert((owner, id.clone()));
    service.gifts.tasks.spawn(async move {
        let snapshot = gift.prepare().await;
        Completed::Prepared {
            owner,
            id,
            gift,
            snapshot,
        }
    });
    Ok(None)
}

impl Skill {
    async fn prepare(&self) -> Result<Snapshot> {
        let shelf = Files::Remote {
            env: self.remote.clone(),
            path: "/house".into(),
        }
        .directory("skills", true)
        .await?;
        anyhow::ensure!(
            !shelf.present(&self.name).await?,
            "`{}` already has a skill named `{}`",
            self.to.agent,
            self.name
        );
        let snapshot = if let Some(source) = &self.source {
            let source = source
                .directory("skills", false)
                .await?
                .directory(&self.name, false)
                .await?;
            let snapshot =
                Snapshot::read(&source, MAX_BYTES, 4096, false)
                    .await?;
            anyhow::ensure!(
                snapshot.files.contains_key("SKILL.md"),
                "The giver has no skill named `{}`",
                self.name
            );
            Some(snapshot)
        } else {
            None
        };
        let stage = tree::Stage::new(&self.root)?;
        if let Some(snapshot) = snapshot {
            snapshot.write_local(&tree::directory(
                &stage.dir, &self.name, true,
            )?)?;
        } else {
            for (path, text) in
                crate::starters::files(&self.name).unwrap_or_default()
            {
                stage
                    .file(&format!("{}/{path}", self.name), true)?
                    .write_all(text.as_bytes())?;
            }
        }
        Snapshot::read(
            &Files::Local(stage.dir.try_clone()?),
            MAX_BYTES,
            4096,
            false,
        )
        .await
    }

    fn allowed(
        &self,
        service: &mut Service,
    ) -> Result<std::result::Result<(), Refusal>> {
        for (address, expected) in
            std::iter::once((&self.to, &self.target)).chain(
                self.from
                    .as_ref()
                    .map(|(address, agent)| (address, agent)),
            )
        {
            match service.agent(address, "give")? {
                Ok(agent) if agent.number == expected.number => {}
                Ok(_) => {
                    return Ok(Err(refused(
                        "The gift's agent is no longer present at \
                         that address.",
                    )));
                }
                Err(error) => return Ok(Err(error)),
            }
        }
        Ok(Ok(()))
    }
}

pub(crate) async fn complete(
    service: &mut Service,
    harnesses: &mut Harnesses,
    completed: Completed,
) -> Result<Option<(u64, ServerMessage)>> {
    match completed {
        Completed::Began {
            owner,
            id,
            token,
            result,
        } => {
            if !service.gifts.working.remove(&(owner, id.clone())) {
                return Ok(None);
            }
            let Some(mut upload) =
                service.gifts.pending.remove(&token)
            else {
                return Ok(Some((
                    owner,
                    reply(&id, Err(refused(super::UNKNOWN))),
                )));
            };
            let result = match result {
                Err(error) => Err(refused(error.to_string())),
                Ok(()) => match service.agent(&upload.to, "give")? {
                    Err(error) => Err(error),
                    Ok(agent)
                        if agent.number != upload.agent.number =>
                    {
                        Err(refused(
                            "The receiving agent is no longer \
                             present.",
                        ))
                    }
                    Ok(_) if upload.bytes == 0 => {
                        service.gift_finish(upload, &id).await?
                    }
                    Ok(_) => {
                        upload.touched = std::time::Instant::now();
                        service
                            .gifts
                            .pending
                            .insert(token.clone(), upload);
                        Ok(Progress::Waiting(token))
                    }
                },
            };
            Ok(super::receipt(service, harnesses, &id, result)
                .await?
                .map(|answer| (owner, answer)))
        }
        Completed::Files {
            owner,
            id,
            original,
            agent,
            result,
        } => {
            service.gifts.working.remove(&(owner, id.clone()));
            let progress = result
                .map(|paths| Progress::Delivered {
                    id: original,
                    agent,
                    paths,
                })
                .map_err(|error| refused(error.to_string()));
            Ok(super::receipt(service, harnesses, &id, progress)
                .await?
                .map(|answer| (owner, answer)))
        }
        Completed::Prepared {
            owner,
            id,
            gift,
            snapshot,
        } => {
            if !service.gifts.working.contains(&(owner, id.clone())) {
                return Ok(None);
            }
            let allowed = gift.allowed(service)?;
            let snapshot = match snapshot
                .map_err(|error| refused(error.to_string()))
                .and_then(|snapshot| allowed.map(|()| snapshot))
            {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    service
                        .gifts
                        .working
                        .remove(&(owner, id.clone()));
                    return Ok(Some((owner, reply(&id, Err(error)))));
                }
            };
            service.gifts.tasks.spawn(async move {
                let result = async {
                    remote_shelf(
                        &gift.remote,
                        "skills",
                        std::slice::from_ref(&gift.name),
                    )
                    .await?;
                    snapshot
                        .deliver(
                            &gift.remote,
                            "/house/skills",
                            std::slice::from_ref(&gift.name),
                            false,
                        )
                        .await?;
                    Ok(())
                }
                .await;
                Completed::Delivered {
                    owner,
                    id,
                    gift,
                    result,
                }
            });
            Ok(None)
        }
        Completed::Delivered {
            owner,
            id,
            gift,
            result,
        } => {
            service.gifts.working.remove(&(owner, id.clone()));
            let result = match result {
                Err(error) => Err(refused(error.to_string())),
                Ok(()) => {
                    if let Some(mut record) = service
                        .home
                        .store()
                        .record(&gift.target.hamlet)?
                    {
                        record.decisions.push(Decision {
                            at: chrono::Utc::now().timestamp_millis(),
                            by: service.chief.clone(),
                            reason: String::new(),
                            kind: DecisionKind::SkillGiven {
                                to: gift.target.number,
                                skill: gift.name.clone(),
                                from: gift.giver,
                            },
                        });
                        let tx =
                            service.home.store().transaction()?;
                        crate::record::save(&tx, &record)?;
                        tx.commit()?;
                    }
                    Ok(format!(
                        "Gave `{}` the skill `{}`.",
                        gift.to.agent, gift.name
                    ))
                }
            };
            Ok(Some((owner, reply(&id, result))))
        }
    }
}
