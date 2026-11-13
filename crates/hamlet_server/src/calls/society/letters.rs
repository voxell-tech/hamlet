//! An agent's words to another along a road (H30, H31), its ask for a
//! road, and the workers it starts, each kept once per call.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::ids::{Actor, AgentId};
use hamlet_store::schema::worker;

use super::args::{Ask, RequestRoad, StartWorker, Tell};
use super::{Call, Step, said, task_chief};
use crate::calls::looks::Look;
use crate::calls::place;
use crate::core::Core;
use crate::inbox::INBOX;
use crate::society::agents::{self, EndHow};
use crate::society::refusal::Refusal;
use crate::society::workers::{self, Start, Starter};
use crate::society::{addresses, hires, roads};
use crate::talk::briefs;
use crate::talk::conversations::Kind;
use crate::talk::letters::{self, Message, MessageKind, Send};
use crate::talk::tasks::{self, Origin};
use crate::work::catalogue::Society;

impl Core<'_, '_> {
    pub(in crate::calls) fn tell(
        &mut self,
        call: &Call<'_>,
        args: Tell,
    ) -> Result<Step> {
        self.send(
            call,
            MessageKind::Letter,
            &args.to,
            &args.text,
            &args.paths,
        )
    }

    pub(in crate::calls) fn ask(
        &mut self,
        call: &Call<'_>,
        args: Ask,
    ) -> Result<Step> {
        self.send(
            call,
            MessageKind::Question,
            &args.to,
            &args.question,
            &args.paths,
        )
    }

    /// Asks the caller's lead for a road, as a letter to it.
    pub(in crate::calls) fn request_road(
        &mut self,
        call: &Call<'_>,
        args: RequestRoad,
    ) -> Result<Step> {
        let sender = call.conversation.agent;
        let here = self.hamlet_of(sender)?;
        let to = match self.named(&args.to, here.as_ref())? {
            Ok(to) => to,
            Err(refusal) => return Ok(Step::of(Err(refusal))),
        };
        let db = self.realm.store().connection();
        let lead = match hires::find(db, sender)? {
            Some(hire) => match hire.parent {
                Some(parent) => parent,
                None => crate::society::hand::id(db)?,
            },
            None => return Ok(Step::of(Err(Refusal::RoadEnds))),
        };
        let name = self.name(to)?;
        let words =
            format!("I ask for a road to {name}: {}", args.reason);
        self.deliver(
            call,
            MessageKind::Letter,
            lead,
            "lead",
            &words,
            &[],
        )
    }

    /// Sends a letter or a question, with the files under `paths`,
    /// to the agent at `address`.
    fn send(
        &mut self,
        call: &Call<'_>,
        kind: MessageKind,
        address: &str,
        words: &str,
        paths: &[String],
    ) -> Result<Step> {
        let sender = call.conversation.agent;
        let db = self.realm.store().connection();
        if let Some(found) =
            letters::find(db, &call.call.runtime, &call.call.key)?
        {
            return self.sent(&found);
        }
        let here = self.hamlet_of(sender)?;
        let receiver = match self.named(address, here.as_ref())? {
            Ok(receiver) => receiver,
            Err(refusal) => return Ok(Step::of(Err(refusal))),
        };
        self.deliver(call, kind, receiver, address, words, paths)
    }

    fn deliver(
        &mut self,
        call: &Call<'_>,
        kind: MessageKind,
        receiver: AgentId,
        address: &str,
        words: &str,
        paths: &[String],
    ) -> Result<Step> {
        let sender = call.conversation.agent;
        if receiver == sender {
            return Ok(Step::of(Err(Refusal::Usage {
                words: "That is you.".into(),
            })));
        }
        if words.is_empty() {
            return Ok(Step::of(Err(Refusal::Usage {
                words: "Say what to send.".into(),
            })));
        }
        let other = self.name(receiver)?;
        let db = self.realm.store().connection();
        if roads::way(db, sender, receiver)?.is_none() {
            return Ok(Step::of(Err(Refusal::Usage {
                words: format!(
                    "You have no road to {other}. Ask your lead for \
                     one with request_road."
                ),
            })));
        }
        if kind == MessageKind::Question
            && letters::would_loop(db, sender, receiver)?
        {
            return Ok(Step::of(Err(Refusal::Usage {
                words: format!(
                    "{other} is waiting on your answer already; \
                     tell it what it needs instead of asking."
                ),
            })));
        }
        let mut words = words.to_owned();
        let mut images = Vec::new();
        if !paths.is_empty() {
            // The files are copied off the loop first; this runs
            // again once they are.
            let Some(copied) = self.looks.copied(&call.call).cloned()
            else {
                let look = self.parcel(call, receiver, paths)?;
                return Ok(Step::Look(Box::new(look)));
            };
            match copied {
                Ok((captions, kept)) => {
                    words.push_str("\n\n");
                    words.push_str(&captions);
                    images = kept;
                }
                Err(refusal) => {
                    return Ok(Step::of(Err(refusal)));
                }
            }
        }
        let task = self.task_of(call)?;
        let (runtime, key) =
            (call.call.runtime.clone(), call.call.key.clone());
        let from = call.conversation.id;
        let sent =
            self.attempt(Actor::Agent(sender), |cx, env| {
                let task = match task {
                    Some(task) => task,
                    None => tasks::mint(
                        cx,
                        Origin::Notice,
                        sender,
                        None,
                        None,
                    )?,
                };
                Ok(Ok(letters::send(
                    cx,
                    env,
                    Send {
                        kind,
                        runtime: &runtime,
                        key: &key,
                        sender,
                        receiver,
                        from,
                        address,
                        task,
                        text: &words,
                        images: &images,
                    },
                )?))
            })?;
        match sent {
            Ok(found) => self.sent(&found),
            Err(refusal) => Ok(Step::of(Err(refusal))),
        }
    }

    /// The look that copies the files under `paths`, as the caller
    /// may read them, into `receiver`'s inbox, with the captions that
    /// say where each landed and the images among them.
    fn parcel(
        &mut self,
        call: &Call<'_>,
        receiver: AgentId,
        paths: &[String],
    ) -> Result<Look> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let at =
            place::of_conversation(db, &layout, &call.conversation)?;
        let from =
            addresses::address_of(db, call.conversation.agent)?
                .replace('#', "-");
        let (session, lodging) =
            self.tools_of(call, at.place, false)?;
        let inbox = self
            .open_inbox(receiver, &format!("{INBOX}/from-{from}"))?;
        Ok(Look::Copy {
            session,
            lodging,
            paths: paths.to_vec(),
            inbox: Box::new(inbox),
            images: layout.images(),
            receiver,
        })
    }

    /// What a message kept for a call answers it: a letter is sent; a
    /// question waits for its answer.
    fn sent(&mut self, found: &Message) -> Result<Step> {
        let name = self.name(found.receiver)?;
        Ok(match (found.kind, &found.answer, &found.failure) {
            (MessageKind::Letter, _, _) => {
                Step::Done(said(format!("Sent to {name}."), false))
            }
            (MessageKind::Question, Some(answer), _) => Step::Done(
                said(format!("{name} answers: {answer}"), false),
            ),
            (MessageKind::Question, None, Some(failure)) => {
                Step::Done(said(
                    format!("{name} could not answer: {failure}"),
                    true,
                ))
            }
            (MessageKind::Question, None, None) => {
                Step::Wait(format!("Waiting for {name}'s answer."))
            }
        })
    }

    /// Starts a worker on the call's task, once per call.
    pub(in crate::calls) fn start_worker(
        &mut self,
        call: &Call<'_>,
        args: StartWorker,
    ) -> Result<Step> {
        let starter = call.conversation.agent;
        let (runtime, key) =
            (call.call.runtime.clone(), call.call.key.clone());
        let db = self.realm.store().connection();
        let started = worker::table
            .filter(worker::call_runtime.eq(&runtime))
            .filter(worker::call_key.eq(&key))
            .select(worker::agent)
            .first::<AgentId>(db)
            .optional()?;
        if let Some(id) = started {
            return self.worked(id);
        }
        let StartWorker {
            task: brief,
            tools,
            wait: waits,
        } = args;
        if tools.iter().any(|tool| Society::named(tool).is_some()) {
            return Ok(Step::of(Err(Refusal::WorkerTools)));
        }
        let names = self.catalogue.names();
        let db = self.realm.store().connection();
        let offered = place::tools(db, starter, Kind::Chat, &names)?;
        let hamlet_protocol::runtime::ConfigureTools::Only { only } =
            offered
        else {
            unreachable!("a role's tools are a list");
        };
        if let Some(missing) =
            tools.iter().find(|tool| !only.contains(tool))
        {
            let name = self.name(starter)?;
            return Ok(Step::of(Err(Refusal::NoTool {
                name,
                tool: missing.clone(),
            })));
        }
        let task = self.task_of(call)?;
        let author = self.name(starter)?;
        let db = self.realm.store().connection();
        let chief = task_chief(db, task)?;
        let done =
            self.attempt(Actor::Agent(starter), |cx, env| {
                let id = match workers::start(
                    cx,
                    Start {
                        starter: Starter::Agent(starter),
                        hamlet: None,
                        brief: brief.clone(),
                        tools,
                        model: None,
                        waits,
                        call: Some((runtime, key)),
                    },
                )? {
                    Ok(id) => id,
                    Err(refusal) => return Ok(Err(refusal)),
                };
                briefs::give(cx, env, id, &brief, &author, chief)?;
                Ok(Ok(id))
            })?;
        match done {
            Ok(id) => self.worked(id),
            Err(refusal) => Ok(Step::of(Err(refusal))),
        }
    }

    /// What a worker started by a call answers it.
    fn worked(&mut self, id: AgentId) -> Result<Step> {
        let db = self.realm.store().connection();
        let found = workers::get(db, id)?;
        let ended = agents::get(db, id)?;
        let name = addresses::address_of(db, id)?;
        if !found.waits {
            return Ok(Step::Done(said(
                format!(
                    "Started {name}; its answer comes to you as a \
                     reply."
                ),
                false,
            )));
        }
        Ok(match (ended.ended_how, found.answer) {
            (Some(EndHow::Answered), Some(answer)) => Step::Done(
                said(format!("{name} answers: {answer}"), false),
            ),
            (Some(how), _) => Step::Done(said(
                format!("{name} ended: {}.", how.words()),
                true,
            )),
            (None, _) => {
                Step::Wait(format!("Waiting for {name}'s answer."))
            }
        })
    }
}
