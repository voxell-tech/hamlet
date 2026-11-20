//! An agent's messages and workers (H30, H31).

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::ids::{Actor, AgentId};
use hamlet_store::schema::worker;

use super::args::{Notify, Request, StartWorker};
use super::{Call, Step, rejected, said, task_chief};
use crate::attachments::inbox::INBOX;
use crate::calls::looks::Look;
use crate::calls::place;
use crate::conversations::messages::{
    self, Message, MessageKind, Send,
};
use crate::conversations::tasks::{self, Origin};
use crate::conversations::{Kind, briefs};
use crate::core::Core;
use crate::realm::change::done;
use crate::society::agents::{self, EndHow};
use crate::society::rejection::Rejection;
use crate::society::workers::{self, Start, Starter};
use crate::society::{addresses, channels};
use crate::tools::builtin::{Network, Outcome};
use crate::tools::catalogue::Society;

impl Core<'_, '_> {
    pub(in crate::calls) fn send_notification(
        &mut self,
        call: &Call<'_>,
        args: Notify,
    ) -> Result<Step> {
        self.send(
            call,
            MessageKind::Notification,
            &args.to,
            &args.text,
            &args.paths,
        )
    }

    pub(in crate::calls) fn send_request(
        &mut self,
        call: &Call<'_>,
        args: Request,
    ) -> Result<Step> {
        self.send(
            call,
            MessageKind::Request,
            &args.to,
            &args.text,
            &args.paths,
        )
    }

    /// Sends a notification or a request, with the files under
    /// `paths`, to the agent at `address`.
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
            messages::find(db, &call.call.runtime, &call.call.key)?
        {
            return self.sent(&found);
        }
        let here = self.hamlet_of(sender)?;
        let receiver = match self.named(address, here.as_ref())? {
            Ok(receiver) => receiver,
            Err(rejection) => return Ok(Step::of(Err(rejection))),
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
            return Ok(Step::of(Err(Rejection::SendToSelf)));
        }
        if words.is_empty() {
            return Ok(Step::of(Err(Rejection::NothingToSend)));
        }
        let other = self.name(receiver)?;
        let db = self.realm.store().connection();
        if channels::way(db, sender, receiver)?.is_none() {
            return Ok(Step::of(Err(Rejection::NoChannelTo {
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
                Err(rejection) => {
                    return Ok(Step::of(Err(rejection)));
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
                Ok(Ok(messages::send(
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
            Err(rejection) => Ok(Step::of(Err(rejection))),
        }
    }

    /// The look that copies the files under `paths` into `receiver`'s
    /// inbox.
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

    /// The answer of the call that sent `found`.
    fn sent(&mut self, found: &Message) -> Result<Step> {
        let name = self.name(found.receiver)?;
        let words = match found.kind {
            MessageKind::Notification => format!("Sent to {name}."),
            MessageKind::Request => format!(
                "Sent the request to {name}. Its response comes to \
                 you in a later turn; end this turn if you have \
                 nothing else to do meanwhile."
            ),
        };
        Ok(Step::Done(reached(said(words), found.receiver)))
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
            return Ok(Step::of(Err(Rejection::WorkerTools)));
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
            return Ok(Step::of(Err(Rejection::NoTool {
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
                let id = done!(workers::start(
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
                ));
                briefs::give(cx, env, id, &brief, &author, chief)?;
                Ok(Ok(id))
            })?;
        match done {
            Ok(id) => self.worked(id),
            Err(rejection) => Ok(Step::of(Err(rejection))),
        }
    }

    /// The answer a worker started by a call gives it.
    fn worked(&mut self, id: AgentId) -> Result<Step> {
        let db = self.realm.store().connection();
        let found = workers::get(db, id)?;
        let ended = agents::get(db, id)?;
        let name = addresses::address_of(db, id)?;
        if !found.waits {
            return Ok(Step::Done(reached(
                said(format!(
                    "Started {name}; its answer comes to you later \
                     as a response."
                )),
                id,
            )));
        }
        Ok(match (ended.ended_how, found.answer) {
            (Some(EndHow::Answered), Some(answer)) => {
                Step::Done(reached(
                    said(format!("{name} answers: {answer}")),
                    id,
                ))
            }
            (Some(how), _) => Step::Done(reached(
                rejected(format!("{name} ended: {}.", how.words())),
                id,
            )),
            (None, _) => {
                Step::Wait(format!("Waiting for {name}'s answer."))
            }
        })
    }
}

/// `outcome` with the agent its call reached, for the transcript.
fn reached(mut outcome: Outcome, agent: AgentId) -> Outcome {
    outcome
        .details
        .get_or_insert_default()
        .insert("agent".into(), agent.0.into());
    outcome
}
