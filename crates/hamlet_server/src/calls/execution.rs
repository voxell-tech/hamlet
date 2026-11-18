//! A tool call carried out once under its key (S19): its kept answer
//! when it was made before, else answered by a chief or the gate, run
//! as a society tool, or run as a built-in or added tool on a task.

use anyhow::Result;
use bevy_ecs::entity::Entity;
use bevy_log::warn;
use hamlet_protocol::runtime::{
    self, RuntimeMessage, ToolSpecReplay,
};
use hamlet_store::ids::AgentId;
use hamlet_store::marker::{self, Marked, Request};
use serde_json::{Map, Value};

use super::objections::OBJECTING;
use super::specs::{is_offered, unavailable};
use super::tool_runs::tool_run;
use super::{
    Answer, CONFLICT, Call, CallOutcome, Decision, Reply, held,
    place, rejected, society,
};
use crate::attachments::images::Limits;
use crate::conversations::{Kind, models};
use crate::core::Core;
use crate::knowledge::CHARACTER;
use crate::realm::workspace::serving::RemoteWorktree;
use crate::tools::builtin::shell::Dialect;
use crate::tools::builtin::{Network, Outcome, Sight, ToolSession};
use crate::tools::catalogue::Society;

impl Core<'_, '_> {
    /// Carries out the call `request`: its kept result when it ran,
    /// else as the gate decides now.
    pub(crate) fn operation(
        &mut self,
        from: Entity,
        request: runtime::Operation,
    ) -> Result<()> {
        let again = RuntimeMessage::Operation(request.clone());
        let reply = self.reply_of(from, &request.id);
        let call = match self.admit(
            from,
            &request.conversation,
            &request.key,
            request.input.as_deref(),
            &request.tool,
            &request.arguments,
        )? {
            Ok(call) => call,
            Err(answer) => {
                self.tell_call(&reply, &request.key, answer);
                return Ok(());
            }
        };
        // A call that runs already answers this request too.
        if let Some((_, mut running)) = self
            .runs
            .iter_mut()
            .find(|(_, running)| running.call == call.call)
        {
            running.replies.push(reply);
            return Ok(());
        }
        if let Some(answer) = self.kept_answer(&call)? {
            self.tell_call(&reply, &request.key, answer);
            return Ok(());
        }
        match self.carry_out(&call, &reply, from, again)? {
            CallOutcome::Done(outcome) => {
                let db = self.realm.store().connection();
                marker::settle(
                    db,
                    call.call.identity(),
                    &call.request,
                    &outcome,
                    crate::app::clock::now(),
                )?;
                self.looks.forget(&call.call);
                self.tell_call(
                    &reply,
                    &request.key,
                    Answer::Result(outcome),
                );
            }
            CallOutcome::Answer(answer) => {
                self.tell_call(&reply, &request.key, answer);
            }
            CallOutcome::Later => {}
        }
        Ok(())
    }

    /// The answer kept for `call` from when it was made before: its
    /// outcome, or that a restart cut it off; none when it is new.
    fn kept_answer(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Option<Answer>> {
        let db = self.realm.store().connection();
        let tool = call.tool;
        Ok(
            match marker::read::<Outcome>(
                db,
                call.call.identity(),
                &call.request,
            )? {
                Marked::Begun => {
                    Some(Answer::Result(Outcome::thrown(
                        "tool_interrupted",
                        format!(
                            "Tool {tool} was interrupted by a \
                             restart and may have partially run"
                        ),
                    )))
                }
                Marked::Outcome(outcome) => {
                    Some(Answer::Result(outcome))
                }
                Marked::Conflict => {
                    Some(Answer::Rejection(CONFLICT.into()))
                }
                Marked::Unmarked => None,
            },
        )
    }

    /// Carries out the new call `call` as far as it goes now:
    /// answered by a chief or the gate, run, or looked at off the
    /// loop first.
    fn carry_out(
        &mut self,
        call: &Call<'_>,
        reply: &Reply,
        from: Entity,
        again: RuntimeMessage,
    ) -> Result<CallOutcome> {
        let tool = call.tool;
        if !is_offered(&call.conversation, tool)? {
            return Ok(CallOutcome::Done(unavailable(tool)));
        }
        if let Some(chief @ (Society::AskChief | Society::Object)) =
            call.society
        {
            let ruled = if chief == Society::Object {
                self.object(call)?
            } else {
                self.ask_chief(call)?
            };
            return Ok(match ruled {
                Ok(outcome) => CallOutcome::Done(outcome),
                Err(answer) => CallOutcome::Answer(answer),
            });
        }
        if self.is_withdrawn(call)? {
            return Ok(CallOutcome::Done(rejected(held::WITHDRAWN)));
        }
        if self.is_objecting(call)? {
            return Ok(CallOutcome::Answer(Answer::Waiting(
                OBJECTING.into(),
            )));
        }
        Ok(match self.decide(call)? {
            Decision::Look(look) => {
                self.look(call, look, from, again);
                CallOutcome::Later
            }
            Decision::Block(words) => {
                CallOutcome::Answer(Answer::Rejection(words))
            }
            Decision::Wait(words) => {
                CallOutcome::Answer(Answer::Waiting(words.into()))
            }
            Decision::Run { net } => match call.society {
                Some(society) => match self.society(call, society)? {
                    society::Step::Done(outcome) => {
                        CallOutcome::Done(outcome)
                    }
                    society::Step::Wait(words) => {
                        CallOutcome::Answer(Answer::Waiting(words))
                    }
                    society::Step::Look(look) => {
                        self.look(call, look, from, again);
                        CallOutcome::Later
                    }
                },
                None => {
                    let request = call.request.clone();
                    match self.run_tool(
                        call,
                        reply.clone(),
                        request,
                        Network::of(net),
                    )? {
                        Some(outcome) => CallOutcome::Done(outcome),
                        None => CallOutcome::Later,
                    }
                }
            },
        })
    }

    /// Runs the built-in or added tool of `call` with the network
    /// `net`, after marking it begun when it may not run again. Gives
    /// the outcome of a call rejected before it runs.
    fn run_tool(
        &mut self,
        call: &Call<'_>,
        reply: Reply,
        request: Request,
        net: Network,
    ) -> Result<Option<Outcome>> {
        let once = self.is_once_only(call.tool);
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let agent = call.conversation.agent;
        let at =
            place::of_conversation(db, &layout, &call.conversation)?;
        if let Some(rejection) = character_rejection(&at.place, call)
        {
            return Ok(Some(rejected(rejection)));
        }
        if once {
            marker::begin(
                db,
                call.call.identity(),
                &request,
                crate::app::clock::now(),
            )?;
        }
        // A shell call shows in its agent's commands terminal.
        let shown = match Dialect::of_tool(call.tool) {
            Some(_) => {
                let command = call
                    .arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Some(self.commands_terminal(agent, &at, command)?)
            }
            None => None,
        };
        let db = self.realm.store().connection();
        let change =
            held::kept(db, &call.call.runtime, &call.call.key)?
                .and_then(|kept| kept.change)
                .and_then(|change| {
                    serde_json::from_str::<Value>(&change).ok()
                });
        let (session, worktree) =
            self.tools_of(call, at.place, net)?;
        let session = session.seeing(self.sight_of(agent)?);
        let secrets = self.credentials.secrets();
        let arguments =
            call.arguments.clone().into_iter().collect::<Map<_, _>>();
        let name = call.tool.to_owned();
        let running = tool_run(
            &self.inputs,
            call.call.clone(),
            call.conversation.id,
            reply,
            request,
            async move {
                // A remote worktree is in step before the call runs
                // there, and comes home after one that may write it,
                // before the agent's next call reads it.
                if let Some(worktree) = &worktree
                    && let Err(error) = worktree.in_step().await
                {
                    return rejected(
                        hamlet_sandbox::scrub::scrub_text(
                            &secrets,
                            &format!(
                                "The remote workspace could not \
                                 take the worktree in: {error:#}"
                            ),
                        ),
                    );
                }
                let mut outcome = session
                    .run(&secrets, &name, &arguments, shown.as_ref())
                    .await;
                if let Some(shown) = &shown {
                    let code = outcome
                        .details
                        .as_ref()
                        .and_then(|details| details.get("exitCode"))
                        .and_then(Value::as_i64);
                    shown.ended_run(code);
                }
                if once
                    && let Some(worktree) = &worktree
                    && let Err(error) = worktree.pull().await
                {
                    let words = hamlet_sandbox::scrub::scrub_text(
                        &secrets,
                        &format!("{error:#}"),
                    );
                    warn!("could not bring a worktree home: {words}");
                }
                if let Some(change) = change {
                    outcome
                        .details
                        .get_or_insert_with(Map::new)
                        .insert("change".into(), change);
                }
                outcome
            },
        );
        self.commands.spawn(running);
        Ok(None)
    }

    /// The session the tools of `call`'s agent run in: the remote
    /// workspace the realm works in, with the worktree's remote copy
    /// kept in step, else this machine.
    pub(super) fn tools_of(
        &mut self,
        call: &Call<'_>,
        place: crate::tools::builtin::Place,
        network: Network,
    ) -> Result<(ToolSession, Option<RemoteWorktree>)> {
        let agent = call.conversation.agent;
        let remote = match call.conversation.kind {
            // A dream works on this machine's checkout of its
            // profile.
            Kind::Dream => None,
            _ => self.remote.session(
                self.realm.store().connection(),
                agent,
                network,
            )?,
        };
        Ok(match remote {
            Some(session) => (
                ToolSession::remote(place, session),
                self.remote.worktree(
                    &mut self.realm,
                    &self.writing,
                    agent,
                )?,
            ),
            None => (
                ToolSession::local(
                    place,
                    network,
                    self.tools.sandbox.0,
                    self.tools.shells.clone(),
                    self.tools.leftovers.of(agent.0),
                ),
                None,
            ),
        })
    }

    /// The images the model `agent` runs now takes.
    fn sight_of(&mut self, agent: AgentId) -> Result<Sight> {
        let folder = self.realm.layout().images();
        let db = self.realm.store().connection();
        let model = models::choose(
            db,
            &self.env.settings,
            &self.env.overrides,
            agent,
        )?
        .model;
        Ok(Sight {
            folder,
            limits: self
                .providers
                .images(db, &model)?
                .map(|takes| Limits::of(&takes)),
        })
    }

    /// Tells whether `tool` may not run twice: what writes, runs or
    /// is added by hand.
    fn is_once_only(&self, tool: &str) -> bool {
        self.tools
            .catalogue
            .iter()
            .find(|spec| spec.name == tool)
            .is_none_or(|spec| spec.replay == ToolSpecReplay::Unsafe)
    }
}

/// The rejection of a write or edit of `CHARACTER.md` at the
/// worktree's root, which is the profile's own.
fn character_rejection(
    place: &crate::tools::builtin::Place,
    call: &Call<'_>,
) -> Option<String> {
    if !matches!(call.tool, "write" | "edit") {
        return None;
    }
    let path = call.arguments.get("path")?.as_str()?;
    let path = std::path::Path::new(path);
    let full = if path.is_absolute() {
        path.to_owned()
    } else {
        place
            .folder
            .as_deref()
            .unwrap_or(&place.worktree)
            .join(path)
    };
    (full == place.worktree.join(CHARACTER)).then(|| {
        format!(
            "{CHARACTER} is your profile's character, which only a \
             chief changes. Keep what you learn in memory/ instead."
        )
    })
}
