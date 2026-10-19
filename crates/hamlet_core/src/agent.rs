//! The agent loop: one session's history, derived from and written to
//! its log.

mod compact;
mod event;
mod gate;
mod hamlet_tools;
mod library;
mod message;
mod plug;
mod told;
mod voting;

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use hamlet_home::Home;
use hamlet_model::log::{self, Log, ToolRecord};
use hamlet_model::provider::{CallRecord, Delta, Provider};
use hamlet_rules::chief::{asking, rulings};
use hamlet_rules::hamlet::{Hamlet, MAYOR, Resident};
use hamlet_rules::realm::{Realm, Rules};
use hamlet_rules::{budget, vote};
use serde_json::{Value, json};

pub(crate) use self::event::SUMMARY_CHARS;
pub use self::event::{Event, Listener};
pub(crate) use self::gate::{Gate, Objection, Request, Ruling};
pub(crate) use self::plug::{Ask, Given, Plug, What};
pub(crate) use self::told::Told;
use self::voting::Polling;
use crate::tools::change::failure;
use crate::tools::{Call, Change, Leave, Outcome, Tools};

const REFLECT_PROMPT: &str =
    "\
Reflection time (automatic, not from a user). Look back over this \
     session. Update NOTES.md with anything worth remembering about \
     the users or lessons learned, and save or improve a skill for \
     anything you did that is likely to come up again. If nothing \
     is worth saving, say so in one line.";

const REFLECT_HAMLET: &str =
    "\nThen, in one line: did anything about how the hamlet is run \
     get in the way of your work? If nothing did, say so.";

/// One agent and its conversation, kept in a log.
pub struct Agent {
    pub(crate) provider: Arc<dyn Provider>,
    tools: Tools,
    /// The sandbox and network line of the system prompt.
    pub(crate) environment: String,
    /// The model that runs where nobody chose one, by name.
    pub(crate) model: String,
    /// A model and the tokens it was found to hold at once.
    seen: Option<(String, u64)>,
    /// The last call nearly filled its model's window.
    tight: bool,
    log: Log,
    messages: Vec<Value>,
    secrets: Vec<String>,
    /// The hamlet it lives in.
    pub hamlet: Option<Hamlet>,
    /// Its entry in the hamlet's record. `None` for the hamlet's
    /// first mayor until its first turn.
    pub resident: Option<Resident>,
    /// It holds the mayor's office, as the record last said.
    mayor: bool,
    /// The rules votes and dealings between hamlets follow.
    pub rules: Rules,
    /// The ballot this turn is for.
    ballot: Option<Polling>,
    /// This turn is the mayor's one answer to a vote: words only.
    answering: bool,
    /// The task of the current turn. Its caps are counted under this
    /// id along every agent it touches (H12).
    task: String,
    /// Another agent is waiting on this one's reply.
    awaited: bool,
    pub(crate) gate: Option<Gate>,
    /// The way to the tools installed plugins give it.
    pub(crate) plug: Option<Plug>,
    /// Some plugin may steer it, as the app last said.
    steered: bool,
    pub(crate) objection: Objection,
    /// A chief answers its gate, so it can be asked a question.
    pub(crate) overseen: bool,
    /// User turns since the last reflection.
    pub(crate) unreflected: usize,
}

impl Agent {
    /// Opens a session, rebuilding history from its log if one
    /// exists.
    pub fn open(
        provider: Arc<dyn Provider>,
        tools: Tools,
        environment: String,
        log_path: &Path,
        secrets: Vec<String>,
    ) -> Result<Self> {
        let messages =
            log::derive_messages(&log::read_standing(log_path)?);
        let log = Log::open(log_path, secrets.clone())?;
        Ok(Agent {
            provider,
            tools,
            environment,
            model: String::new(),
            seen: None,
            tight: false,
            log,
            messages,
            secrets,
            hamlet: None,
            resident: None,
            mayor: true,
            rules: Rules::default(),
            ballot: None,
            answering: false,
            task: String::new(),
            awaited: false,
            gate: None,
            plug: None,
            steered: false,
            objection: Objection::Closed,
            overseen: false,
            unreflected: 0,
        })
    }

    pub(crate) fn log_path(&self) -> &Path {
        self.log.path()
    }

    /// An agent in `house` that logs to `log`, with this one's
    /// provider, environment, secrets and rules.
    fn beside(&self, house: Home, log: &Path) -> Result<Agent> {
        let mut agent = Agent::open(
            self.provider.clone(),
            self.tools.in_house(house),
            self.environment.clone(),
            log,
            self.secrets.clone(),
        )?;
        agent.model = self.model.clone();
        agent.rules = self.rules;
        Ok(agent)
    }

    /// Closes a turn that was cut off, so its history stays valid.
    pub(crate) fn stopped(&mut self, reason: &str) -> Result<()> {
        self.log.turn_aborted(reason)?;
        self.messages = log::derive_messages(&log::read_standing(
            self.log.path(),
        )?);
        Ok(())
    }

    /// Takes the conversation back to just before the message that
    /// is line `to` of its log.
    pub(crate) fn rewind(&mut self, to: usize) -> Result<()> {
        self.log.rewind(to)?;
        let stands = log::read_standing(self.log.path())?;
        // The turns that stand since the last reflection.
        let since = stands
            .iter()
            .rev()
            .filter(|line| line["type"] == "user")
            .take_while(|line| line["user"] != "hamlet")
            .count();
        self.unreflected = self.unreflected.min(since);
        self.messages = log::derive_messages(&stands);
        self.tight = false;
        Ok(())
    }

    /// Answers `text` from `user` as a turn of `task`. `None`
    /// begins a new task.
    pub(crate) async fn turn(
        &mut self,
        task: Option<&str>,
        user: &str,
        text: &str,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        self.task = match task {
            Some(task) => task.to_string(),
            None => budget::task_after(&self.name(), &self.task),
        };
        let said = self.run(user, text, on).await;
        // A turn that failed left nothing to look back on.
        self.unreflected += usize::from(said.is_ok());
        said
    }

    /// Carries on a turn that failed or was stopped, from where it
    /// ended, as a new task. `Err` when the last message was
    /// answered.
    pub(crate) async fn again(
        &mut self,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let open = self
            .messages
            .last()
            .is_some_and(|last| last["role"] != "assistant");
        if !open {
            anyhow::bail!(
                "the last message was answered, so there is nothing \
                 to carry on"
            );
        }
        self.task = budget::task_after(&self.name(), &self.task);
        self.here()?;
        let said = self.work(on).await;
        self.unreflected += usize::from(said.is_ok());
        said
    }

    /// Ends the turn as one that failed, for the reason `why`.
    fn fail(&mut self, why: &str) -> anyhow::Error {
        match self.log.turn_aborted(why) {
            Ok(()) => anyhow::anyhow!("{why}"),
            Err(e) => e,
        }
    }

    /// Tells whoever reads the conversation `text`.
    fn note(
        &mut self,
        text: &str,
        on: &mut Listener<'_>,
    ) -> Result<()> {
        self.log.notice(text)?;
        on(Event::Notice(text.to_string()));
        Ok(())
    }

    /// The address it goes by in the hamlet: the office while it is
    /// mayor, its name otherwise.
    fn name(&self) -> String {
        match &self.resident {
            Some(resident) if !self.mayor => resident.name.clone(),
            _ => MAYOR.to_string(),
        }
    }

    fn realm(&self, hamlet: &Hamlet) -> Realm {
        Realm::with(hamlet.home().to_path_buf(), self.rules)
    }

    /// Ends the turn with a statement of what ran out.
    fn stop(
        &mut self,
        why: &str,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let text = format!("[stopped: {why}]");
        let message = json!({"role": "assistant", "content": text});
        self.log.assistant(&message)?;
        self.messages.push(message);
        on(Event::Text(text.clone()));
        Ok(text)
    }

    /// Has the agent reflect (C5).
    pub(crate) async fn reflect(
        &mut self,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        self.unreflected = 0;
        self.objection = Objection::Closed;
        self.task = budget::task_after(&self.name(), &self.task);
        // An agent of a hamlet is asked once more, plainly: it is
        // the one moment it looks back at how things went.
        let lives = self.hamlet.is_some() && !self.mayor;
        let asked = match lives {
            true => format!("{REFLECT_PROMPT}{REFLECT_HAMLET}"),
            false => REFLECT_PROMPT.to_string(),
        };
        self.run("hamlet", &asked, on).await
    }

    /// Reads who this agent is, and whether it holds the mayor's
    /// office, from the record again. `Err` says how it left the
    /// hamlet.
    fn here(&mut self) -> Result<()> {
        let Some(hamlet) = &self.hamlet else {
            return Ok(());
        };
        let (id, name) = match &self.resident {
            Some(me) => (me.id, me.name.as_str()),
            None => (0, MAYOR),
        };
        let (resident, mayor) = hamlet.seat(id, name)?;
        self.resident = Some(resident);
        self.mayor = mayor;
        Ok(())
    }

    /// Ends the turn of an agent that has left the hamlet.
    fn stop_if_gone(&mut self) -> Result<()> {
        let gone = self.here();
        if let Err(e) = &gone {
            self.log.turn_aborted(&e.to_string())?;
        }
        gone
    }

    /// The system prompt and the tools of one step. `faults` are
    /// the tools that failed to load and `budget` the words on the
    /// hamlet's budget.
    fn brief(
        &self,
        model: Option<&str>,
        faults: &str,
        budget: &str,
    ) -> (String, Vec<Value>) {
        let mut system = Told {
            house: self.tools.home(),
            environment: &self.environment,
            model: model.unwrap_or(&self.model),
            faults,
            project: self.tools.project(),
            library: self.tools.library(),
            place: self.hamlet.as_ref().zip(self.resident.as_ref()),
            mayor: self.mayor,
            rules: self.rules,
            budget,
        }
        .prompt();
        let mut defs = self.tools.definitions();
        if let Some(hamlet) =
            self.hamlet.as_ref().filter(|_| self.resident.is_some())
        {
            if self.mayor {
                defs.extend(Hamlet::definitions());
                if self.realm(hamlet).hamlets().len() > 1 {
                    defs.extend(Realm::definitions());
                }
            } else {
                defs.extend(Hamlet::resident_definitions());
            }
        }
        if self.objection == Objection::Open {
            system.push_str(
                "\nThe latest message is a chief's instruction. If \
                 you think it is a mistake, call `object` with your \
                 reason before doing the work: only that holds the \
                 work for the chief's reply.",
            );
        }
        if self.overseen {
            defs.push(rulings::ask_chief_definition());
        }
        if self.objection == Objection::Open {
            defs.push(rulings::object_definition());
        }
        // Whoever waits on this turn's reply would never hear a
        // later one.
        if self.awaited {
            defs.retain(|d| d["function"]["name"] != "tell_agent");
        }
        // A voter can do nothing but vote.
        if self.ballot.is_some() {
            defs = vec![vote::ballot_definition()];
        }
        (system, defs)
    }

    /// Asks the model for the next message of the conversation, and
    /// writes it and what it used to the log and to the books of
    /// `payer`.
    async fn complete(
        &mut self,
        request: &[Value],
        defs: &[Value],
        model: Option<&str>,
        payer: &str,
        on: &mut Listener<'_>,
    ) -> Result<(Value, CallRecord)> {
        let mut on_delta = |d: Delta<'_>| {
            on(match d {
                Delta::Text(t) => Event::Text(t.to_string()),
                Delta::Reasoning(r) => {
                    Event::Reasoning(r.to_string())
                }
            });
        };
        let asked = self
            .provider
            .complete(request, defs, model, &mut on_delta)
            .await;
        let done = match asked {
            Ok(done) => done,
            Err(e) => {
                self.log
                    .turn_aborted(&format!("provider error: {e}"))?;
                return Err(e);
            }
        };
        self.log.assistant(&done.message)?;
        self.log.model_call(&done.call)?;
        if let Some(hamlet) = &self.hamlet {
            let estimate =
                budget::estimate(request, defs, &done.message);
            hamlet.spend(payer, &self.task, &done.call, estimate)?;
        }
        on(Event::ModelCall(done.call.clone()));
        self.messages.push(done.message.clone());
        Ok((done.message, done.call))
    }

    async fn run(
        &mut self,
        user: &str,
        text: &str,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        self.here()?;
        self.log.user(user, text)?;
        self.messages.push(json!({"role": "user", "content": text}));
        self.work(on).await
    }

    /// Asks the model and runs its tools until it answers in words.
    async fn work(
        &mut self,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let faults = self.tools.faults();

        // A ballot and the mayor's answer to a vote are short. Any
        // other turn ends when the model answers, the budget runs
        // out or a chief stops it.
        let plain = self.ballot.is_none() && !self.answering;
        let steps = match plain {
            true => usize::MAX,
            false => self.rules.ballot_steps,
        };
        // The words on the budget tell of what is spent, so they
        // differ after every call, and a prompt that differs is one
        // the model's server reads again in full. A turn keeps the
        // words it began with, for whoever held the office then.
        let mut budget = None::<(bool, String)>;
        // The conversation was summarised for a reply that had no
        // room.
        let mut squeezed = false;
        let mut step = 0u32;
        for _ in 0..steps {
            step = step.saturating_add(1);
            // It may have been removed or merged since its last
            // step, or the office may have changed hands.
            self.stop_if_gone()?;
            let me = self.name();
            // A ballot is the hamlet's to pay, and so is the mayor's
            // answer.
            let payer = match plain {
                true => me.as_str(),
                false => vote::PAYER,
            };
            // Checked before every call, so a change to the limits or
            // another agent's spending counts from the next one.
            let stopped = self
                .hamlet
                .as_ref()
                .and_then(|h| h.stopped(&self.task));
            if let Some(why) = stopped {
                return self.stop(&why, on);
            }
            let model =
                self.hamlet.as_ref().and_then(|h| h.model_of(&me));
            let model = model.as_deref();
            // The chief may point the hamlet at another folder, or
            // change how it is worked on, at any time.
            if let Some(hamlet) = &self.hamlet {
                self.tools
                    .work_on(hamlet.work().project(), hamlet.home());
                self.tools.lead(match self.mayor {
                    true => hamlet.workspaces(),
                    false => vec![],
                });
            }
            if budget.as_ref().is_none_or(|b| b.0 != self.mayor) {
                let words = self
                    .hamlet
                    .as_ref()
                    .map(|h| h.budget_words(&me, self.mayor));
                budget =
                    Some((self.mayor, words.unwrap_or_default()));
            }
            let words = budget.as_ref().map_or("", |b| b.1.as_str());
            let notes = self.plugged(step, plain, on).await?;
            let (mut system, defs) =
                self.brief(model, &faults, words);
            system.push_str(&notes);
            let system = json!({"role": "system", "content": system});
            let window = self.window(model);
            if let Some(window) = window
                && self.crowded(&system, &defs, window)
            {
                self.compact(model, payer, window, on).await?;
            }
            let mut request = vec![system];
            request.extend(self.messages.iter().cloned());
            let (message, call) = self
                .complete(&request, &defs, model, payer, on)
                .await?;
            self.measure(&call, window);

            let calls = message["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let said =
                message["content"].as_str().unwrap_or_default();
            if calls.is_empty() && said.trim().is_empty() && plain {
                let sent =
                    budget::estimate(&request, &defs, &message);
                match self.out_of_room(&call, model, sent) {
                    Some(window) if !squeezed => {
                        squeezed = true;
                        // The reply that said nothing.
                        self.messages.pop();
                        self.compact(model, payer, window, on)
                            .await?;
                        continue;
                    }
                    Some(_) => {
                        return Err(self.fail(
                            "the model has no room left to answer, \
                             even with the conversation summarised. \
                             Start a new session, or choose a model \
                             that holds more",
                        ));
                    }
                    None => {
                        return Err(self.fail(
                            "the model answered with nothing. Send \
                             the message again",
                        ));
                    }
                }
            }
            if calls.is_empty() {
                return Ok(said.to_string());
            }
            for c in calls {
                let call = Call {
                    id: c["id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    name: c["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    args: c["function"]["arguments"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                };
                self.stop_if_gone()?;
                on(Event::Tool {
                    name: call.name.clone(),
                    summary: call
                        .args
                        .chars()
                        .take(SUMMARY_CHARS)
                        .collect(),
                    change: Change::of(&call.name, &call.args),
                });
                let started = Instant::now();
                let spent = call.name == rulings::OBJECT
                    && self.objection != Objection::Open;
                let who = self.name();
                // A plugin may only make a call stricter, so it is
                // asked before the gate, and a call it holds back
                // never reaches the chief.
                let held = match plain {
                    true => self.held(step, &call, on).await?,
                    false => None,
                };
                let ruling = match &mut self.gate {
                    _ if let Some(why) = &held => {
                        Ruling::Deny(why.clone())
                    }
                    _ if spent => Ruling::Deny(
                        "you may not object to this instruction \
                         now; carry it out"
                            .into(),
                    ),
                    // A ballot is between the voter and the record.
                    _ if self.ballot.is_some() => {
                        Ruling::Go(Leave::default())
                    }
                    Some(gate) => {
                        gate(Request {
                            who: &who,
                            reach: self.tools.reach(&call),
                            call: &call,
                        })
                        .await
                    }
                    None => Ruling::Go(Leave::default()),
                };
                if call.name == rulings::OBJECT {
                    self.objection = Objection::Spent;
                }
                let outcome = match ruling {
                    Ruling::Deny(reason) => Outcome::blocked(&reason),
                    Ruling::Say(text) => Outcome::plain(0, text),
                    Ruling::Go(leave) => {
                        if let Some(outcome) =
                            self.hamlet_call(&call, leave, on).await
                        {
                            outcome
                        } else if let Some(outcome) =
                            self.plug_call(&call).await
                        {
                            outcome
                        } else {
                            self.tools.call(&call, leave).await
                        }
                    }
                };
                let duration_ms =
                    started.elapsed().as_millis() as u64;
                on(Event::ToolDone {
                    name: call.name.clone(),
                    exit_code: outcome.exit_code,
                    duration_ms,
                    failure: (outcome.exit_code != 0)
                        .then(|| failure(&outcome.output)),
                });
                self.log.tool(&ToolRecord {
                    call_id: &call.id,
                    name: &call.name,
                    args: &call.args,
                    exit_code: outcome.exit_code,
                    duration_ms,
                    policy: &outcome.policy,
                    degraded: outcome.degraded,
                    output: &outcome.output,
                })?;
                if call.name == rulings::ASK_CHIEF
                    && let Some(said) = asking::said(&outcome.output)
                {
                    on(Event::Answered(said.to_string()));
                }
                self.messages.push(json!({"role": "tool", "tool_call_id": call.id, "content": outcome.output}));
            }
            if self.ballot.as_ref().is_some_and(|b| b.cast.is_some())
            {
                return Ok(String::new());
            }
        }
        let reason = format!("step limit ({steps}) reached");
        self.log.turn_aborted(&reason)?;
        Ok(format!("[stopped: {reason}]"))
    }
}

#[cfg(test)]
mod tests;
