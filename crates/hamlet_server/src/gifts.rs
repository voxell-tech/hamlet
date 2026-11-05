//! What a chief gives (C14): files sent in parts to an agent's
//! `inbox/`, and skills committed to a scholar's `main` from a
//! starter, the library, another scholar or a hire's lodge.

use std::collections::HashMap;

use anyhow::Result;
use base64::Engine as _;
use hamlet_git::{Change as GitChange, MAIN, Repository};
use hamlet_protocol::client::{
    GiveFile, GivePart, GiveSkillSource, Keeper, RequestId,
    ServerMessage,
};
use hamlet_store::ids::{Actor, AgentId, ScholarId};

use crate::clients::check::reply;
use crate::core::Core;
use crate::knowledge::folder::{Folder, remove_folder};
use crate::knowledge::{self, library};
use crate::society::refusal::Refusal;
use crate::society::{addresses, authority, city};
use crate::talk::chats;

/// The most a file gift may hold, together.
const MOST_BYTES: u64 = 32 * 1024 * 1024;
/// The most files one gift may hold.
const MOST_FILES: usize = 1000;

/// The file gifts being sent.
#[derive(Default)]
pub(crate) struct Gifts {
    uploads: HashMap<String, Upload>,
}

struct Upload {
    client: u64,
    /// The request that began it, which its delivery answers too.
    id: RequestId,
    to: AgentId,
    files: Vec<GiveFile>,
    /// The bytes each file holds so far, by path.
    received: HashMap<String, u64>,
}

impl Gifts {
    /// Drops what the connection `client` was sending.
    pub fn disconnect(
        &mut self,
        client: u64,
        staging: &std::path::Path,
    ) {
        self.uploads.retain(|upload, found| {
            let keep = found.client != client;
            if !keep {
                let _ = remove_folder(&staging.join(upload));
            }
            keep
        });
    }
}

/// Whether `path` is a plain relative path of names.
fn is_plain(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".."
        })
}

impl Core {
    /// Begins a gift of `files` to `to`'s inbox; its reply names the
    /// upload its parts are sent under.
    pub(crate) fn give_files(
        &mut self,
        client: u64,
        id: &RequestId,
        to: AgentId,
        files: Vec<GiveFile>,
    ) -> Result<ServerMessage> {
        if let Err(refusal) = self.reaches(to)? {
            return Ok(reply(id, Err(refusal)));
        }
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        if knowledge::lodging(db, &layout, to).is_err() {
            return Ok(reply(
                id,
                Err(Refusal::Usage {
                    words: "Only the Hand and hires keep an inbox."
                        .into(),
                }),
            ));
        }
        let total = files.iter().map(|file| file.size).sum::<u64>();
        let bad = files.iter().find(|file| !is_plain(&file.path));
        let words = if files.is_empty() {
            Some("Give at least one file.".to_owned())
        } else if files.len() > MOST_FILES {
            Some(format!("A gift holds at most {MOST_FILES} files."))
        } else if total > MOST_BYTES {
            Some("A gift holds at most 32 MiB.".into())
        } else {
            bad.map(|file| {
                format!(
                    "`{}` is not a plain relative path.",
                    file.path
                )
            })
        };
        if let Some(words) = words {
            return Ok(reply(id, Err(Refusal::Usage { words })));
        }
        let upload = uuid::Uuid::new_v4().simple().to_string();
        Folder::make(&layout.staging().join(&upload))?;
        self.gifts.uploads.insert(
            upload.clone(),
            Upload {
                client,
                id: id.clone(),
                to,
                received: files
                    .iter()
                    .map(|file| (file.path.clone(), 0))
                    .collect(),
                files,
            },
        );
        let mut answer = reply(
            id,
            Ok::<_, Refusal>(
                "Ready for the gift's file parts.".into(),
            ),
        );
        if let ServerMessage::Reply(answer) = &mut answer {
            answer.upload = Some(upload);
        }
        Ok(answer)
    }

    /// Takes a part of a file gift, and delivers the gift once whole.
    pub(crate) async fn give_part(
        &mut self,
        client: u64,
        part: GivePart,
    ) -> Result<ServerMessage> {
        let refuse = |words: &str| {
            reply(
                &part.id,
                Err(Refusal::Usage {
                    words: words.into(),
                }),
            )
        };
        let Some(upload) = self
            .gifts
            .uploads
            .get_mut(&part.upload)
            .filter(|upload| upload.client == client)
        else {
            return Ok(refuse(
                "No gift of yours is being sent under that upload.",
            ));
        };
        let Some(size) = upload
            .files
            .iter()
            .find(|file| file.path == part.path)
            .map(|file| file.size)
        else {
            return Ok(refuse("That file is not part of the gift."));
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD
            .decode(&part.data)
        else {
            return Ok(refuse("A part's data is base64."));
        };
        let received =
            upload.received.get(&part.path).copied().unwrap_or(0);
        if part.offset != received {
            return Ok(refuse(&format!(
                "`{}` continues at byte {received}.",
                part.path
            )));
        }
        let end = received + bytes.len() as u64;
        if end > size {
            return Ok(refuse(&format!(
                "`{}` is longer than declared.",
                part.path
            )));
        }
        let staging =
            self.realm.layout().staging().join(&part.upload);
        let folder = Folder::make(&staging)?;
        let mut whole = if received == 0 {
            Vec::new()
        } else {
            folder.read(&part.path)?
        };
        whole.extend_from_slice(&bytes);
        folder.write(&part.path, &whole, false)?;
        upload.received.insert(part.path.clone(), end);
        let done = upload.files.iter().all(|file| {
            upload.received.get(&file.path) == Some(&file.size)
        });
        if !done {
            return Ok(reply(
                &part.id,
                Ok::<_, Refusal>("Received the gift part.".into()),
            ));
        }
        let upload = self
            .gifts
            .uploads
            .remove(&part.upload)
            .expect("the upload was found above");
        let words = self.hand_over(&part.upload, upload).await?;
        Ok(reply(&part.id, words))
    }

    /// Moves a whole gift into its agent's inbox and tells the agent.
    async fn hand_over(
        &mut self,
        name: &str,
        upload: Upload,
    ) -> Result<Result<String, Refusal>> {
        let layout = self.realm.layout().clone();
        let staging = layout.staging().join(name);
        let source = Folder::open(&staging)?;
        let db = self.realm.store().connection();
        let lodge = knowledge::lodging(db, &layout, upload.to)?.path;
        let inbox = Folder::make(&lodge)?;
        let mut paths = Vec::new();
        for file in &upload.files {
            let bytes = if file.size == 0 {
                Vec::new()
            } else {
                source.read(&file.path)?
            };
            inbox.write(
                &format!("inbox/{}", file.path),
                &bytes,
                false,
            )?;
            paths.push(format!("`inbox/{}`", file.path));
        }
        remove_folder(&staging)?;
        let paths = paths.join(", ");
        let line = self.credentials.scrub_text(&format!(
            "I handed you {paths} in your lodge. Read it and do \
             what it asks. If it asks for nothing, tell me in a few \
             lines what it is."
        ));
        let chief = self.realm.chief();
        let request = format!("gift:{}", upload.id.as_str());
        let to = upload.to;
        self.change(Actor::Chief(chief), |cx, env| {
            chats::say(cx, env, to, chief, &request, &line)
        })
        .await?;
        Ok(Ok(format!("Handed over {paths}.")))
    }

    /// Gives the skill `source` to the scholar `to`'s `main`, as this
    /// server's chief.
    pub(crate) async fn give_skill(
        &mut self,
        to: ScholarId,
        source: GiveSkillSource,
    ) -> Result<Result<String, Refusal>> {
        let chief = self.realm.chief();
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let Some(scholar) = city::find(db, to)? else {
            return Ok(Err(Refusal::NoScholar {
                name: to.to_string(),
            }));
        };
        if !authority::may_keep(db, Actor::Chief(chief), to)? {
            return Ok(Err(Refusal::ScholarNotOwn {
                name: scholar.name,
            }));
        }
        let (name, files) = match source {
            GiveSkillSource::Starter { starter } => {
                match library::starter(&starter) {
                    Some(files) => (starter, files),
                    None => {
                        return Ok(Err(Refusal::NoSkill {
                            name: starter,
                        }));
                    }
                }
            }
            GiveSkillSource::Library { library: name } => {
                let files = library::files(&layout, &name)?;
                if files.is_empty() {
                    return Ok(Err(Refusal::NoSkill { name }));
                }
                (name, files)
            }
            GiveSkillSource::From { from, name } => {
                let files = match from {
                    Keeper::Scholar(other) => {
                        let other =
                            ScholarId(i64::try_from(other.0)?);
                        if !authority::may_keep(
                            db,
                            Actor::Chief(chief),
                            other,
                        )? {
                            let name = city::get(db, other)?.name;
                            return Ok(Err(Refusal::ScholarNotOwn {
                                name,
                            }));
                        }
                        main_skill(&layout.repository(other), &name)?
                    }
                    Keeper::Agent(agent) => {
                        let agent = AgentId(i64::try_from(agent.0)?);
                        if let Err(refusal) = self.reaches(agent)? {
                            return Ok(Err(refusal));
                        }
                        let db = self.realm.store().connection();
                        let lodge =
                            knowledge::lodging(db, &layout, agent)?
                                .path;
                        lodge_skill(&lodge, &name)?
                    }
                };
                if files.is_empty() {
                    return Ok(Err(Refusal::NoSkill { name }));
                }
                (name, files)
            }
        };
        if !is_plain(&name) || name.contains('/') {
            return Ok(Err(Refusal::NoSkill { name }));
        }
        let changes = files
            .into_iter()
            .map(|(path, bytes)| GitChange::Write {
                path: format!("skills/{name}/{path}"),
                bytes,
            })
            .collect::<Vec<_>>();
        self.commit_main(
            Actor::Chief(chief),
            to,
            &changes,
            &format!("Gave the skill {name}"),
        )
        .await?;
        let db = self.realm.store().connection();
        let scholar = city::get(db, to)?.name;
        Ok(Ok(format!(
            "Gave {name} to {scholar}; its hires take it in once \
             idle."
        )))
    }

    /// `/give <scholar|address> <skill> [from <scholar|address>]`
    pub(crate) async fn give_command(
        &mut self,
        here: Option<&hamlet_store::ids::HamletId>,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let (target, skill, from) = match words {
            [target, skill] => (*target, *skill, None),
            [target, skill, "from", from] => {
                (*target, *skill, Some(*from))
            }
            _ => {
                return Ok(Err(Refusal::Usage {
                    words: "Use /give <scholar|address> <skill> \
                            [from <scholar|address>]"
                        .into(),
                }));
            }
        };
        let to = match self.scholar_of(here, target)? {
            Ok(to) => to,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let source = match from {
            None if library::starter(skill).is_some() => {
                GiveSkillSource::Starter {
                    starter: skill.into(),
                }
            }
            None => GiveSkillSource::Library {
                library: skill.into(),
            },
            Some(from) => {
                let db = self.realm.store().connection();
                let keeper = match city::named(db, from)? {
                    Some(found) => Keeper::Scholar(
                        hamlet_protocol::client::ScholarId(
                            u64::try_from(found.id.0)?,
                        ),
                    ),
                    None => {
                        let address = match addresses::parse(from) {
                            Ok(address) => address,
                            Err(refusal) => return Ok(Err(refusal)),
                        };
                        match addresses::resolve(
                            db,
                            &address,
                            here,
                            addresses::Reach::Living,
                        )? {
                            Ok(agent) => Keeper::Agent(
                                crate::clients::state::wire_agent(
                                    agent,
                                ),
                            ),
                            Err(refusal) => return Ok(Err(refusal)),
                        }
                    }
                };
                GiveSkillSource::From {
                    from: keeper,
                    name: skill.into(),
                }
            }
        };
        self.give_skill(to, source).await
    }

    /// The scholar `text` names: a scholar's name, else an agent's
    /// address, whose scholar it is.
    fn scholar_of(
        &mut self,
        here: Option<&hamlet_store::ids::HamletId>,
        text: &str,
    ) -> Result<Result<ScholarId, Refusal>> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        if let Some(found) = city::named(db, text)? {
            return Ok(Ok(found.id));
        }
        let address = match addresses::parse(text) {
            Ok(address) => address,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let agent = match addresses::resolve(
            db,
            &address,
            here,
            addresses::Reach::Living,
        )? {
            Ok(agent) => agent,
            Err(refusal) => return Ok(Err(refusal)),
        };
        Ok(match knowledge::lodging(db, &layout, agent) {
            Ok(lodging) => Ok(lodging.scholar),
            Err(_) => Err(Refusal::NoScholar { name: text.into() }),
        })
    }
}

/// The files of the skill `name` at the `main` of the repository at
/// `path`.
fn main_skill(
    path: &std::path::Path,
    name: &str,
) -> Result<Vec<(String, Vec<u8>)>> {
    let repo = Repository::open(path)?;
    let Some(tip) = repo.tip(MAIN)? else {
        return Ok(Vec::new());
    };
    let prefix = format!("skills/{name}/");
    repo.entries(tip)?
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path.strip_prefix(&prefix)?.to_owned();
            Some((path, entry.blob))
        })
        .map(|(path, blob)| Ok((path, repo.blob(blob)?)))
        .collect()
}

/// The files of the skill `name` in the lodge at `lodge`.
fn lodge_skill(
    lodge: &std::path::Path,
    name: &str,
) -> Result<Vec<(String, Vec<u8>)>> {
    let Ok(folder) = Folder::open(lodge) else {
        return Ok(Vec::new());
    };
    let prefix = format!("skills/{name}/");
    folder
        .list_under(&format!("skills/{name}"))?
        .into_iter()
        .map(|entry| {
            Ok((
                entry.path[prefix.len()..].to_owned(),
                folder.read(&entry.path)?,
            ))
        })
        .collect()
}
