//! An agent's words to another along a road (H30, H31), its ask for a
//! road, and the workers it starts, each kept once per call.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::ids::{Actor, AgentId};
use hamlet_store::schema::worker;

use super::{Call, Step, list, said, task_chief, text};
use crate::calls::place;
use crate::core::Core;
use crate::society::agents::{self, EndHow};
use crate::society::refusal::Refusal;
use crate::society::workers::{self, Start, Starter};
use crate::society::{addresses, hires, roads};
use crate::talk::briefs;
use crate::talk::conversations::Kind;
use crate::talk::letters::{self, Message, MessageKind, Send};
use crate::talk::tasks::{self, Origin};
use crate::work::catalogue::SOCIETY;

impl Core {
    pub(in crate::calls) async fn tell(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Step> {
        self.send(
            call,
            MessageKind::Letter,
            "to",
            &text(call, "text"),
        )
        .await
    }

    pub(in crate::calls) async fn ask(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Step> {
        self.send(
            call,
            MessageKind::Question,
            "to",
            &text(call, "question"),
        )
        .await
    }

    /// Asks the caller's lead for a road, as a letter to it.
    pub(in crate::calls) async fn request_road(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Step> {
        let sender = call.conversation.agent;
        let here = self.hamlet_of(sender)?;
        let to = match self.named(call, "to", here.as_ref())? {
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
        let words = format!(
            "I ask for a road to {name}: {}",
            text(call, "reason")
        );
        self.deliver(call, MessageKind::Letter, lead, "lead", &words)
            .await
    }

    /// Sends a letter or a question to the agent the argument `key`
    /// names.
    async fn send(
        &mut self,
        call: &Call<'_>,
        kind: MessageKind,
        key: &str,
        words: &str,
    ) -> Result<Step> {
        let sender = call.conversation.agent;
        let db = self.realm.store().connection();
        if let Some(found) =
            letters::find(db, &call.call.runtime, &call.call.key)?
        {
            return self.sent(&found);
        }
        let here = self.hamlet_of(sender)?;
        let receiver = match self.named(call, key, here.as_ref())? {
            Ok(receiver) => receiver,
            Err(refusal) => return Ok(Step::of(Err(refusal))),
        };
        let address = text(call, key);
        self.deliver(call, kind, receiver, &address, words).await
    }

    async fn deliver(
        &mut self,
        call: &Call<'_>,
        kind: MessageKind,
        receiver: AgentId,
        address: &str,
        words: &str,
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
        let here = self.hamlet_of(sender)?;
        let db = self.realm.store().connection();
        let most = super::limits_of(db, here.as_ref())?.task.messages;
        let task = self.task_of(call)?;
        let (runtime, key) =
            (call.call.runtime.clone(), call.call.key.clone());
        let from = call.conversation.id;
        let sent = self
            .attempt(Actor::Agent(sender), |cx, env| {
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
                if !tasks::count_message(cx.db, task, most)? {
                    return Ok(Err(Refusal::Usage {
                        words: format!(
                            "This task has sent its {most} \
                             messages; finish with what you have."
                        ),
                    }));
                }
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
                        text: words,
                    },
                )?))
            })
            .await?;
        match sent {
            Ok(found) => self.sent(&found),
            Err(refusal) => Ok(Step::of(Err(refusal))),
        }
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
    pub(in crate::calls) async fn start_worker(
        &mut self,
        call: &Call<'_>,
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
        let brief = text(call, "task");
        let tools = list(call, "tools");
        if tools.iter().any(|tool| SOCIETY.contains(&tool.as_str())) {
            return Ok(Step::of(Err(Refusal::WorkerTools)));
        }
        let names = self.catalogue_names();
        let db = self.realm.store().connection();
        let offered =
            place::tools(db, starter, Kind::Chat, &names, &SOCIETY)?;
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
        let waits = call
            .arguments
            .get("wait")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let task = self.task_of(call)?;
        let author = self.name(starter)?;
        let db = self.realm.store().connection();
        let chief = task_chief(db, task)?;
        let done = self
            .attempt(Actor::Agent(starter), |cx, env| {
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
            })
            .await?;
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

    /// The names of every tool in the catalogue.
    pub(crate) fn catalogue_names(&self) -> Vec<String> {
        self.catalogue
            .iter()
            .map(|spec| spec.name.clone())
            .collect()
    }
}
