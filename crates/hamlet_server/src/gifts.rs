//! Connection-owned file uploads and chief skill gifts (H25, H28).

pub(crate) mod remote;
mod rules;
#[cfg(all(test, unix))]
mod tests;
#[cfg(unix)]
mod tree;
#[cfg(not(unix))]
mod unsupported;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::Result;
use hamlet_protocol::v0::{
    Address, ClientMessage, Give, GiveFilesFiles, GivePart,
    GiveSkillSkill, ServerMessage,
};
use rules::{DELIVERY, MAX_BYTES, MISMATCH, STAGING, UNKNOWN};
use serde_json::{Value, json};

use crate::harness::Harnesses;
use crate::protocol::{Refusal, reply};
use crate::record::{AgentRef, Decision, DecisionKind};
use crate::service::Service;

#[derive(Default)]
pub(crate) struct Uploads {
    #[cfg(unix)]
    pending: HashMap<String, Upload>,
    pub tasks: tokio::task::JoinSet<remote::Completed>,
    pub working: HashSet<(u64, String)>,
}

#[cfg(unix)]
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
}

impl Uploads {
    pub fn gone(&mut self, owner: u64) {
        #[cfg(unix)]
        self.pending.retain(|_, u| u.owner != owner);
        self.working.retain(|(client, _)| *client != owner);
    }

    pub fn expire(&mut self) {
        #[cfg(unix)]
        self.pending.retain(|_, u| {
            self.working.contains(&(u.owner, u.id.clone()))
                || u.touched.elapsed() < Duration::from_secs(60)
        });
    }
}

fn refused(words: impl Into<String>) -> Refusal {
    Refusal::Invalid(words.into())
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
            if service.houses.workspace.is_some() {
                return remote::skill(
                    service, owner, id, to, name, from,
                );
            }
            return Ok(Some(reply(
                id,
                service.gift_skill(to, name, from).await?,
            )));
        }
        ClientMessage::Give(Give::Files { id, to, files }) => {
            (id, service.gift_begin(owner, id, to, files).await?)
        }
        ClientMessage::GivePart(part) => {
            (&part.id, service.gift_part(owner, part).await?)
        }
        _ => return Ok(None),
    };
    receipt(service, harnesses, id, progress).await
}

async fn receipt(
    service: &mut Service,
    harnesses: &mut Harnesses,
    id: &str,
    progress: std::result::Result<Progress, Refusal>,
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
    };
    let mut answer = reply(id, Ok(text));
    if let ServerMessage::Reply(answer) = &mut answer {
        answer.upload = upload;
    }
    Ok(Some(answer))
}

#[cfg(unix)]
impl Service {
    pub(crate) fn prepare_gifts(&self) -> Result<()> {
        tree::sweep(self.houses.root())
    }

    async fn gift_begin(
        &mut self,
        owner: u64,
        id: &str,
        to: &Address,
        files: &[GiveFilesFiles],
    ) -> Result<std::result::Result<Progress, Refusal>> {
        use std::io::Write;
        let target = match self.agent(to, "give")? {
            Ok(a) => a,
            Err(e) => return Ok(Err(e)),
        };
        let bytes = match rules::manifest(files) {
            Ok(n) => n,
            Err(e) => return Ok(Err(refused(e))),
        };
        let agent = AgentRef {
            hamlet: to.hamlet.clone(),
            number: target.number,
        };
        let roots = roots(files.iter().map(|f| f.path.as_str()));
        let remote =
            self.houses.remote(&agent.hamlet, agent.number, false)?;
        if remote.is_some()
            && files.iter().any(|file| {
                file.size > crate::workspace::MAX_FILE as u64
            })
        {
            return Ok(Err(refused(
                "Remote files are limited to 4 MiB per transfer.",
            )));
        }
        if remote.is_none()
            && self
                .houses
                .house(&agent.hamlet, agent.number, false)
                .and_then(|house| {
                    tree::shelf(&house, "inbox", &roots)
                })
                .is_err()
        {
            return Ok(Err(refused(DELIVERY)));
        }
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
        let stage = (|| -> Result<_> {
            let stage = tree::Stage::new(self.houses.root())?;
            for file in files {
                stage.file(&file.path, true)?.flush()?;
            }
            Ok(stage)
        })();
        let stage = match stage {
            Ok(stage) => stage,
            Err(_) => return Ok(Err(refused(STAGING))),
        };
        let upload = Upload {
            owner,
            to: to.clone(),
            agent,
            id: id.into(),
            bytes,
            touched: Instant::now(),
            files: files.iter().cloned().map(|f| (f, 0)).collect(),
            stage,
        };
        if let Some(remote) = remote {
            remote::begin(self, upload, remote, roots);
            return Ok(Ok(Progress::Pending));
        }
        if bytes == 0 {
            return self.gift_finish(upload, id).await;
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.gifts.pending.insert(token.clone(), upload);
        Ok(Ok(Progress::Waiting(token)))
    }

    async fn gift_part(
        &mut self,
        owner: u64,
        part: &GivePart,
    ) -> Result<std::result::Result<Progress, Refusal>> {
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
        let bytes = match STANDARD.decode(&part.data) {
            Ok(b) => b,
            Err(_) => return Ok(Err(refused(MISMATCH))),
        };
        if bytes.is_empty()
            || u64::try_from(part.offset).ok() != Some(*offset)
            || *offset + bytes.len() as u64 > file.size as u64
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
            .all(|(file, offset)| *offset == file.size as u64)
        {
            return self.gift_finish(upload, &part.id).await;
        }
        upload.touched = Instant::now();
        self.gifts.pending.insert(part.upload.clone(), upload);
        Ok(Ok(Progress::Received))
    }

    async fn gift_finish(
        &mut self,
        upload: Upload,
        request: &str,
    ) -> Result<std::result::Result<Progress, Refusal>> {
        let target = match self.agent(&upload.to, "give")? {
            Ok(a) => a,
            Err(e) => return Ok(Err(e)),
        };
        if target.number != upload.agent.number {
            return Ok(Err(refused(
                "The receiving agent is no longer present.",
            )));
        }
        let roots =
            roots(upload.files.iter().map(|(f, _)| f.path.as_str()));
        if let Some(remote) = self.houses.remote(
            &upload.agent.hamlet,
            upload.agent.number,
            false,
        )? {
            remote::finish(self, upload, request, remote, roots);
            return Ok(Ok(Progress::Pending));
        }
        let delivered = (|| -> Result<_> {
            let house = self.houses.house(
                &upload.agent.hamlet,
                upload.agent.number,
                false,
            )?;
            let shelf = tree::shelf(&house, "inbox", &roots)?;
            upload.stage.deliver(&shelf, &roots, true)
        })();
        Ok(delivered
            .map(|paths| Progress::Delivered {
                id: upload.id,
                agent: upload.agent,
                paths,
            })
            .map_err(|error| {
                if self.houses.workspace.is_some() {
                    refused(error.to_string())
                } else {
                    refused(DELIVERY)
                }
            }))
    }

    pub(crate) async fn gift_skill(
        &mut self,
        to: &Address,
        skill: &str,
        from: Option<&Address>,
    ) -> Result<std::result::Result<String, Refusal>> {
        if skill.is_empty()
            || !skill.bytes().all(|b| {
                b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
            })
        {
            return Ok(Err(refused(
                "a skill is named in letters, digits and hyphens",
            )));
        }
        let target = match self.agent(to, "give")? {
            Ok(a) => a,
            Err(e) => return Ok(Err(e)),
        };
        let shelf = self
            .houses
            .files(&to.hamlet, target.number)?
            .directory("skills", true)
            .await;
        let shelf = match shelf {
            Ok(s) => s,
            Err(_) => return Ok(Err(refused(DELIVERY))),
        };
        match shelf.present(skill).await {
            Ok(true) => {
                return Ok(Err(refused(format!(
                    "`{}` already has a skill named `{skill}`",
                    target.name
                ))));
            }
            Err(_) => return Ok(Err(refused(DELIVERY))),
            Ok(false) => {}
        }
        let mut giver = None;
        let mut remote_source = None;
        let source = if let Some(from) = from {
            let agent = match self.agent(from, "give")? {
                Ok(a) => a,
                Err(e) => return Ok(Err(e)),
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
            if self.houses.workspace.is_some() {
                let copied = async {
                    let source = self
                        .houses
                        .files(&from.hamlet, agent.number)?
                        .directory("skills", false)
                        .await?
                        .directory(skill, false)
                        .await?;
                    let snapshot =
                        crate::papers::copy::Snapshot::read(
                            &source, MAX_BYTES, 4096, false,
                        )
                        .await?;
                    anyhow::ensure!(
                        snapshot.files.contains_key("SKILL.md"),
                        "`{}` has no skill named `{skill}`",
                        from.agent
                    );
                    Ok::<_, anyhow::Error>(snapshot)
                }
                .await;
                match copied {
                    Ok(snapshot) => remote_source = Some(snapshot),
                    Err(error) => {
                        return Ok(Err(refused(error.to_string())));
                    }
                }
                None
            } else {
                let source = self
                    .houses
                    .house(&from.hamlet, agent.number, false)
                    .and_then(|h| {
                        tree::directory(&h, "skills", false)
                    });
                let source = match source {
                    Ok(s) => s,
                    Err(_) => return Ok(Err(refused(DELIVERY))),
                };
                let paper = match tree::paper(&source, skill) {
                    Ok(paper) => paper,
                    Err(words) => return Ok(Err(refused(words))),
                };
                if !paper {
                    return Ok(Err(refused(format!(
                        "`{}` has no skill named `{skill}`",
                        from.agent
                    ))));
                }
                Some(source)
            }
        } else {
            if crate::starters::files(skill).is_none() {
                return Ok(Err(refused(format!(
                    "no starter skill named `{skill}`. Say whose it \
                     is: /give {} {skill} from <agent>",
                    target.name
                ))));
            }
            None
        };
        let stage = match tree::Stage::new(self.houses.root()) {
            Ok(s) => s,
            Err(_) => return Ok(Err(refused(STAGING))),
        };
        if let Some(snapshot) = remote_source {
            let target = tree::directory(&stage.dir, skill, true)?;
            snapshot.write_local(&target)?;
        } else if let Some(source) = source {
            if let Err(words) = tree::copy(&source, &stage, skill) {
                return Ok(Err(refused(words)));
            }
        } else {
            use std::io::Write;
            for (path, text) in
                crate::starters::files(skill).unwrap_or_default()
            {
                if stage
                    .file(&format!("{skill}/{path}"), true)
                    .and_then(|mut f| {
                        Ok(f.write_all(text.as_bytes())?)
                    })
                    .is_err()
                {
                    return Ok(Err(refused(STAGING)));
                }
            }
        }
        // Reopen the house and shelf after reading the giver's tree.
        let delivered = if let Some(remote) =
            self.houses.remote(&to.hamlet, target.number, false)?
        {
            async {
                remote_shelf(&remote, "skills", &[skill.into()])
                    .await?;
                let source = crate::papers::files::Files::Local(
                    stage.dir.try_clone()?,
                );
                let snapshot = crate::papers::copy::Snapshot::read(
                    &source, MAX_BYTES, 4096, false,
                )
                .await?;
                snapshot
                    .deliver(
                        &remote,
                        "/house/skills",
                        &[skill.into()],
                        false,
                    )
                    .await
            }
            .await
        } else {
            self.houses
                .house(&to.hamlet, target.number, false)
                .and_then(|h| {
                    tree::shelf(&h, "skills", &[skill.into()])
                })
                .and_then(|s| {
                    stage.deliver(&s, &[skill.into()], false)
                })
        };
        if let Err(error) = delivered {
            if self.houses.workspace.is_some() {
                return Ok(Err(refused(error.to_string())));
            }
            return Ok(Err(refused(DELIVERY)));
        }
        let mut record =
            self.home.store().record(&to.hamlet)?.ok_or_else(
                || anyhow::anyhow!("gift recipient has no record"),
            )?;
        record.decisions.push(Decision {
            at: chrono::Utc::now().timestamp_millis(),
            by: self.chief.clone(),
            reason: String::new(),
            kind: DecisionKind::SkillGiven {
                to: target.number,
                skill: skill.into(),
                from: giver,
            },
        });
        let tx = self.home.store().db.transaction()?;
        crate::record::save(&tx, &record)?;
        tx.commit()?;
        Ok(Ok(format!("Gave `{}` the skill `{skill}`.", to.agent)))
    }
}

async fn remote_shelf(
    remote: &crate::workspace::Remote,
    name: &str,
    roots: &[String],
) -> Result<()> {
    use crate::sandbox::FileKind;
    let house = crate::papers::files::Files::Remote {
        env: remote.clone(),
        path: "/house".into(),
    };
    house.directory(name, true).await?;
    for root in roots {
        match remote.info(&format!("/house/{name}/{root}")).await {
            Ok(info) => anyhow::ensure!(
                info.kind != FileKind::Symlink,
                "linked gift target"
            ),
            Err(error) if error.code == "not_found" => {}
            Err(error) => anyhow::bail!("{}", error.message),
        }
    }
    Ok(())
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

pub(crate) fn starters() -> Vec<Value> {
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
            json!({"name": name, "description": description})
        })
        .collect()
}
