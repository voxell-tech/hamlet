//! An agent's words to another along a channel (H30, H31), its
//! request for a channel, and the workers it starts, each kept once
//! per call.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::ids::{Actor, AgentId};
use hamlet_store::schema::worker;

use super::args::{Ask, RequestChannel, StartWorker, Tell};
use super::{Call, Step, refused, said, task_chief};
use crate::attachments::inbox::INBOX;
use crate::calls::looks::Look;
use crate::calls::place;
use crate::conversations::letters::{
    self, Message, MessageKind, Send,
};
use crate::conversations::tasks::{self, Origin};
use crate::conversations::{Kind, briefs};
use crate::core::Core;
use crate::society::agents::{self, EndHow};
use crate::society::refusal::Refusal;
use crate::society::workers::{self, Start, Starter};
use crate::society::{addresses, channels, hamlet_agents};
use crate::tools::builtin::Network;
use crate::tools::catalogue::Society;

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

    /// Requests a channel from the caller's parent, as a letter to
    /// it.
    pub(in crate::calls) fn request_channel(
        &mut self,
        call: &Call<'_>,
        args: RequestChannel,
    ) -> Result<Step> {
        let sender = call.conversation.agent;
        let here = self.hamlet_of(sender)?;
        let to = match self.named(&args.to, here.as_ref())? {
            Ok(to) => to,
            Err(refusal) => return Ok(Step::of(Err(refusal))),
        };
        let db = self.realm.store().connection();
        let parent = match hamlet_agents::find(db, sender)? {
            Some(found) => match found.parent {
                Some(parent) => parent,
                None => crate::society::hand::id(db)?,
            },
            None => return Ok(Step::of(Err(Refusal::ChannelEnds))),
        };
        let name = self.name(to)?;
        let words =
            format!("I request a channel to {name}: {}", args.reason);
        self.deliver(
            call,
            MessageKind::Letter,
            parent,
            "parent",
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
            return Ok(Step::of(Err(Refusal::TellSelf)));
        }
        if words.is_empty() {
            return Ok(Step::of(Err(Refusal::NothingToSend)));
        }
        let other = self.name(receiver)?;
        let db = self.realm.store().connection();
        if channels::way(db, sender, receiver)?.is_none() {
            return Ok(Step::of(Err(Refusal::NoChannelTo { other })));
        }
        if kind == MessageKind::Question
            && letters::would_loop(db, sender, receiver)?
        {
            return Ok(Step::of(Err(Refusal::AlreadyWaitingOn {
                other,
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
        let (session, worktree) =
            self.tools_of(call, at.place, Network::Closed)?;
        let inbox = self
            .open_inbox(receiver, &format!("{INBOX}/from-{from}"))?;
        Ok(Look::Copy {
            session,
            worktree,
            paths: paths.to_vec(),
            inbox: Box::new(inbox),
            images: layout.images(),
            receiver,
        })
    }

    /// The answer a message kept for a call gives it: a letter is
    /// sent; a question waits for its answer.
    fn sent(&mut self, found: &Message) -> Result<Step> {
        let name = self.name(found.receiver)?;
        Ok(match (found.kind, &found.answer, &found.failure) {
            (MessageKind::Letter, _, _) => {
                Step::Done(said(format!("Sent to {name}.")))
            }
            (MessageKind::Question, Some(answer), _) => {
                Step::Done(said(format!("{name} answers: {answer}")))
            }
            (MessageKind::Question, None, Some(failure)) => {
                Step::Done(refused(format!(
                    "{name} could not answer: {failure}"
                )))
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
        let names = self.tools.catalogue.names();
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

    /// The answer a worker started by a call gives it.
    fn worked(&mut self, id: AgentId) -> Result<Step> {
        let db = self.realm.store().connection();
        let found = workers::get(db, id)?;
        let ended = agents::get(db, id)?;
        let name = addresses::address_of(db, id)?;
        if !found.waits {
            return Ok(Step::Done(said(format!(
                "Started {name}; its answer comes to you as a reply."
            ))));
        }
        Ok(match (ended.ended_how, found.answer) {
            (Some(EndHow::Answered), Some(answer)) => {
                Step::Done(said(format!("{name} answers: {answer}")))
            }
            (Some(how), _) => Step::Done(refused(format!(
                "{name} ended: {}.",
                how.words()
            ))),
            (None, _) => {
                Step::Wait(format!("Waiting for {name}'s answer."))
            }
        })
    }
}
