//! Connection-owned file uploads and chief skill gifts (H25, H28).
//! Uploads are staged on this machine; every delivery runs apart from
//! the service loop on the houses' backend.

mod rules;
#[cfg(test)]
mod tests;
mod tree;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::Result;
use hamlet_protocol::client::{
    self as protocol, Address, ClientMessage, Give, GiveFilesFiles,
    GivePart, GiveSkillSkill, ServerMessage,
};
use hamlet_workspace_wire::{Code, TREE_ENTRIES, TreeKind};
use rules::{DELIVERY, MAX_BYTES, MISMATCH, STAGING, UNKNOWN};
use serde_json::Value;

use crate::harness::Harnesses;
use crate::papers::copy::{Snapshot, Stopped, UNSAFE};
use crate::papers::files::Files;
use crate::papers::tree::Entry;
use crate::protocol::{Refusable, Refusal, reply};
use crate::record::AgentRef;
use crate::service::Service;

/// The most files and folders a skill gift copies at once.
const SKILL_ENTRIES: usize = 4096;

#[derive(Default)]
pub(crate) struct Uploads {
    pending: HashMap<String, Upload>,
    pub tasks: tokio::task::JoinSet<Completed>,
    pub working: HashSet<(u64, String)>,
}

struct Upload {
    owner: u64,
    to: Address,
    agent: AgentRef,
    id: String,
    bytes: u64,
    touched: Instant,
    files: Vec<(GiveFilesFiles, u64)>,
    stage: tree::Stage,
}

enum Progress {
    Pending,
    Waiting(String),
    Received,
    Delivered {
        id: String,
        agent: AgentRef,
        paths: Vec<String>,
    },
    Given(String),
}

/// A skill gift between its steps: whom it is for, and where its
/// skill comes from.
pub(crate) struct Skill {
    to: Address,
    target: AgentRef,
    /// The receiver's name in the record.
    receiver: String,
    from: Option<(Address, AgentRef)>,
    giver: Option<String>,
    name: String,
    house: Files,
    source: Option<Files>,
}

/// A gift step that finished apart from the service loop.
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
        snapshot: Refusable<Snapshot>,
    },
    Delivered {
        owner: u64,
        id: String,
        gift: Skill,
        result: Result<Vec<String>>,
    },
}

impl Uploads {
    pub fn gone(&mut self, owner: u64) {
        self.pending.retain(|_, u| u.owner != owner);
        self.working.retain(|(client, _)| *client != owner);
    }

    pub fn expire(&mut self) {
        self.pending.retain(|_, u| {
            self.working.contains(&(u.owner, u.id.clone()))
                || u.touched.elapsed() < Duration::from_secs(60)
        });
    }
}

fn refused(words: impl Into<String>) -> Refusal {
    Refusal::Invalid(words.into())
}

/// The chief's words for a delivery that failed: the copy's own where
/// it may have changed the target, else that nothing was handed over.
fn undelivered(error: &anyhow::Error) -> Refusal {
    match error.downcast_ref::<Stopped>() {
        Some(stopped) => refused(stopped.0),
        None => refused(DELIVERY),
    }
}

/// Handles the gift protocol, including admission of its completed
/// file letter.
pub(crate) async fn request(
    service: &mut Service,
    harnesses: &mut Harnesses,
    owner: u64,
    message: &ClientMessage,
) -> Result<Option<ServerMessage>> {
    let (id, progress) = match message {
        ClientMessage::Give(Give::Skill { id, to, skill }) => {
            let (name, from) = match skill {
                GiveSkillSkill::Starter { starter } => {
                    (starter, None)
                }
                GiveSkillSkill::From { from, name } => {
                    (name, Some(from))
                }
            };
            (id, service.gift_skill(owner, id, to, name, from)?)
        }
        ClientMessage::Give(Give::Files { id, to, files }) => {
            (id, service.gift_begin(owner, id, to, files)?)
        }
        ClientMessage::GivePart(part) => {
            (&part.id, service.gift_part(owner, part)?)
        }
        _ => return Ok(None),
    };
    receipt(service, harnesses, id, progress).await
}

/// Starts the chief's gift of the skill `name` to `to`, from `from`'s
/// house or the starters; the answer comes once it is delivered.
pub(crate) fn skill(
    service: &mut Service,
    owner: u64,
    id: &str,
    to: &Address,
    name: &str,
    from: Option<&Address>,
) -> Result<Option<ServerMessage>> {
    Ok(match service.gift_skill(owner, id, to, name, from)? {
        Err(error) => Some(reply(id, Err(error))),
        Ok(_) => None,
    })
}

async fn receipt(
    service: &mut Service,
    harnesses: &mut Harnesses,
    id: &str,
    progress: Refusable<Progress>,
) -> Result<Option<ServerMessage>> {
    let progress = match progress {
        Ok(progress) => progress,
        Err(error) => return Ok(Some(reply(id, Err(error)))),
    };
    let mut upload = None;
    let text = match progress {
        Progress::Pending => return Ok(None),
        Progress::Waiting(token) => {
            upload = Some(token);
            "Ready for the gift's file parts.".into()
        }
        Progress::Received => "Received the gift part.".into(),
        Progress::Delivered { id, agent, paths } => {
            let paths = paths
                .iter()
                .map(|p| format!("`inbox/{p}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut line = Value::String(format!(
                "I handed you {paths} in your workspace. Read it \
                 and do what it asks. If it asks for nothing, tell \
                 me in a few lines what it is."
            ));
            service.credentials.scrub(&mut line);
            crate::chats::admit_line(
                service,
                harnesses,
                &id,
                &agent,
                line.as_str().unwrap_or_default(),
            )
            .await?;
            format!("Handed over {paths}.")
        }
        Progress::Given(text) => text,
    };
    let mut answer = reply(id, Ok(text));
    if let ServerMessage::Reply(answer) = &mut answer {
        answer.upload = upload;
    }
    Ok(Some(answer))
}

/// The folder `name` of `house`, made where it is missing, whose
/// `roots` are not links.
async fn shelf(
    house: &Files,
    name: &str,
    roots: &[String],
) -> Result<Files> {
    let shelf = house.directory(name, true).await?;
    for root in roots {
        anyhow::ensure!(
            shelf.kind(root).await? != Some(TreeKind::Link),
            "linked gift target"
        );
    }
    Ok(shelf)
}

impl Service {
    pub(crate) fn prepare_gifts(&self) -> Result<()> {
        tree::sweep(self.houses.root())
    }

    fn gift_begin(
        &mut self,
        owner: u64,
        id: &str,
        to: &Address,
        files: &[GiveFilesFiles],
    ) -> Result<Refusable<Progress>> {
        use std::io::Write;
        let target = match self.agent(to, "give")? {
            Ok(a) => a,
            Err(e) => return Ok(Err(e)),
        };
        let bytes = match rules::manifest(files) {
            Ok(n) => n,
            Err(e) => return Ok(Err(refused(e))),
        };
        for file in files {
            if let Err(words) = self.houses.admits(file.size) {
                return Ok(Err(refused(words)));
            }
        }
        let agent = AgentRef {
            hamlet: to.hamlet.clone(),
            number: target.number,
        };
        self.gifts.expire();
        if self
            .gifts
            .pending
            .values()
            .filter(|u| u.owner == owner)
            .count()
            >= 3
        {
            return Ok(Err(refused(
                "This connection already has three file gifts \
                 waiting. Finish one before starting another.",
            )));
        }
        let reserved = self
            .gifts
            .pending
            .values()
            .filter(|u| u.agent == agent)
            .map(|u| u.bytes)
            .sum::<u64>();
        if reserved + bytes > MAX_BYTES {
            return Ok(Err(refused(
                "This agent already has 32 MB of file gifts \
                 waiting. Finish or discard one before starting \
                 another.",
            )));
        }
        let house = self.houses.house(&agent.hamlet, agent.number)?;
        let stage = (|| -> Result<_> {
            let stage = tree::Stage::new(self.houses.root())?;
            for file in files {
                stage.file(&file.path, true)?.flush()?;
            }
            Ok(stage)
        })();
        let Ok(stage) = stage else {
            return Ok(Err(refused(STAGING)));
        };
        let roots = roots(files.iter().map(|f| f.path.as_str()));
        let token = uuid::Uuid::new_v4().to_string();
        self.gifts.pending.insert(
            token.clone(),
            Upload {
                owner,
                to: to.clone(),
                agent,
                id: id.into(),
                bytes,
                touched: Instant::now(),
                files: files
                    .iter()
                    .cloned()
                    .map(|f| (f, 0))
                    .collect(),
                stage,
            },
        );
        self.gifts.working.insert((owner, id.into()));
        let id = id.to_owned();
        self.gifts.tasks.spawn(async move {
            let result =
                shelf(&house, "inbox", &roots).await.map(drop);
            Completed::Began {
                owner,
                id,
                token,
                result,
            }
        });
        Ok(Ok(Progress::Pending))
    }

    fn gift_part(
        &mut self,
        owner: u64,
        part: &GivePart,
    ) -> Result<Refusable<Progress>> {
        use std::io::Write;

        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;
        self.gifts.expire();
        if self
            .gifts
            .pending
            .get(&part.upload)
            .is_none_or(|u| u.owner != owner)
        {
            return Ok(Err(refused(UNKNOWN)));
        }
        let Some(mut upload) =
            self.gifts.pending.remove(&part.upload)
        else {
            return Ok(Err(refused(UNKNOWN)));
        };
        let Some((file, offset)) = upload
            .files
            .iter_mut()
            .find(|(f, _)| f.path == part.path)
        else {
            return Ok(Err(refused(MISMATCH)));
        };
        let Ok(bytes) = STANDARD.decode(&part.data) else {
            return Ok(Err(refused(MISMATCH)));
        };
        if bytes.is_empty()
            || part.offset != *offset
            || *offset + bytes.len() as u64 > file.size
            || STANDARD.encode(&bytes) != part.data
        {
            return Ok(Err(refused(MISMATCH)));
        }
        if upload
            .stage
            .file(&part.path, false)
            .and_then(|mut f| Ok(f.write_all(&bytes)?))
            .is_err()
        {
            return Ok(Err(refused(STAGING)));
        }
        *offset += bytes.len() as u64;
        if upload
            .files
            .iter()
            .all(|(file, offset)| *offset == file.size)
        {
            return self.gift_finish(upload, &part.id);
        }
        upload.touched = Instant::now();
        self.gifts.pending.insert(part.upload.clone(), upload);
        Ok(Ok(Progress::Received))
    }

    /// Delivers the complete `upload` into its agent's inbox, for the
    /// request `request`.
    fn gift_finish(
        &mut self,
        upload: Upload,
        request: &str,
    ) -> Result<Refusable<Progress>> {
        let target = match self.agent(&upload.to, "give")? {
            Ok(a) => a,
            Err(e) => return Ok(Err(e)),
        };
        if target.number != upload.agent.number {
            return Ok(Err(refused(
                "The receiving agent is no longer present.",
            )));
        }
        let house = self
            .houses
            .house(&upload.agent.hamlet, upload.agent.number)?;
        let roots =
            roots(upload.files.iter().map(|(f, _)| f.path.as_str()));
        let owner = upload.owner;
        let id = request.to_owned();
        self.gifts.working.insert((owner, id.clone()));
        self.gifts.tasks.spawn(async move {
            let result = async {
                let source =
                    Files::Local(upload.stage.dir.try_clone()?);
                let snapshot = Snapshot::read(
                    &source,
                    MAX_BYTES,
                    TREE_ENTRIES as usize,
                    false,
                )
                .await?;
                drop(source);
                drop(upload.stage);
                let inbox = shelf(&house, "inbox", &roots).await?;
                snapshot.deliver(&inbox, &roots, true).await
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
        Ok(Ok(Progress::Pending))
    }

    /// Starts the gift of the skill `name` to `to`, from `from`'s
    /// house or the starters.
    fn gift_skill(
        &mut self,
        owner: u64,
        id: &str,
        to: &Address,
        name: &str,
        from: Option<&Address>,
    ) -> Result<Refusable<Progress>> {
        let gift = match self.resolve_skill(to, name, from)? {
            Ok(gift) => gift,
            Err(error) => return Ok(Err(error)),
        };
        let id = id.to_owned();
        self.gifts.working.insert((owner, id.clone()));
        self.gifts.tasks.spawn(async move {
            let snapshot = gift.snapshot().await;
            Completed::Prepared {
                owner,
                id,
                gift,
                snapshot,
            }
        });
        Ok(Ok(Progress::Pending))
    }

    /// The gift of the skill `name` to `to`, or why it cannot be.
    fn resolve_skill(
        &mut self,
        to: &Address,
        name: &str,
        from: Option<&Address>,
    ) -> Result<Refusable<Skill>> {
        if name.is_empty()
            || !name.bytes().all(|b| {
                b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
            })
        {
            return Ok(Err(refused(
                "a skill is named in letters, digits and hyphens",
            )));
        }
        let receiver = match self.agent(to, "give")? {
            Ok(agent) => agent,
            Err(error) => return Ok(Err(error)),
        };
        let mut giver = None;
        let mut source = None;
        let from = if let Some(from) = from {
            let agent = match self.agent(from, "give")? {
                Ok(agent) => agent,
                Err(error) => return Ok(Err(error)),
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
                Some(self.houses.house(&from.hamlet, agent.number)?);
            Some((
                from.clone(),
                AgentRef {
                    hamlet: from.hamlet.clone(),
                    number: agent.number,
                },
            ))
        } else {
            if crate::starters::files(name).is_none() {
                return Ok(Err(refused(format!(
                    "no starter skill named `{name}`. Say whose it \
                     is: /give {} {name} from <agent>",
                    receiver.name
                ))));
            }
            None
        };
        Ok(Ok(Skill {
            house: self.houses.house(&to.hamlet, receiver.number)?,
            target: AgentRef {
                hamlet: to.hamlet.clone(),
                number: receiver.number,
            },
            receiver: receiver.name,
            to: to.clone(),
            from,
            giver,
            name: name.into(),
            source,
        }))
    }
}

impl Skill {
    /// The skill's tree under its name, once the receiver is known
    /// not to have one.
    async fn snapshot(&self) -> Refusable<Snapshot> {
        let name = &self.name;
        let shelf = self
            .house
            .directory("skills", true)
            .await
            .map_err(|_| refused(DELIVERY))?;
        if shelf
            .kind(name)
            .await
            .map_err(|_| refused(DELIVERY))?
            .is_some()
        {
            return Err(refused(format!(
                "`{}` already has a skill named `{name}`",
                self.receiver
            )));
        }
        let Some((from, source)) =
            self.from.as_ref().zip(self.source.as_ref())
        else {
            let files = crate::starters::files(name)
                .unwrap_or_default()
                .iter()
                .map(|(path, text)| {
                    let entry = Entry {
                        bytes: text.as_bytes().to_vec(),
                        mode: 0o644,
                    };
                    (format!("{name}/{path}"), entry)
                })
                .collect::<BTreeMap<_, _>>();
            return Ok(Snapshot::of(files));
        };
        let missing = || {
            refused(format!(
                "`{}` has no skill named `{name}`",
                from.0.agent
            ))
        };
        let skills = source
            .directory("skills", false)
            .await
            .map_err(|_| refused(DELIVERY))?;
        match skills
            .kind(name)
            .await
            .map_err(|_| refused(DELIVERY))?
        {
            Some(TreeKind::Directory) => {}
            Some(TreeKind::Link | TreeKind::Special) => {
                return Err(refused(UNSAFE));
            }
            _ => return Err(missing()),
        }
        let skill = skills
            .directory(name, false)
            .await
            .map_err(|_| refused(UNSAFE))?;
        if skill.kind("SKILL.md").await.ok().flatten()
            != Some(TreeKind::File)
        {
            return Err(missing());
        }
        let snapshot =
            Snapshot::read(&skill, MAX_BYTES, SKILL_ENTRIES, false)
                .await
                .map_err(|error| {
                    if crate::workspace::is(&error, Code::TooMany) {
                        refused(
                            "A skill gift takes at most 4096 files \
                             and folders at once.",
                        )
                    } else if crate::workspace::is(
                        &error,
                        Code::TooLarge,
                    ) {
                        refused(
                            "The skill gift is larger than 32 MB.",
                        )
                    } else {
                        undelivered(&error)
                    }
                })?;
        Ok(snapshot.under(name))
    }

    /// Whether the receiver and the giver are still the agents the
    /// gift was asked for.
    fn allowed(
        &self,
        service: &mut Service,
    ) -> Result<Refusable<()>> {
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

    /// Delivers `snapshot` onto the receiver's shelf.
    async fn deliver(
        &self,
        snapshot: &Snapshot,
    ) -> Result<Vec<String>> {
        let roots = std::slice::from_ref(&self.name);
        let skills = shelf(&self.house, "skills", roots).await?;
        snapshot.deliver(&skills, roots, false).await
    }

    /// Keeps the gift in the record.
    fn record(self, service: &mut Service) -> Result<String> {
        crate::record::give_skill(
            service.db(),
            &service.chief,
            &self.target.hamlet,
            self.target.number,
            &self.name,
            self.giver,
        )?;
        Ok(format!(
            "Gave `{}` the skill `{}`.",
            self.to.agent, self.name
        ))
    }
}

/// Answers the connection a finished gift step was for, unless it is
/// gone.
pub(crate) async fn complete(
    service: &mut Service,
    harnesses: &mut Harnesses,
    completed: Completed,
) -> Result<Option<(u64, ServerMessage)>> {
    let Some((owner, id, progress)) = step(service, completed)?
    else {
        return Ok(None);
    };
    Ok(receipt(service, harnesses, &id, progress)
        .await?
        .map(|answer| (owner, answer)))
}

/// The gift's next step after `completed`: its connection, its
/// request and how it stands, unless the connection is gone.
fn step(
    service: &mut Service,
    completed: Completed,
) -> Result<Option<(u64, String, Refusable<Progress>)>> {
    Ok(Some(match completed {
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
                return Ok(Some((owner, id, Err(refused(UNKNOWN)))));
            };
            let progress = match result {
                Err(error) => Err(undelivered(&error)),
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
                        service.gift_finish(upload, &id)?
                    }
                    Ok(_) => {
                        upload.touched = Instant::now();
                        service
                            .gifts
                            .pending
                            .insert(token.clone(), upload);
                        Ok(Progress::Waiting(token))
                    }
                },
            };
            (owner, id, progress)
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
                .map_err(|error| undelivered(&error));
            (owner, id, progress)
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
                .and_then(|snapshot| allowed.map(|()| snapshot))
            {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    service
                        .gifts
                        .working
                        .remove(&(owner, id.clone()));
                    return Ok(Some((owner, id, Err(error))));
                }
            };
            service.gifts.tasks.spawn(async move {
                let result = gift.deliver(&snapshot).await;
                Completed::Delivered {
                    owner,
                    id,
                    gift,
                    result,
                }
            });
            return Ok(None);
        }
        Completed::Delivered {
            owner,
            id,
            gift,
            result,
        } => {
            service.gifts.working.remove(&(owner, id.clone()));
            let progress = match result {
                Err(error) => Err(undelivered(&error)),
                Ok(_) => Ok(Progress::Given(gift.record(service)?)),
            };
            (owner, id, progress)
        }
    }))
}

fn roots<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut roots = Vec::new();
    for path in paths {
        let root =
            path.split('/').next().unwrap_or_default().to_owned();
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

pub(crate) fn starters() -> Vec<protocol::Skill> {
    let mut names = crate::starters::names().collect::<Vec<_>>();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let description = crate::starters::files(name)
                .into_iter()
                .flatten()
                .find(|(path, _)| *path == "SKILL.md")
                .and_then(|(_, text)| {
                    text.lines().find_map(|line| {
                        line.strip_prefix("description: ")
                    })
                })
                .unwrap_or_default();
            protocol::Skill {
                description: description.into(),
                name: name.into(),
            }
        })
        .collect()
}
