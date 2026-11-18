//! The documents a chief edits and reads: the charter, characters,
//! memory, limits, settings, papers, the prompt and a chat's
//! sessions.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_git::{Author, Change as GitChange, MAIN, Repository};
use hamlet_protocol::client::{
    self as protocol, Edit, EditLimits, EditSettingsSet, Keeper,
    Read, ReadResult, RequestId, ServerMessage,
};
use hamlet_store::ids::{Actor, AgentId, ProfileId};
use hamlet_store::schema::document;

use super::check::reply;
use crate::conversations::{chats, items, prompt};
use crate::core::{Core, UnpushedWorktree};
use crate::knowledge::folder::Folder;
use crate::knowledge::library::field;
use crate::knowledge::{self, CHARACTER, outside_git};
use crate::realm::change::{Done, done, refuse};
use crate::realm::jobs::{self, Job};
use crate::society::agents::Effort;
use crate::society::city::{self, CHARACTER_MOST};
use crate::society::limits::{self, Limits, RealmLimits};
use crate::society::refusal::Refusal;
use crate::society::spend::micros;
use crate::society::{authority, hamlet_agents, hamlets};

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

impl Core<'_, '_> {
    pub(crate) fn edit(
        &mut self,
        client: u64,
        edit: Edit,
    ) -> Result<()> {
        let id = edit_id(&edit).clone();
        let answer = self.edited(edit)?;
        self.answer(client, reply(&id, answer))
    }

    /// Makes the chief's edit of one document.
    fn edited(&mut self, edit: Edit) -> Done<String> {
        match edit {
            Edit::Charter { text, .. } => self.edit_charter(text),
            Edit::Character { profile, text, .. } => {
                let profile = ProfileId(i64::try_from(profile.0)?);
                self.edit_character(profile, text)
            }
            Edit::Memory { of, path, text, .. } => {
                let path = match memory_path(&path) {
                    Ok(path) => path,
                    Err(refusal) => refuse!(refusal),
                };
                match of {
                    Keeper::Profile(profile) => {
                        let profile =
                            ProfileId(i64::try_from(profile.0)?);
                        self.edit_profile_memory(profile, path, text)
                    }
                    Keeper::Agent(agent) => {
                        let agent = AgentId(i64::try_from(agent.0)?);
                        self.edit_worktree_memory(agent, &path, &text)
                    }
                }
            }
            Edit::Limits(EditLimits::Whole {
                hamlet, whole, ..
            }) => {
                let value = serde_json::Value::Object(
                    whole.into_iter().collect(),
                );
                self.set_limits(LimitsOwner::of(hamlet), |_| {
                    Ok(value)
                })
            }
            Edit::Limits(EditLimits::Set { hamlet, set, .. }) => self
                .set_limits(
                    LimitsOwner::of(hamlet),
                    |mut current| {
                        let section =
                            current.get_mut(&set.section).and_then(
                                serde_json::Value::as_object_mut,
                            );
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
                    },
                ),
            Edit::Settings { set, .. } => self.edit_settings(set),
            Edit::Catalogue { model, set, .. } => {
                let answer = self.edit_catalogue(&model, set)?;
                self.publish();
                Ok(answer)
            }
        }
    }

    /// Keeps the realm's charter, or none when `text` is blank.
    fn edit_charter(&mut self, text: Option<String>) -> Done<String> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        match text.filter(|t| !t.trim().is_empty()) {
            Some(text) => {
                diesel::replace_into(document::table)
                    .values((
                        document::kind.eq("charter"),
                        document::value.eq(&text),
                        document::at.eq(crate::app::clock::now()),
                        document::chief.eq(chief),
                    ))
                    .execute(db)?;
            }
            None => {
                diesel::delete(document::table.find("charter"))
                    .execute(db)?;
            }
        }
        self.publish();
        Ok(Ok("The charter is saved.".into()))
    }

    /// Commits `text` as the character of `profile`.
    fn edit_character(
        &mut self,
        profile: ProfileId,
        text: String,
    ) -> Done<String> {
        if text.chars().count() > CHARACTER_MOST {
            refuse!(Refusal::LongCharacter);
        }
        done!(self.keeps(profile));
        let by = Actor::Chief(self.realm.chief());
        self.commit_main(
            by,
            profile,
            &[GitChange::Write {
                path: CHARACTER.into(),
                bytes: text.into_bytes(),
            }],
            "Edited the character",
        )?;
        Ok(Ok("The character is saved.".into()))
    }

    /// Commits a memory file of `profile`'s `main`; empty removes it.
    fn edit_profile_memory(
        &mut self,
        profile: ProfileId,
        path: String,
        text: String,
    ) -> Done<String> {
        done!(self.keeps(profile));
        let change = if text.is_empty() {
            GitChange::Remove { path }
        } else {
            GitChange::Write {
                path,
                bytes: text.into_bytes(),
            }
        };
        let by = Actor::Chief(self.realm.chief());
        self.commit_main(
            by,
            profile,
            &[change],
            "Edited the memory",
        )?;
        Ok(Ok("The memory is saved.".into()))
    }

    /// Writes a memory file of `agent`'s worktree, empty removing it,
    /// and has the worktree committed.
    fn edit_worktree_memory(
        &mut self,
        agent: AgentId,
        path: &str,
        text: &str,
    ) -> Done<String> {
        done!(self.reaches(agent));
        let layout = self.realm.layout().clone();
        let worktree = knowledge::worktree(
            self.realm.store().connection(),
            &layout,
            agent,
        )?;
        {
            let writing = self.writing.clone();
            let _held = writing.hold();
            let folder = Folder::make(&worktree.path)?;
            if text.is_empty() {
                folder.remove(path)?;
            } else {
                folder.write(path, text.as_bytes(), false)?;
            }
        }
        self.remote.unpushed.write(UnpushedWorktree(agent));
        let by = Actor::Chief(self.realm.chief());
        self.change(by, |cx, _| {
            jobs::add(cx, &Job::Commit { agent })?;
            Ok(())
        })?;
        Ok(Ok("The memory is saved.".into()))
    }

    /// Keeps the settings `set` changes, and the workspace it
    /// chooses.
    fn edit_settings(
        &mut self,
        set: EditSettingsSet,
    ) -> Done<String> {
        let db = self.realm.store().connection();
        let mut settings = crate::realm::settings::stored(db)?;
        if let Some(model) = set.model {
            settings.model = model;
            settings.picked = true;
        }
        if let Some(every) = set.reflect_every {
            let Ok(every) = u64::try_from(every) else {
                refuse!(Refusal::BadReflectEvery);
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
                refuse!(Refusal::NoCharacter);
            }
            if character.chars().count() > CHARACTER_MOST {
                refuse!(Refusal::LongCharacter);
            }
            settings.character = character;
        }
        let mut removed = Vec::new();
        if let Some(endpoints) = set.endpoints {
            let endpoints = endpoints
                .into_iter()
                .map(|e| crate::realm::settings::Endpoint {
                    name: e.name.trim().to_owned(),
                    base_url: e.base_url.trim().to_owned(),
                })
                .collect::<Vec<_>>();
            if let Some(why) = endpoint_fault(&endpoints) {
                refuse!(Refusal::BadEndpoint { why });
            }
            removed = settings
                .endpoints
                .iter()
                .filter(|old| {
                    !endpoints.iter().any(|e| e.name == old.name)
                })
                .map(|old| old.name.clone())
                .collect();
            settings.endpoints = endpoints;
        }
        let chosen = match set.workspace {
            None => None,
            Some(None) => Some(None),
            Some(Some(workspace)) => {
                let address = crate::realm::settings::Endpoint {
                    name: "workspace".into(),
                    base_url: workspace.url.trim().to_owned(),
                };
                if let Some(why) =
                    endpoint_fault(std::slice::from_ref(&address))
                {
                    refuse!(Refusal::BadEndpoint { why });
                }
                Some(Some(
                    crate::realm::credentials::WorkspaceAddress {
                        url: address.base_url,
                        token: workspace.token,
                    },
                ))
            }
        };
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        crate::realm::settings::keep(
            db,
            &settings,
            crate::app::clock::now(),
            Some(chief),
        )?;
        for name in removed {
            self.credentials.save_model(&name, None)?;
        }
        if let Some(chosen) = chosen {
            self.credentials.choose_workspace(chosen)?;
        }
        self.env.settings = self.env.overrides.apply(settings);
        self.publish();
        Ok(Ok("The settings are saved.".into()))
    }

    /// Replaces the limits of `owner` with what `edit` makes of their
    /// client form.
    fn set_limits(
        &mut self,
        owner: LimitsOwner,
        edit: impl FnOnce(serde_json::Value) -> Result<serde_json::Value>,
    ) -> Done<String> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        match owner {
            LimitsOwner::Hamlet(hamlet) => {
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
                    return Ok(Err(Refusal::LimitsUnfit));
                };
                let own = limits_of(&edited);
                if let Some(why) = own.fault() {
                    return Ok(Err(Refusal::BadLimits { why }));
                }
                let done = self
                    .attempt(Actor::Chief(chief), |cx, _| {
                        hamlets::set_limits(cx, &id, &own)
                    })?;
                Ok(done.map(|()| {
                    format!("The limits of `{hamlet}` are saved.")
                }))
            }
            LimitsOwner::Realm => {
                let current =
                    super::state::realm_limits(&limits::realm(db)?);
                let edited = edit(serde_json::to_value(current)?)?;
                let Ok(edited) = serde_json::from_value::<
                    protocol::RealmLimits,
                >(edited) else {
                    return Ok(Err(Refusal::LimitsUnfit));
                };
                let own = RealmLimits {
                    roads_between: edited.roads_between,
                    budget: budget_of(&edited.budget),
                };
                if let Some(why) = own.budget.fault() {
                    return Ok(Err(Refusal::BadLimits { why }));
                }
                diesel::replace_into(document::table)
                    .values((
                        document::kind.eq("limits"),
                        document::value
                            .eq(serde_json::to_string(&own)?),
                        document::at.eq(crate::app::clock::now()),
                        document::chief.eq(chief),
                    ))
                    .execute(db)?;
                self.publish();
                Ok(Ok("The realm's limits are saved.".into()))
            }
        }
    }

    /// Commits `changes` to the `main` of `profile` as `by`'s, and
    /// has its idle agents take them in.
    pub(crate) fn commit_main(
        &mut self,
        by: Actor,
        profile: ProfileId,
        changes: &[GitChange],
        message: &str,
    ) -> Result<()> {
        let path = self.realm.layout().repository(profile);
        let author = by.to_string();
        let writing = self.writing.clone();
        let held = writing.hold();
        let repo = Repository::open(&path)?;
        repo.commit(
            MAIN,
            changes,
            message,
            Author {
                name: &author,
                at: crate::app::clock::now(),
            },
        )?;
        drop(held);
        self.main_moved(by, profile)
    }

    /// Has the idle agents of `profile` take in its `main`, which
    /// `by` moved.
    pub(crate) fn main_moved(
        &mut self,
        by: Actor,
        profile: ProfileId,
    ) -> Result<()> {
        self.change(by, |cx, _| {
            for agent in hamlet_agents::living_of(cx.db, profile)? {
                if !crate::conversations::inputs::is_agent_busy(
                    cx.db,
                    agent.agent,
                )? {
                    jobs::add(
                        cx,
                        &Job::TakeIn { agent: agent.agent },
                    )?;
                }
            }
            cx.touched.profiles.insert(profile);
            Ok(())
        })?;
        Ok(())
    }

    /// Refuses unless this server's chief keeps `profile`.
    fn keeps(&mut self, profile: ProfileId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        let Some(found) = city::find(db, profile)? else {
            return Ok(Err(Refusal::NoProfile {
                address: format!("profile#{profile}"),
            }));
        };
        if !found.is_living() {
            return Ok(Err(Refusal::ProfileGone {
                name: found.label(),
            }));
        }
        if !authority::may_keep(db, Actor::Chief(chief), profile)? {
            return Ok(Err(Refusal::NotOwner {
                owner: found.label(),
            }));
        }
        Ok(Ok(()))
    }

    pub(crate) fn read(
        &mut self,
        client: u64,
        read: Read,
    ) -> Result<()> {
        let chief = self.realm.chief();
        let message = match read {
            Read::Image { id, image, size } => {
                // Read off the loop; the image is its own input.
                self.read_image(
                    client,
                    &id,
                    &image,
                    size.unwrap_or_default(),
                );
                return Ok(());
            }
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
                    })?;
                let db = self.realm.store().connection();
                let kept =
                    crate::conversations::get(db, conversation)?;
                let prompt = self
                    .prompt(&kept)?
                    .into_iter()
                    .map(|s| protocol::PromptSection {
                        name: s.name,
                        text: s.text,
                    })
                    .collect();
                ServerMessage::Read(ReadResult::Prompt {
                    prompt,
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

    /// The papers of `of`: a profile's `main`, or an agent's
    /// worktree.
    fn papers(&mut self, of: Keeper) -> Done<protocol::Papers> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let (profile, worktree) = match of {
            Keeper::Profile(profile) => {
                (ProfileId(i64::try_from(profile.0)?), None)
            }
            Keeper::Agent(agent) => {
                let agent = AgentId(i64::try_from(agent.0)?);
                let Ok(worktree) =
                    knowledge::worktree(db, &layout, agent)
                else {
                    return Ok(Err(Refusal::NoOne {
                        address: agent.to_string(),
                    }));
                };
                (worktree.profile, Some(worktree.path))
            }
        };
        let repo = Repository::open(&layout.repository(profile))?;
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
        let mut files = Vec::<(String, Vec<u8>)>::new();
        let mut learned = Vec::new();
        match &worktree {
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
            let mut page =
                items::page(db, conversation, None, 10_000)?;
            if page.is_empty() {
                page = crate::conversations::imports::held(
                    db,
                    conversation,
                )?
                .into_iter()
                .enumerate()
                .map(|(n, item)| (n as i64, item))
                .collect();
            }
            let mut first = None;
            let mut messages = 0_i64;
            let mut modified = began;
            for (_, item) in &page {
                use crate::conversations::marks::Mark;
                modified =
                    modified.max(i64::try_from(item.at).unwrap_or(0));
                let said = match Mark::of_item(item.mark.as_ref()) {
                    Ok(Some(Mark::Chief { text, .. })) => Some(text),
                    // A message of the chief's brought in from
                    // another program.
                    Ok(Some(Mark::Earlier { .. })) => {
                        Some(item.text.clone())
                    }
                    _ => None,
                };
                if let Some(text) = said {
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

/// Whose limits an edit sets.
enum LimitsOwner {
    Hamlet(String),
    Realm,
}

impl LimitsOwner {
    /// The owner of the limits `hamlet` names, or the realm's when
    /// none.
    fn of(hamlet: Option<String>) -> Self {
        hamlet.map_or(Self::Realm, Self::Hamlet)
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

/// The fault of `endpoints`, in words; none when they hold. A key
/// goes in a sign-in, never in an address.
fn endpoint_fault(
    endpoints: &[crate::realm::settings::Endpoint],
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
        road: limits::RoadRules {
            price: micros(shown.road.price),
            most: shown.road.most,
            grows_over_days: shown.road.grows_over_days,
        },
        reserved: limits::Reserved {
            add: shown.reserved.add,
            roads: shown.reserved.roads,
            end: shown.reserved.end,
            merge: shown.reserved.merge,
        },
        model: shown.model.clone(),
        told_days: shown.told_days,
        asking: limits::ChiefQuestions {
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
