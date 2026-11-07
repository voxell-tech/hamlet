//! What a chief edits and reads: the charter, characters, memory,
//! limits, settings, papers, the prompt and a chat's sessions.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_git::{Author, Change as GitChange, MAIN, Repository};
use hamlet_protocol::client::{
    self as protocol, Edit, EditLimits, Keeper, Read, ReadResult,
    RequestId, ServerMessage,
};
use hamlet_store::ids::{Actor, AgentId, ScholarId};
use hamlet_store::schema::document;

use super::check::reply;
use crate::core::Core;
use crate::jobs::{self, Job};
use crate::knowledge::folder::Folder;
use crate::knowledge::library::field;
use crate::knowledge::{self, CHARACTER, outside_git};
use crate::society::agents::Effort;
use crate::society::city::{self, CHARACTER_MOST};
use crate::society::limits::{self, Limits, RealmLimits};
use crate::society::refusal::Refusal;
use crate::society::{authority, hamlets, hires};
use crate::spend::micros;
use crate::talk::{chats, items, prompt};

/// Refuses a memory path that is not a plain relative path under
/// `memory/`.
fn memory_path(path: &str) -> Result<String, Refusal> {
    let path = path.trim().trim_start_matches("memory/");
    let plain = !path.is_empty()
        && !path.starts_with('/')
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".."
        });
    if plain {
        Ok(format!("memory/{path}"))
    } else {
        Err(Refusal::NoFile { path: path.into() })
    }
}

impl Core {
    pub(crate) async fn edit(
        &mut self,
        client: u64,
        edit: Edit,
    ) -> Result<()> {
        let id = edit_id(&edit).clone();
        let answer = self.edited(edit).await?;
        self.answer(client, reply(&id, answer))
    }

    async fn edited(
        &mut self,
        edit: Edit,
    ) -> Result<Result<String, Refusal>> {
        let chief = self.realm.chief();
        let at = crate::clock::now();
        match edit {
            Edit::Charter { text, .. } => {
                let db = self.realm.store().connection();
                match text.filter(|t| !t.trim().is_empty()) {
                    Some(text) => {
                        diesel::replace_into(document::table)
                            .values((
                                document::kind.eq("charter"),
                                document::value.eq(&text),
                                document::at.eq(at),
                                document::chief.eq(chief),
                            ))
                            .execute(db)?;
                    }
                    None => {
                        diesel::delete(
                            document::table.find("charter"),
                        )
                        .execute(db)?;
                    }
                }
                self.publish().await?;
                Ok(Ok("The charter is saved.".into()))
            }
            Edit::Character { scholar, text, .. } => {
                let scholar = ScholarId(i64::try_from(scholar.0)?);
                if text.chars().count() > CHARACTER_MOST {
                    return Ok(Err(Refusal::LongCharacter));
                }
                if let Err(refusal) = self.keeps(scholar)? {
                    return Ok(Err(refusal));
                }
                self.commit_main(
                    Actor::Chief(chief),
                    scholar,
                    &[GitChange::Write {
                        path: CHARACTER.into(),
                        bytes: text.into_bytes(),
                    }],
                    "Edited the character",
                )
                .await?;
                Ok(Ok("The character is saved.".into()))
            }
            Edit::Memory { of, path, text, .. } => {
                let path = match memory_path(&path) {
                    Ok(path) => path,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                match of {
                    Keeper::Scholar(scholar) => {
                        let scholar =
                            ScholarId(i64::try_from(scholar.0)?);
                        if let Err(refusal) = self.keeps(scholar)? {
                            return Ok(Err(refusal));
                        }
                        let change = if text.is_empty() {
                            GitChange::Remove { path }
                        } else {
                            GitChange::Write {
                                path,
                                bytes: text.into_bytes(),
                            }
                        };
                        self.commit_main(
                            Actor::Chief(chief),
                            scholar,
                            &[change],
                            "Edited the memory",
                        )
                        .await?;
                        Ok(Ok("The memory is saved.".into()))
                    }
                    Keeper::Agent(agent) => {
                        let agent = AgentId(i64::try_from(agent.0)?);
                        if let Err(refusal) = self.reaches(agent)? {
                            return Ok(Err(refusal));
                        }
                        let layout = self.realm.layout().clone();
                        let lodging = knowledge::lodging(
                            self.realm.store().connection(),
                            &layout,
                            agent,
                        )?;
                        let writing = self.writing.clone();
                        let held = writing.lock().unwrap_or_else(
                            std::sync::PoisonError::into_inner,
                        );
                        let folder = Folder::make(&lodging.path)?;
                        if text.is_empty() {
                            folder.remove(&path)?;
                        } else {
                            folder.write(
                                &path,
                                text.as_bytes(),
                                false,
                            )?;
                        }
                        drop(held);
                        self.push_lodge(agent).await?;
                        self.change(Actor::Chief(chief), |cx, _| {
                            jobs::add(
                                cx,
                                &Job::Commit { hire: agent },
                            )?;
                            Ok(())
                        })
                        .await?;
                        Ok(Ok("The memory is saved.".into()))
                    }
                }
            }
            Edit::Limits(EditLimits::Whole {
                hamlet, whole, ..
            }) => {
                let value = serde_json::Value::Object(
                    whole.into_iter().collect(),
                );
                self.set_limits(hamlet, |_| Ok(value)).await
            }
            Edit::Limits(EditLimits::Set { hamlet, set, .. }) => {
                self.set_limits(hamlet, |mut current| {
                    let section = current
                        .get_mut(&set.section)
                        .and_then(serde_json::Value::as_object_mut);
                    match section {
                        Some(section) => {
                            section.insert(
                                set.key.clone(),
                                set.value.clone(),
                            );
                        }
                        None => {
                            current[&set.key] = set.value.clone();
                        }
                    }
                    Ok(current)
                })
                .await
            }
            Edit::Settings { set, .. } => {
                let db = self.realm.store().connection();
                let mut settings = crate::settings::stored(db)?;
                if let Some(model) = set.model {
                    settings.model = model;
                }
                if let Some(every) = set.reflect_every {
                    let Ok(every) = u64::try_from(every) else {
                        return Ok(Err(Refusal::Usage {
                            words: "Reflect after zero or more \
                                    turns."
                                .into(),
                        }));
                    };
                    settings.reflect_every = every;
                }
                if let Some(net) = set.allow_net {
                    settings.allow_net = net;
                }
                if let Some(effort) = set.effort {
                    settings.effort = effort.map(Effort::of);
                }
                if let Some(newcomers) = set.newcomers {
                    settings.newcomers = newcomers;
                }
                if let Some(character) = set.character {
                    if character.trim().is_empty() {
                        return Ok(Err(Refusal::Usage {
                            words: "A blank scholar needs a \
                                    character to start from."
                                .into(),
                        }));
                    }
                    if character.chars().count() > CHARACTER_MOST {
                        return Ok(Err(Refusal::LongCharacter));
                    }
                    settings.character = character;
                }
                let mut removed = Vec::new();
                if let Some(endpoints) = set.endpoints {
                    let endpoints = endpoints
                        .into_iter()
                        .map(|e| crate::settings::Endpoint {
                            name: e.name.trim().to_owned(),
                            base_url: e.base_url.trim().to_owned(),
                        })
                        .collect::<Vec<_>>();
                    if let Some(words) = endpoint_fault(&endpoints) {
                        return Ok(Err(Refusal::Usage { words }));
                    }
                    removed = settings
                        .endpoints
                        .iter()
                        .filter(|old| {
                            !endpoints
                                .iter()
                                .any(|e| e.name == old.name)
                        })
                        .map(|old| old.name.clone())
                        .collect();
                    settings.endpoints = endpoints;
                }
                settings.saved = true;
                let db = self.realm.store().connection();
                crate::settings::keep(
                    db,
                    &settings,
                    at,
                    Some(chief),
                )?;
                for name in removed {
                    self.credentials.save_model(&name, None)?;
                }
                if let Some(chosen) = set.workspace {
                    let chosen = match chosen {
                        Some(workspace) => {
                            let address = crate::settings::Endpoint {
                                name: "workspace".into(),
                                base_url: workspace
                                    .url
                                    .trim()
                                    .to_owned(),
                            };
                            if let Some(words) = endpoint_fault(
                                std::slice::from_ref(&address),
                            ) {
                                return Ok(Err(Refusal::Usage {
                                    words,
                                }));
                            }
                            Some(crate::credentials::WorkspaceAddress {
                                url: address.base_url,
                                token: workspace.token,
                            })
                        }
                        None => None,
                    };
                    self.credentials.choose_workspace(chosen)?;
                }
                self.env.settings =
                    self.env.overrides.apply(settings);
                self.publish().await?;
                Ok(Ok("The settings are saved.".into()))
            }
            Edit::Catalogue { model, set, .. } => {
                let answer = self.edit_catalogue(&model, set)?;
                self.publish().await?;
                Ok(answer)
            }
        }
    }

    /// Replaces the limits of `hamlet`, or the realm's, with what
    /// `edit` makes of their client form.
    async fn set_limits(
        &mut self,
        hamlet: Option<String>,
        edit: impl FnOnce(serde_json::Value) -> Result<serde_json::Value>,
    ) -> Result<Result<String, Refusal>> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        match hamlet {
            Some(hamlet) => {
                let id = super::state::hamlet_id(&hamlet);
                if hamlets::find(db, &id)?.is_none() {
                    return Ok(Err(Refusal::NoHamlet { id: hamlet }));
                }
                if !authority::is_chief_of(db, chief, &id)? {
                    return Ok(Err(Refusal::NotChief { hamlet }));
                }
                let current = super::state::hamlet_limits(
                    &limits::of(db, &id)?,
                );
                let edited = edit(serde_json::to_value(current)?)?;
                let Ok(edited) = serde_json::from_value::<
                    protocol::Limits,
                >(edited) else {
                    return Ok(Err(Refusal::Usage {
                        words: "Those limits do not fit their form."
                            .into(),
                    }));
                };
                let own = limits_of(&edited);
                if let Some(words) = own.fault() {
                    return Ok(Err(Refusal::Usage { words }));
                }
                let done = self
                    .attempt(Actor::Chief(chief), |cx, _| {
                        hamlets::set_limits(cx, &id, &own)
                    })
                    .await?;
                Ok(done.map(|()| {
                    format!("The limits of `{hamlet}` are saved.")
                }))
            }
            None => {
                let current =
                    super::state::realm_limits(&limits::realm(db)?);
                let edited = edit(serde_json::to_value(current)?)?;
                let Ok(edited) = serde_json::from_value::<
                    protocol::RealmLimits,
                >(edited) else {
                    return Ok(Err(Refusal::Usage {
                        words: "Those limits do not fit their form."
                            .into(),
                    }));
                };
                let own = RealmLimits {
                    roads_between: edited.roads_between,
                    budget: budget_of(&edited.budget),
                };
                if let Some(words) = own.budget.fault() {
                    return Ok(Err(Refusal::Usage { words }));
                }
                diesel::replace_into(document::table)
                    .values((
                        document::kind.eq("limits"),
                        document::value
                            .eq(serde_json::to_string(&own)?),
                        document::at.eq(crate::clock::now()),
                        document::chief.eq(chief),
                    ))
                    .execute(db)?;
                self.publish().await?;
                Ok(Ok("The realm's limits are saved.".into()))
            }
        }
    }

    /// Commits `changes` to the `main` of `scholar` as `by`'s, and
    /// has its idle hires take them in.
    pub(crate) async fn commit_main(
        &mut self,
        by: Actor,
        scholar: ScholarId,
        changes: &[GitChange],
        message: &str,
    ) -> Result<()> {
        let path = self.realm.layout().repository(scholar);
        let author = by.to_string();
        let writing = self.writing.clone();
        let held = writing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let repo = Repository::open(&path)?;
        repo.commit(
            MAIN,
            changes,
            message,
            Author {
                name: &author,
                at: crate::clock::now(),
            },
        )?;
        drop(held);
        self.main_moved(by, scholar).await
    }

    /// Has the idle hires of `scholar` take in its `main`, which `by`
    /// moved.
    pub(crate) async fn main_moved(
        &mut self,
        by: Actor,
        scholar: ScholarId,
    ) -> Result<()> {
        self.change(by, |cx, _| {
            for hire in hires::living_of(cx.db, scholar)? {
                if !crate::talk::inputs::is_agent_busy(
                    cx.db, hire.agent,
                )? {
                    jobs::add(cx, &Job::TakeIn { hire: hire.agent })?;
                }
            }
            cx.touched.scholars.insert(scholar);
            Ok(())
        })
        .await?;
        Ok(())
    }

    /// Refuses unless this server's chief keeps `scholar`.
    fn keeps(
        &mut self,
        scholar: ScholarId,
    ) -> Result<Result<(), Refusal>> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        let Some(found) = city::find(db, scholar)? else {
            return Ok(Err(Refusal::NoScholar {
                address: format!("scholar#{scholar}"),
            }));
        };
        if !found.is_living() {
            return Ok(Err(Refusal::ScholarGone {
                name: found.label(),
            }));
        }
        if !authority::may_keep(db, Actor::Chief(chief), scholar)? {
            return Ok(Err(Refusal::NotOwner {
                owner: found.label(),
            }));
        }
        Ok(Ok(()))
    }

    pub(crate) async fn read(
        &mut self,
        client: u64,
        read: Read,
    ) -> Result<()> {
        let chief = self.realm.chief();
        let message = match read {
            Read::Charter { id } => {
                let db = self.realm.store().connection();
                let own = document::table
                    .find("charter")
                    .count()
                    .get_result::<i64>(db)?
                    > 0;
                ServerMessage::Read(ReadResult::Charter {
                    charter: protocol::Charter {
                        own,
                        text: prompt::charter(db)?,
                    },
                    to: id,
                })
            }
            Read::Prompt { agent, id } => {
                let agent = AgentId(i64::try_from(agent.0)?);
                if let Err(refusal) = self.reaches(agent)? {
                    return self
                        .answer(client, reply(&id, Err(refusal)));
                }
                let conversation = self
                    .change(Actor::Chief(chief), |cx, env| {
                        chats::current(cx, env, agent, chief)
                    })
                    .await?;
                let db = self.realm.store().connection();
                let kept = crate::talk::conversations::get(
                    db,
                    conversation,
                )?;
                let sections = self.prompt(&kept, None)?;
                let text = sections
                    .into_iter()
                    .map(|s| s.text)
                    .collect::<Vec<_>>()
                    .join("\n\n");
                ServerMessage::Read(ReadResult::Prompt {
                    prompt: text,
                    to: id,
                })
            }
            Read::History { agent, id } => {
                let agent = AgentId(i64::try_from(agent.0)?);
                if let Err(refusal) = self.reaches(agent)? {
                    return self
                        .answer(client, reply(&id, Err(refusal)));
                }
                let history = self.pasts(agent)?;
                ServerMessage::Read(ReadResult::History {
                    history,
                    to: id,
                })
            }
            Read::Models { id, .. } => self.catalogue_read(&id)?,
            Read::Papers { of, id, .. } => match self.papers(of)? {
                Ok(papers) => {
                    ServerMessage::Read(ReadResult::Papers {
                        papers,
                        to: id,
                    })
                }
                Err(refusal) => reply(&id, Err(refusal)),
            },
        };
        self.answer(client, message)
    }

    /// What `of` knows: a scholar's `main`, or an agent's lodge.
    fn papers(
        &mut self,
        of: Keeper,
    ) -> Result<Result<protocol::Papers, Refusal>> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let (scholar, lodge) = match of {
            Keeper::Scholar(scholar) => {
                (ScholarId(i64::try_from(scholar.0)?), None)
            }
            Keeper::Agent(agent) => {
                let agent = AgentId(i64::try_from(agent.0)?);
                let Ok(lodging) =
                    knowledge::lodging(db, &layout, agent)
                else {
                    return Ok(Err(Refusal::NoOne {
                        address: agent.to_string(),
                    }));
                };
                (lodging.scholar, Some(lodging.path))
            }
        };
        let repo = Repository::open(&layout.repository(scholar))?;
        let main = repo.tip(MAIN)?;
        let character = match main {
            Some(tip) => repo
                .read(tip, CHARACTER)?
                .map(|bytes| {
                    String::from_utf8_lossy(&bytes).into_owned()
                })
                .unwrap_or_default(),
            None => String::new(),
        };
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut learned = Vec::new();
        match &lodge {
            Some(path) => {
                let Ok(folder) = Folder::open(path) else {
                    return Ok(Ok(protocol::Papers {
                        character,
                        learned,
                        memory: Vec::new(),
                        skills: Vec::new(),
                    }));
                };
                let listing = folder.list(&outside_git)?;
                let base = match main {
                    Some(tip) => repo.entries(tip)?,
                    None => Vec::new(),
                };
                for entry in &listing {
                    if !base.contains(entry) {
                        learned.push(entry.path.clone());
                    }
                    files.push((
                        entry.path.clone(),
                        folder.read(&entry.path)?,
                    ));
                }
                for entry in &base {
                    if !outside_git(&entry.path)
                        && !listing
                            .iter()
                            .any(|e| e.path == entry.path)
                    {
                        learned.push(entry.path.clone());
                    }
                }
            }
            None => {
                if let Some(tip) = main {
                    files = repo.files(tip)?;
                }
            }
        }
        let mut memory = Vec::new();
        let mut skills = Vec::new();
        for (path, bytes) in files {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if let Some(rest) = path.strip_prefix("memory/") {
                memory.push(protocol::MemoryFile {
                    description: field(&text, "description:"),
                    path: rest.to_owned(),
                    text,
                });
            } else if let Some(rest) = path.strip_prefix("skills/")
                && let Some((name, "SKILL.md")) = rest.split_once('/')
            {
                skills.push(protocol::Skill {
                    description: field(&text, "description:"),
                    name: name.to_owned(),
                });
            }
        }
        learned.sort();
        Ok(Ok(protocol::Papers {
            character,
            learned,
            memory,
            skills,
        }))
    }

    /// The sessions of the chief's chat with `agent` that hold a
    /// line, newest first, each named by its first line (C24).
    fn pasts(
        &mut self,
        agent: AgentId,
    ) -> Result<Vec<protocol::Past>> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        let current = chats::current_number(db, agent, chief)?;
        let name = crate::society::chiefs::find(db, chief)?
            .map(|c| c.name)
            .unwrap_or_default();
        let mut out = Vec::new();
        for (number, conversation, began) in
            chats::sessions(db, agent, chief)?
        {
            let page = items::page(db, conversation, None, 10_000)?;
            let mut first = None;
            let mut messages = 0_i64;
            let mut modified = began;
            for (_, item) in &page {
                modified =
                    modified.max(i64::try_from(item.at).unwrap_or(0));
                if let Ok(Some(crate::talk::marks::Mark::Chief {
                    text,
                    ..
                })) = crate::talk::marks::Mark::of_item(
                    item.mark.as_ref(),
                ) {
                    messages += 1;
                    if first.is_none() && !text.trim().is_empty() {
                        first = Some(text);
                    }
                } else if item.kind
                    == hamlet_protocol::runtime::ItemKind::Assistant
                {
                    messages += 1;
                }
            }
            let Some(first) = first else {
                continue;
            };
            let line = first
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or_default()
                .trim();
            let mut title = line.chars().take(90).collect::<String>();
            if line.chars().count() > 90 {
                title.push_str("...");
            }
            out.push(protocol::Past {
                began,
                by: name.clone(),
                current: Some(number) == current,
                messages,
                modified,
                number,
                title,
                tokens: 0,
            });
        }
        out.sort_by_key(|past| std::cmp::Reverse(past.number));
        Ok(out)
    }
}

fn edit_id(edit: &Edit) -> &RequestId {
    match edit {
        Edit::Charter { id, .. }
        | Edit::Character { id, .. }
        | Edit::Memory { id, .. }
        | Edit::Limits(
            EditLimits::Set { id, .. } | EditLimits::Whole { id, .. },
        )
        | Edit::Settings { id, .. }
        | Edit::Catalogue { id, .. } => id,
    }
}

/// What is wrong with `endpoints`, in words; none when they hold. A
/// key goes in a sign-in, never in an address.
fn endpoint_fault(
    endpoints: &[crate::settings::Endpoint],
) -> Option<String> {
    for (at, endpoint) in endpoints.iter().enumerate() {
        if endpoint.name.is_empty() || endpoint.name.contains("::") {
            return Some(
                "An endpoint has a name without `::`.".into(),
            );
        }
        if endpoints[..at].iter().any(|e| e.name == endpoint.name) {
            return Some(format!(
                "Two endpoints are named `{}`.",
                endpoint.name
            ));
        }
        let Ok(url) = url::Url::parse(&endpoint.base_url) else {
            return Some(format!(
                "`{}` is not an address.",
                endpoint.base_url
            ));
        };
        if !matches!(url.scheme(), "http" | "https") {
            return Some(format!(
                "`{}` is not an http or https address.",
                endpoint.base_url
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Some(
                "An address may not hold a name or a key; sign in \
                 to the endpoint with its key instead."
                    .into(),
            );
        }
    }
    None
}

fn budget_of(budget: &protocol::LimitsBudget) -> limits::Budget {
    limits::Budget {
        cost: budget.cost.map(micros),
        minutes: budget.minutes,
        period: match budget.period {
            protocol::Period::Day => limits::Period::Day,
            protocol::Period::Month => limits::Period::Month,
        },
    }
}

/// A hamlet's limits from their client form.
fn limits_of(shown: &protocol::Limits) -> Limits {
    Limits {
        budget: budget_of(&shown.budget),
        task: limits::TaskCaps {
            cost: shown.task.cost.map(micros),
            minutes: shown.task.minutes,
            messages: shown.task.messages,
        },
        road: limits::RoadRules {
            price: micros(shown.road.price),
            most: shown.road.most,
            grows_over_days: shown.road.grows_over_days,
        },
        reserved: limits::Reserved {
            hire: shown.reserved.hire,
            roads: shown.reserved.roads,
            end: shown.reserved.end,
            merge: shown.reserved.merge,
        },
        model: shown.model.clone(),
        depth: shown.depth,
        told_days: shown.told_days,
        asking: limits::Asking {
            questions: shown.asking.questions,
            refusals: shown.asking.refusals,
            review: match shown.asking.review {
                protocol::LimitsReview::Always => {
                    limits::Review::Always
                }
                protocol::LimitsReview::Remote => {
                    limits::Review::Remote
                }
                protocol::LimitsReview::Never => {
                    limits::Review::Never
                }
            },
        },
        protected_minutes: shown.protected_minutes,
    }
}
