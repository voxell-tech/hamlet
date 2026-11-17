//! The documents a chief edits and reads: the charter, characters,
//! memory, limits, settings, papers and the prompt.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_git::Change as GitChange;
use hamlet_protocol::client::{
    self as protocol, Edit, EditLimits, EditSettingsSet, Keeper,
    Read, ReadResult, RequestId, ServerMessage,
};
use hamlet_store::ids::{Actor, AgentId, HamletId, ScholarId};
use hamlet_store::schema::document;

use super::check::reply;
use crate::conversations::{chats, prompt};
use crate::core::Core;
use crate::knowledge::papers::papers;
use crate::knowledge::{CHARACTER, memory_path};
use crate::realm::change::{Done, done, refuse};
use crate::society::agents::Effort;
use crate::society::authority;
use crate::society::city::{self, CHARACTER_MOST};
use crate::society::limits::{self, Limit, Limits, RealmLimits};
use crate::society::refusal::Refusal;
use crate::society::spend::micros;

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
            Edit::Character { scholar, text, .. } => {
                let scholar = ScholarId(i64::try_from(scholar.0)?);
                self.edit_character(scholar, text)
            }
            Edit::Memory { of, path, text, .. } => {
                let path = match memory_path(&path) {
                    Ok(path) => path,
                    Err(refusal) => refuse!(refusal),
                };
                match of {
                    Keeper::Scholar(scholar) => {
                        let scholar =
                            ScholarId(i64::try_from(scholar.0)?);
                        self.edit_scholar_memory(scholar, path, text)
                    }
                    Keeper::Agent(agent) => {
                        let agent = AgentId(i64::try_from(agent.0)?);
                        self.edit_lodge_memory(agent, &path, &text)
                    }
                }
            }
            Edit::Limits(EditLimits::Whole {
                hamlet, whole, ..
            }) => {
                let value = serde_json::Value::Object(
                    whole.into_iter().collect(),
                );
                self.set_whole_limits(
                    hamlet.map(HamletId).as_ref(),
                    value,
                )
            }
            Edit::Limits(EditLimits::Set { hamlet, set, .. }) => {
                let limit = match Limit::read(
                    &set.section,
                    &set.key,
                    set.value,
                ) {
                    Ok(limit) => limit,
                    Err(refusal) => refuse!(refusal),
                };
                let hamlet = hamlet.map(HamletId);
                let chief = self.realm.chief();
                let done =
                    self.attempt(Actor::Chief(chief), |cx, _| {
                        limits::set_one(
                            cx,
                            chief,
                            hamlet.as_ref(),
                            limit,
                        )
                    })?;
                Ok(done.map(|()| limits::saved(hamlet.as_ref())))
            }
            Edit::Settings { set, .. } => self.edit_settings(set),
            Edit::Catalogue { model, set, .. } => {
                let answer = self.edit_catalogue(&model, set)?;
                self.publish();
                Ok(answer)
            }
        }
    }

    /// Keeps the realm's charter, or none when `text` is blank.
    pub(crate) fn edit_charter(
        &mut self,
        text: Option<String>,
    ) -> Done<String> {
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

    /// Commits `text` as the character of `scholar`.
    fn edit_character(
        &mut self,
        scholar: ScholarId,
        text: String,
    ) -> Done<String> {
        if text.chars().count() > CHARACTER_MOST {
            refuse!(Refusal::LongCharacter);
        }
        done!(self.keeps(scholar));
        let by = Actor::Chief(self.realm.chief());
        self.commit_main(
            by,
            scholar,
            &[GitChange::Write {
                path: CHARACTER.into(),
                bytes: text.into_bytes(),
            }],
            "Edited the character",
        )?;
        Ok(Ok("The character is saved.".into()))
    }

    /// Commits a memory file of `scholar`'s `main`; empty removes it.
    fn edit_scholar_memory(
        &mut self,
        scholar: ScholarId,
        path: String,
        text: String,
    ) -> Done<String> {
        done!(self.keeps(scholar));
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
            scholar,
            &[change],
            "Edited the memory",
        )?;
        Ok(Ok("The memory is saved.".into()))
    }

    /// Writes a memory file of `agent`'s lodge, empty removing it,
    /// and has the lodge committed.
    fn edit_lodge_memory(
        &mut self,
        agent: AgentId,
        path: &str,
        text: &str,
    ) -> Done<String> {
        let by = Actor::Chief(self.realm.chief());
        let db = self.realm.store().connection();
        done!(authority::reaches(db, by, agent));
        self.write_lodge_memory(by, agent, path, text)?;
        Ok(Ok("The memory is saved.".into()))
    }

    /// Keeps the settings `set` changes, and the workspace it
    /// chooses.
    pub(crate) fn edit_settings(
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

    /// Replaces every limit of `hamlet`, or the realm's when none,
    /// with `whole`, their client form.
    fn set_whole_limits(
        &mut self,
        hamlet: Option<&HamletId>,
        whole: serde_json::Value,
    ) -> Done<String> {
        let chief = self.realm.chief();
        let by = Actor::Chief(chief);
        let done = match hamlet {
            Some(id) => {
                let Ok(whole) =
                    serde_json::from_value::<protocol::Limits>(whole)
                else {
                    refuse!(Refusal::LimitsUnfit);
                };
                let whole = limits_of(&whole);
                self.attempt(by, |cx, _| {
                    limits::set_hamlet(cx, id, |own| {
                        *own = whole;
                        Ok(())
                    })
                })?
            }
            None => {
                let Ok(whole) = serde_json::from_value::<
                    protocol::RealmLimits,
                >(whole) else {
                    refuse!(Refusal::LimitsUnfit);
                };
                let whole = RealmLimits {
                    roads_between: whole.roads_between,
                    budget: budget_of(&whole.budget),
                };
                self.attempt(by, |cx, _| {
                    limits::set_realm(cx, chief, |own| {
                        *own = whole;
                        Ok(())
                    })
                })?
            }
        };
        Ok(done.map(|()| limits::saved(hamlet)))
    }

    /// Refuses unless this server's chief keeps `scholar`.
    fn keeps(&mut self, scholar: ScholarId) -> Done<()> {
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
                let db = self.realm.store().connection();
                if let Err(refusal) = authority::reaches(
                    db,
                    Actor::Chief(chief),
                    agent,
                )? {
                    return self
                        .answer(client, reply(&id, Err(refusal)));
                }
                let conversation = self
                    .change(Actor::Chief(chief), |cx, env| {
                        chats::current(cx, env, agent)
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
            Read::Models { id, .. } => self.catalogue_read(&id)?,
            Read::Papers { of, id, .. } => {
                let layout = self.realm.layout().clone();
                let db = self.realm.store().connection();
                match papers(db, &layout, of)? {
                    Ok(papers) => {
                        ServerMessage::Read(ReadResult::Papers {
                            papers,
                            to: id,
                        })
                    }
                    Err(refusal) => reply(&id, Err(refusal)),
                }
            }
        };
        self.answer(client, message)
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
            hire: shown.reserved.hire,
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
