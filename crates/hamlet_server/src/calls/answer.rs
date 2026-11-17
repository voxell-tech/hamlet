//! A chief's answer to what waits: checked, kept with what it gives
//! or counts in one change, then told to the calls it ruled on.

use anyhow::Result;
use hamlet_protocol::client::{Choice, Permission, Pick};
use hamlet_store::ids::{Actor, ChiefId, WaitId};

use super::{held, trusting};
use crate::app::follow_up::WakeHeldCall;
use crate::conversations::{
    self, Conversation, chats, inputs, questions, tasks,
};
use crate::core::Core;
use crate::realm::change::Change;
use crate::society::addresses;
use crate::society::oversight::gate::{self, TurnGate};
use crate::society::oversight::{
    self, Danger, Ruling, Wait, WaitKind,
};
use crate::society::refusal::Refusal;

/// The fault in a chief's choice that cannot answer a wait so.
#[derive(Debug, thiserror::Error)]
enum Unanswerable {
    #[error("That cannot be answered that way; answer with {0}.")]
    NotThatWay(String),
    #[error("Use picks only for a reply, without text.")]
    PicksWithText,
    #[error("This wait does not take structured answers.")]
    NoPicks,
    #[error("Say what to reply.")]
    NoWords,
    #[error("This wait does not take a reply.")]
    NoReply,
    #[error(transparent)]
    Picks(#[from] questions::Unanswered),
}

/// The words of `choice` on `wait`: a reply's text, or the answer its
/// picks make.
fn words(
    wait: &Wait,
    choice: &Choice,
    text: &str,
    picks: Option<&[Pick]>,
) -> Result<std::result::Result<String, Unanswerable>> {
    let answers = wait.answers()?;
    if !answers.contains(choice) {
        let named = answers
            .iter()
            .map(|choice| format!("/{}", Ruling::of(choice)))
            .collect::<Vec<_>>()
            .join(" ");
        return Ok(Err(Unanswerable::NotThatWay(named)));
    }
    let questions = wait.questions()?;
    if let Some(picks) = picks {
        if *choice != Choice::Reply || !text.is_empty() {
            return Ok(Err(Unanswerable::PicksWithText));
        }
        let Some(questions) = &questions else {
            return Ok(Err(Unanswerable::NoPicks));
        };
        return Ok(
            questions::answer(questions, picks).map_err(Into::into)
        );
    }
    if *choice != Choice::Reply {
        return Ok(Ok(text.into()));
    }
    if text.trim().is_empty() {
        return Ok(Err(Unanswerable::NoWords));
    }
    let Some(questions) = &questions else {
        return Ok(Err(Unanswerable::NoReply));
    };
    let words = Pick::Words { text: text.into() };
    if let Err(unanswered) = questions::answer(questions, &[words]) {
        return Ok(Err(unanswered.into()));
    }
    Ok(Ok(format!("{} {}", questions::ANSWERS, text.trim())))
}

/// Withdraws the chief's instruction of `task`: its turn in
/// `conversation` stops, and the transcript says why.
fn withdraw(
    cx: &mut Change<'_>,
    conversation: hamlet_store::ids::ConversationId,
    task: hamlet_store::ids::TaskId,
) -> Result<()> {
    let text = tasks::get(cx.db, task)?.text.unwrap_or_default();
    inputs::drop_kept(cx.db, conversation)?;
    if inputs::is_busy(cx.db, conversation)? {
        conversations::set_cancelling(cx.db, conversation, true)?;
        conversations::set_stopped(cx.db, conversation, true)?;
    }
    tasks::done(cx.db, task, cx.at)?;
    chats::notice(
        cx,
        conversation,
        &format!("The chief withdrew the instruction: {text}"),
    )?;
    Ok(())
}

/// A call a wait held, as its ruling finds it.
struct Ruled {
    call: held::Holding,
    conversation: Conversation,
    /// It reaches the network once let run.
    net: bool,
}

/// The effect of `ruling` on `wait` of its own kind: a newcomer let
/// in, an objection ruled on and its instruction withdrawn.
fn rule_wait(
    cx: &mut Change<'_>,
    wait: &Wait,
    ruling: Ruling,
    text: &str,
) -> Result<()> {
    // A newcomer gone or let in since stays as it is.
    if wait.kind == WaitKind::Newcomer
        && ruling == Ruling::Welcome
        && let Some(scholar) = wait.scholar
    {
        let _ = crate::society::city::welcome(cx, scholar, text)?;
    }
    if wait.kind == WaitKind::Objection {
        super::objections::rule(cx.db, wait.id, ruling, cx.at)?;
        if ruling == Ruling::Withdraw
            && let (Some(task), Some(conversation)) =
                (wait.task, wait.conversation)
        {
            withdraw(cx, conversation, task)?;
        }
    }
    Ok(())
}

/// The lasting grant an `/always` of the network or of running
/// unconfined gives the agent that waited.
fn grant_always(
    cx: &mut Change<'_>,
    wait: &Wait,
    ruling: Ruling,
    chief: ChiefId,
) -> Result<()> {
    if ruling == Ruling::Always
        && let (
            Some(danger @ (Danger::Network | Danger::Unconfined)),
            Some(agent),
        ) = (wait.danger, wait.agent)
    {
        let permission = if danger == Danger::Network {
            Permission::Network
        } else {
            Permission::Unconfined
        };
        gate::give(cx.db, agent, &permission, chief, cx.at)?;
    }
    Ok(())
}

/// The effect of the ruling of `by` on each call `wait` held: its
/// network kept when it may run, its conversation trusted after an
/// `/always`, and its turn told once of a denial.
fn rule_calls(
    cx: &mut Change<'_>,
    wait: &Wait,
    by: (Ruling, ChiefId),
    text: &str,
    ruled: &[Ruled],
) -> Result<()> {
    let (ruling, chief) = by;
    let mut struck = Vec::new();
    for Ruled {
        call,
        conversation,
        net,
    } in ruled
    {
        if ruling.is_allowing() {
            held::set_net(cx.db, &call.runtime, &call.key, *net)?;
        }
        if ruling == Ruling::Always
            && let Some(danger @ (Danger::Edit | Danger::Command)) =
                wait.danger
            && let Some(trusted) = trusting(conversation)
        {
            gate::trust(cx.db, trusted, chief, danger)?;
        }
        if ruling == Ruling::Deny
            && let Some(danger) = wait.danger
            && !struck.contains(&conversation.id)
        {
            let mut gate = TurnGate::of(
                cx.db,
                conversation.id,
                call.input.as_deref(),
            )?;
            let why = if text.is_empty() {
                "no reason given"
            } else {
                text
            };
            gate.refuse(danger, &wait.about, why);
            gate.keep(cx.db, conversation.id)?;
            struck.push(conversation.id);
        }
    }
    Ok(())
}

impl Core<'_, '_> {
    /// A chief's `choice` on the wait `id`, in the words a reply
    /// gives.
    pub(crate) fn answer_wait(
        &mut self,
        id: &str,
        choice: &Choice,
        text: Option<&str>,
        picks: Option<&[Pick]>,
    ) -> Result<std::result::Result<String, Refusal>> {
        let chief = self.realm.chief();
        let Ok(number) = id.trim_start_matches('#').parse::<i64>()
        else {
            return Ok(Err(Refusal::NoWait));
        };
        let db = self.realm.store().connection();
        let Some(wait) = oversight::find(db, WaitId(number))?
            .filter(|wait| wait.open)
        else {
            return Ok(Err(Refusal::NoWait));
        };
        if let Some(agent) = wait.agent
            && let Err(refusal) = self.reaches(agent)?
        {
            return Ok(Err(refusal));
        }
        let text =
            match words(&wait, choice, text.unwrap_or(""), picks)? {
                Ok(text) => text,
                Err(why) => {
                    return Ok(Err(Refusal::WaitUnanswerable {
                        why: why.to_string(),
                    }));
                }
            };
        let text =
            self.credentials.scrub_text(&text).trim().to_owned();
        let ruling = Ruling::of(choice);
        let ruled = self.held_calls(&wait, ruling)?;
        let db = self.realm.store().connection();
        let by = match wait.agent {
            Some(agent) => addresses::name_of(db, agent)?,
            None => "the realm".into(),
        };
        let settled = self.change(Actor::Chief(chief), |cx, _| {
            if !oversight::settle(
                cx,
                wait.id,
                ruling,
                &text,
                Some(chief),
            )? {
                return Ok(false);
            }
            rule_wait(cx, &wait, ruling, &text)?;
            grant_always(cx, &wait, ruling, chief)?;
            rule_calls(cx, &wait, (ruling, chief), &text, &ruled)?;
            Ok(true)
        })?;
        if !settled {
            return Ok(Err(Refusal::NoWait));
        }
        for ruled in ruled {
            self.wake_call(&ruled.call.runtime, &ruled.call.key);
        }
        if wait.kind == WaitKind::Objection {
            if let (Some(task), Some(conversation)) =
                (wait.task, wait.conversation)
            {
                let db = self.realm.store().connection();
                let runtime =
                    conversations::get(db, conversation)?.runtime;
                self.wake_call(
                    &runtime,
                    &super::reviews::hold_key(task),
                );
            }
            // What the objection held may go to its runtime now.
            self.changed.runtime_answered = true;
        }
        Ok(Ok(format!(
            "Answered /{ruling}: {}",
            oversight::line(&wait, &by, 80)
        )))
    }

    /// The calls `wait` holds, each with its conversation and whether
    /// it reaches the network once `ruling` lets it run.
    fn held_calls(
        &mut self,
        wait: &Wait,
        ruling: Ruling,
    ) -> Result<Vec<Ruled>> {
        let allow_net = self.env.settings.allow_net;
        let db = self.realm.store().connection();
        let mut ruled = Vec::new();
        for call in held::holding(db, wait.id)? {
            let conversation =
                conversations::get(db, call.conversation)?;
            let net = ruling.is_allowing()
                && match wait.danger {
                    Some(Danger::Network) => true,
                    Some(Danger::Edit | Danger::Command) => {
                        allow_net
                            || gate::has_grant(
                                db,
                                conversation.agent,
                                &Permission::Network,
                            )?
                    }
                    _ => false,
                };
            ruled.push(Ruled {
                call,
                conversation,
                net,
            });
        }
        Ok(ruled)
    }

    /// Tells the runtime of a held call that its wait was answered,
    /// so a repeated request may get further, once the input's
    /// changes are sent.
    pub(crate) fn wake_call(&mut self, runtime: &str, key: &str) {
        self.wakes.write(WakeHeldCall {
            runtime: runtime.to_owned(),
            key: key.to_owned(),
        });
    }
}
