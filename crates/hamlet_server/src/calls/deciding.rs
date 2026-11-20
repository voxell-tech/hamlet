//! The gate's decision of a call (C7, H11), made once and kept.

use anyhow::Result;
use hamlet_store::ids::Actor;

use super::held::{self, State};
use super::looks::Look;
use super::objections::OBJECTING;
use super::place::{self, Workplace};
use super::society::args::{Called, Notify, Request};
use super::{
    Call, Decision, NOBODY, STOPPED, WAITING, chat_session, society,
};
use crate::core::Core;
use crate::society::oversight::gate::{
    self, Standing, TurnGate, Verdict,
};
use crate::society::oversight::{self, Raise, Ruling, WaitKind};
use crate::tools::builtin::Network;
use crate::tools::catalogue::Society;
use crate::tools::edit_diff::change_between;

/// A call as the gate rules on it, or the decision made before it.
type Classified = Result<Result<gate::Call, Decision>>;

impl Core<'_, '_> {
    /// The gate's decision of `call`: the one kept, else one made now
    /// and kept, raising a wait when it needs a chief's answer.
    pub(super) fn decide(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Decision> {
        if let Some(decision) = self.kept_decision(call)? {
            return Ok(decision);
        }
        if call.conversation.cancelling {
            return self.block(call, STOPPED.into());
        }
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let at =
            place::of_conversation(db, &layout, &call.conversation)?;
        match self.classify(call, &at)? {
            Ok(classified) => self.rule(call, &at, classified),
            Err(decision) => Ok(decision),
        }
    }

    /// The decision kept for `call`, when one was made and holds.
    fn kept_decision(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Option<Decision>> {
        let db = self.realm.store().connection();
        let Some(kept) =
            held::kept(db, &call.call.runtime, &call.call.key)?
        else {
            return Ok(None);
        };
        Ok(Some(match kept.state {
            State::Blocked(words) => Decision::Block(words),
            State::Ruled { ruling, net, .. }
                if ruling.is_allowing() =>
            {
                Decision::Run { net }
            }
            State::Ruled {
                ruling: Ruling::Deny,
                text,
                ..
            } => Decision::Block(format!(
                "the chief refused it: {}",
                if text.is_empty() {
                    "no reason given"
                } else {
                    &text
                }
            )),
            State::Ruled {
                ruling: Ruling::Stopped,
                ..
            } => Decision::Block(STOPPED.into()),
            State::Ruled { .. } => Decision::Block(NOBODY.into()),
            State::Held(_) => Decision::Wait(WAITING),
            State::Objected => Decision::Wait(OBJECTING),
            State::Allowed { .. } | State::Open => return Ok(None),
        }))
    }

    /// Blocks `call` for good, in `words`.
    fn block(
        &mut self,
        call: &Call<'_>,
        words: String,
    ) -> Result<Decision> {
        let db = self.realm.store().connection();
        held::block(
            db,
            &call.call.runtime,
            &call.call.key,
            &words,
            None,
        )?;
        Ok(Decision::Block(words))
    }

    /// `call` as the gate rules on it: a send with what its files
    /// come to, an edit with what it replaces. `Err` is the decision
    /// while they are looked at, or once they cannot be.
    fn classify(
        &mut self,
        call: &Call<'_>,
        at: &Workplace,
    ) -> Classified {
        let added = self
            .tools
            .added
            .iter()
            .find(|manifest| manifest.spec.name == call.tool)
            .map(|manifest| gate::Reach {
                net: manifest.has_net(),
                acts: false,
            })
            .or_else(|| self.tools.mcp.reach_of(call.tool));
        let classified =
            gate::call_of(call.tool, &call.args(), added);
        match classified {
            gate::Call::Society {
                tool: tool @ (Society::Notify | Society::Request),
                args,
            } => self.measured_send(call, at, tool, args),
            gate::Call::Edit {
                write: true,
                path,
                old,
                now,
                created,
            } => self.previewed(call, at, path, old, now, created),
            classified => Ok(Ok(classified)),
        }
    }

    /// A notification or a request as the gate rules on it: with the
    /// files it sends, once they are measured.
    fn measured_send(
        &mut self,
        call: &Call<'_>,
        at: &Workplace,
        tool: Society,
        args: serde_json::Value,
    ) -> Classified {
        // Arguments that do not fit are rejected once the call runs.
        let paths = match Called::read(tool, call.arguments) {
            Ok(
                Called::Notify(Notify { paths, .. })
                | Called::Request(Request { paths, .. }),
            ) => paths,
            _ => Vec::new(),
        };
        if paths.is_empty() {
            return Ok(Ok(gate::Call::Society { tool, args }));
        }
        let Some(measured) = self.looks.measured(&call.call).cloned()
        else {
            let (session, worktree) = self.tools_of(
                call,
                at.place.clone(),
                Network::Closed,
            )?;
            return Ok(Err(Decision::Look(Box::new(
                Look::Measure {
                    session,
                    worktree,
                    paths,
                },
            ))));
        };
        Ok(match measured {
            Ok((files, bytes)) => Ok(gate::Call::Parcel {
                name: tool.name().to_owned(),
                args,
                files,
                bytes,
            }),
            Err(rejection) => {
                Err(self.block(call, rejection.to_string())?)
            }
        })
    }

    /// A write as the gate rules on it: with the file it replaces,
    /// once that is read.
    fn previewed(
        &mut self,
        call: &Call<'_>,
        at: &Workplace,
        path: String,
        mut old: String,
        now: String,
        mut created: bool,
    ) -> Classified {
        let Some(previewed) =
            self.looks.previewed(&call.call).cloned()
        else {
            let (session, worktree) = self.tools_of(
                call,
                at.place.clone(),
                Network::Closed,
            )?;
            return Ok(Err(Decision::Look(Box::new(
                Look::Preview {
                    session,
                    worktree,
                    path,
                },
            ))));
        };
        match previewed {
            Ok(Some(before)) => {
                old = before;
                created = false;
            }
            Ok(None) => {}
            Err(words) => return Ok(Err(self.block(call, words)?)),
        }
        Ok(Ok(gate::Call::Edit {
            write: true,
            path,
            old,
            now,
            created,
        }))
    }

    /// The gate's rule on `classified`, made at `at`, kept with the
    /// change it shows and the wait it raises.
    fn rule(
        &mut self,
        call: &Call<'_>,
        at: &Workplace,
        classified: gate::Call,
    ) -> Result<Decision> {
        let agent = call.conversation.agent;
        let (session, _) =
            self.tools_of(call, at.place.clone(), Network::Closed)?;
        let (change, worktree_edit) = match &classified {
            gate::Call::Edit {
                write,
                path,
                old,
                now,
                created,
            } => {
                let mut change =
                    serde_json::to_value(change_between(
                        path,
                        old,
                        now,
                        *write && *created,
                    ))?;
                self.credentials.scrub(&mut change);
                (
                    Some(change.to_string()),
                    session.is_worktree_edit(path),
                )
            }
            _ => (None, false),
        };
        let about = self
            .credentials
            .scrub_text(&gate::call_text(&classified));
        let allow_net = self.env.settings.allow_net;
        let task = self.task_of(call)?;
        let db = self.realm.store().connection();
        let limits = society::limits_of(db, at.hamlet.as_ref())?;
        let (edit, command) = match chat_session(&call.conversation) {
            Some(session) => gate::session_granted(db, session)?,
            None => (false, false),
        };
        let acts =
            gate::tool_granted(db, call.conversation.id, call.tool)?;
        let standing = Standing {
            worktree_edit,
            net: allow_net
                || gate::has_grant(
                    db,
                    agent,
                    &hamlet_protocol::client::Permission::Network,
                )?,
            holds_edits: at.manual && !edit,
            holds_commands: at.manual && !command,
            acts_granted: acts,
        };
        let gate =
            TurnGate::of(db, call.conversation.id, call.input)?;
        let verdict = gate::rule(
            &classified,
            &standing,
            at.caller,
            &limits.reserved,
            &gate,
            &about,
            limits.asking.refusals,
        );
        let (runtime, key) =
            (call.call.runtime.clone(), call.call.key.clone());
        let conversation = call.conversation.id;
        self.change(Actor::Realm, |cx, _| {
            let change = change.as_deref();
            Ok(match verdict {
                Verdict::Run { net } => {
                    held::allow(cx.db, &runtime, &key, net, change)?;
                    Decision::Run { net }
                }
                Verdict::Block(words) => {
                    gate.keep(cx.db, conversation)?;
                    held::block(
                        cx.db, &runtime, &key, &words, change,
                    )?;
                    Decision::Block(words)
                }
                Verdict::Hold(danger) => {
                    gate.keep(cx.db, conversation)?;
                    let wait = oversight::raise(
                        cx,
                        Raise {
                            kind: WaitKind::Approval,
                            danger: Some(danger),
                            agent,
                            profile: None,
                            conversation: Some(conversation),
                            task,
                            about,
                            change: change.map(String::from),
                            answers: danger.answers(),
                            questions: None,
                            setup: None,
                            server: None,
                        },
                    )?;
                    held::hold(cx.db, &runtime, &key, wait, change)?;
                    Decision::Wait(WAITING)
                }
            })
        })
    }
}
