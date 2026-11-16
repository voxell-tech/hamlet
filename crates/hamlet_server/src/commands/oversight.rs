//! A chief's commands on what waits and on what agents were allowed:
//! answering a wait, listing what waits and what was granted, taking
//! a grant back, instructing another agent and requesting a
//! reflection.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_protocol::client::{Choice, Permission};
use hamlet_store::ids::{Actor, AgentId, HamletId};
use hamlet_store::schema::permission_grant;

use super::Line;
use crate::change::Done;
use crate::conversations::{chats, reflections};
use crate::core::Core;
use crate::oversight::{self, gate};
use crate::society::addresses::{self, Reach};
use crate::society::refusal::Refusal;

impl Core<'_, '_> {
    /// Answers a wait with `choice`: the one `#<id>` names, else the
    /// oldest of the agent `line` was said to, else the oldest of
    /// all.
    pub(super) fn answer_line(
        &mut self,
        line: &Line,
        choice: &Choice,
    ) -> Done<String> {
        let (agent, rest) = (line.agent, line.rest.as_str());
        let (named, text) = match rest.strip_prefix('#') {
            Some(after) => {
                let (id, text) = after
                    .split_once(char::is_whitespace)
                    .map_or((after, ""), |(id, text)| {
                        (id, text.trim())
                    });
                (Some(id.to_owned()), text)
            }
            None => (None, rest),
        };
        let id = match named {
            Some(id) => id,
            None => {
                let db = self.realm.store().connection();
                let open = oversight::open(db)?;
                let mine = open
                    .iter()
                    .find(|wait| wait.agent == Some(agent));
                match mine.or(open.first()) {
                    Some(wait) => wait.id.to_string(),
                    None => {
                        return Ok(Ok(
                            "Nothing waits for you.".into()
                        ));
                    }
                }
            }
        };
        let text = (!text.is_empty()).then_some(text);
        self.answer_wait(&id, choice, text, None)
    }

    pub(super) fn queue(&mut self) -> Done<String> {
        let db = self.realm.store().connection();
        let mut lines = Vec::new();
        for wait in oversight::open(db)? {
            let by = match wait.agent {
                Some(agent) => addresses::name_of(db, agent)?,
                None => "the realm".into(),
            };
            let answers = wait
                .answers()?
                .iter()
                .map(|choice| {
                    format!("/{}", oversight::Ruling::of(choice))
                })
                .collect::<Vec<_>>()
                .join(" ");
            lines.push(format!(
                "- #{} {}\n  answer with: {answers}",
                wait.id,
                oversight::line(&wait, &by, 80)
            ));
        }
        Ok(Ok(if lines.is_empty() {
            "Nothing waits for you.".into()
        } else {
            lines.join("\n")
        }))
    }

    pub(super) fn grants(&mut self) -> Done<String> {
        let db = self.realm.store().connection();
        let held = permission_grant::table
            .filter(permission_grant::revoked_at.is_null())
            .order(permission_grant::id)
            .select((
                permission_grant::agent,
                permission_grant::permission,
            ))
            .load::<(AgentId, String)>(db)?;
        let mut lines = Vec::new();
        for (agent, permission) in held {
            lines.push(format!(
                "- {} may use {permission}",
                addresses::name_of(db, agent)?
            ));
        }
        Ok(Ok(if lines.is_empty() {
            "No agent holds a lasting grant.".into()
        } else {
            lines.join("\n")
        }))
    }

    pub(super) fn revoke(
        &mut self,
        here: Option<&HamletId>,
        rest: &str,
    ) -> Done<String> {
        let mut words = rest.split_whitespace();
        let (Some(address), Some(permission)) =
            (words.next(), words.next())
        else {
            return Ok(Err(self.deeds.usage("revoke")));
        };
        let Some(permission) = gate::permission(permission) else {
            return Ok(Err(self.deeds.usage("revoke")));
        };
        let address = match addresses::parse(address) {
            Ok(address) => address,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        let target = match addresses::resolve(
            db,
            &address,
            here,
            Reach::Kept,
        )? {
            Ok(target) => target,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if let Err(refusal) = self.reaches(target)? {
            return Ok(Err(refusal));
        }
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        let by =
            addresses::actor_name(db, Some(Actor::Chief(chief)))?;
        let name = addresses::name_of(db, target)?;
        let word = gate::word(&permission);
        Ok(Ok(
            if gate::revoke(
                db,
                target,
                &permission,
                &by,
                crate::clock::now(),
            )? {
                format!("{name} may no longer use {word}.")
            } else if permission == Permission::Network
                && self.env.settings.allow_net
            {
                format!(
                    "{name} holds no grant of {word}; the settings \
                     allow every agent the network."
                )
            } else {
                format!("{name} holds no grant of {word}.")
            },
        ))
    }

    /// Instructs the agent `rest` begins with, from any chat.
    pub(super) fn instruct(
        &mut self,
        here: Option<&HamletId>,
        rest: &str,
    ) -> Done<String> {
        let Some((address, text)) =
            rest.split_once(char::is_whitespace)
        else {
            return Ok(Err(self.deeds.usage("to")));
        };
        let text = text.trim();
        if text.is_empty() {
            return Ok(Err(self.deeds.usage("to")));
        }
        let address = match addresses::parse(address) {
            Ok(address) => address,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        let target = match addresses::resolve(
            db,
            &address,
            here,
            Reach::Living,
        )? {
            Ok(target) => target,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if let Err(refusal) = self.reaches(target)? {
            return Ok(Err(refusal));
        }
        let chief = self.realm.chief();
        let line = self.credentials.scrub_text(text);
        let request = format!("to:{}", uuid::Uuid::new_v4());
        self.change(Actor::Chief(chief), |cx, env| {
            chats::say(cx, env, target, chief, &request, &line)
        })?;
        let name = self.name_of(target)?;
        Ok(Ok(format!("Told {name}.")))
    }

    pub(super) fn reflect(&mut self, agent: AgentId) -> Done<String> {
        let chief = self.realm.chief();
        let reflection =
            self.change(Actor::Chief(chief), |cx, env| {
                let conversation =
                    chats::current(cx, env, agent, chief)?;
                reflections::reflect(cx, conversation)
            })?;
        let name = self.name_of(agent)?;
        Ok(Ok(match reflection {
            Some(_) => format!("Told {name} to reflect."),
            None => format!("{name} is reflecting already."),
        }))
    }

    /// Starts a worker on `brief` in `here`, or outside every hamlet,
    /// whose answer shows in its own transcript.
    pub(super) fn chiefs_worker(
        &mut self,
        here: Option<&HamletId>,
        brief: &str,
    ) -> Done<String> {
        if brief.is_empty() {
            return Ok(Err(self.deeds.usage("worker")));
        }
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        let author =
            addresses::actor_name(db, Some(Actor::Chief(chief)))?;
        let hamlet = here.cloned();
        let brief = self.credentials.scrub_text(brief);
        let done = self.attempt(Actor::Chief(chief), |cx, env| {
            let id = match crate::society::workers::start(
                cx,
                crate::society::workers::Start {
                    starter: crate::society::workers::Starter::Chief(
                        chief,
                    ),
                    hamlet,
                    brief: brief.clone(),
                    tools: Vec::new(),
                    model: None,
                    waits: false,
                    call: None,
                },
            )? {
                Ok(id) => id,
                Err(refusal) => return Ok(Err(refusal)),
            };
            // A worker no model can run would never start.
            if !crate::conversations::models::has_model(
                cx.db,
                &env.settings,
                &env.overrides,
                id,
            )? {
                return Ok(Err(Refusal::NoModelSetUp));
            }
            crate::conversations::briefs::give(
                cx,
                env,
                id,
                &brief,
                &author,
                Some(chief),
            )?;
            Ok(Ok(id))
        })?;
        Ok(done.map(|id| {
            format!(
                "Started worker#{id}; open it to watch it and read \
                 its answer."
            )
        }))
    }

    fn name_of(&mut self, agent: AgentId) -> Result<String> {
        let db = self.realm.store().connection();
        addresses::name_of(db, agent)
    }
}
