//! A chief's answer to what waits: checked, kept with what it gives
//! or counts in one change, then told to the calls it ruled on.

use anyhow::Result;
use hamlet_protocol::client::{Choice, Permission, Pick};
use hamlet_protocol::runtime::{self, ServerMessage};
use hamlet_store::ids::{Actor, WaitId};

use super::{chat_session, held};
use crate::core::Core;
use crate::oversight::gate::{self, Asked};
use crate::oversight::{self, Danger, Ruling, Wait, WaitKind};
use crate::society::addresses;
use crate::society::refusal::Refusal;
use crate::talk::{asking, conversations};

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
    Picks(#[from] asking::Unanswered),
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
            asking::answer(questions, picks).map_err(Into::into)
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
    if let Err(unanswered) = asking::answer(questions, &[words]) {
        return Ok(Err(unanswered.into()));
    }
    Ok(Ok(format!("{} {}", asking::ANSWERS, text.trim())))
}

/// Withdraws the chief's instruction of `task`: its turn in
/// `conversation` stops, and the transcript says why.
fn withdraw(
    cx: &mut crate::change::Change<'_>,
    conversation: hamlet_store::ids::ConversationId,
    task: hamlet_store::ids::TaskId,
) -> Result<()> {
    let text = crate::talk::tasks::get(cx.db, task)?
        .text
        .unwrap_or_default();
    crate::talk::inputs::drop_kept(cx.db, conversation)?;
    if crate::talk::inputs::is_busy(cx.db, conversation)? {
        conversations::set_cancelling(cx.db, conversation, true)?;
        conversations::set_stopped(cx.db, conversation, true)?;
    }
    crate::talk::tasks::done(cx.db, task, cx.at)?;
    crate::talk::chats::notice(
        cx,
        conversation,
        &format!("The chief withdrew the instruction: {text}"),
    )?;
    Ok(())
}

impl Core {
    /// A chief's `choice` on the wait `id`, in the words a reply
    /// gives.
    pub(crate) async fn answer_wait(
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
                    return Ok(Err(Refusal::Usage {
                        words: why.to_string(),
                    }));
                }
            };
        let text =
            self.credentials.scrub_text(&text).trim().to_owned();
        let ruling = Ruling::of(choice);
        let allow_net = self.env.settings.allow_net;
        let db = self.realm.store().connection();
        let holding = held::holding(db, wait.id)?;
        let mut ruled = Vec::new();
        for call in holding {
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
            ruled.push((call, conversation, net));
        }
        let by = match wait.agent {
            Some(agent) => addresses::name_of(db, agent)?,
            None => "the realm".into(),
        };
        let settled = self
            .change(Actor::Chief(chief), |cx, _| {
                if !oversight::settle(
                    cx,
                    wait.id,
                    ruling,
                    &text,
                    Some(chief),
                )? {
                    return Ok(false);
                }
                if wait.kind == WaitKind::Objection {
                    super::objections::rule(
                        cx.db,
                        wait.id,
                        ruling == Ruling::Go,
                        cx.at,
                    )?;
                    if ruling == Ruling::Withdraw
                        && let (Some(task), Some(conversation)) =
                            (wait.task, wait.conversation)
                    {
                        withdraw(cx, conversation, task)?;
                    }
                }
                if ruling == Ruling::Always
                    && let (
                        Some(
                            danger @ (Danger::Network
                            | Danger::Unconfined),
                        ),
                        Some(agent),
                    ) = (wait.danger, wait.agent)
                {
                    let permission = if danger == Danger::Network {
                        Permission::Network
                    } else {
                        Permission::Unconfined
                    };
                    gate::give(
                        cx.db,
                        agent,
                        &permission,
                        chief,
                        cx.at,
                    )?;
                }
                let mut struck = Vec::new();
                for (call, conversation, net) in &ruled {
                    if ruling.is_allowing() {
                        held::set_net(
                            cx.db,
                            &call.runtime,
                            &call.key,
                            *net,
                        )?;
                    }
                    if ruling == Ruling::Always
                        && let Some(
                            danger @ (Danger::Edit | Danger::Command),
                        ) = wait.danger
                        && let Some(session) =
                            chat_session(conversation)
                    {
                        gate::trust(cx.db, session, danger)?;
                    }
                    if ruling == Ruling::Deny
                        && let Some(danger) = wait.danger
                        && !struck.contains(&conversation.id)
                    {
                        let mut asked = Asked::of(
                            cx.db,
                            conversation.id,
                            call.input.as_deref(),
                        )?;
                        asked.refuse(
                            danger,
                            &wait.about,
                            if text.is_empty() {
                                "no reason given"
                            } else {
                                &text
                            },
                        );
                        asked.keep(cx.db, conversation.id)?;
                        struck.push(conversation.id);
                    }
                }
                Ok(true)
            })
            .await?;
        if !settled {
            return Ok(Err(Refusal::NoWait));
        }
        for (call, _, _) in ruled {
            self.wake_call(&call.runtime, &call.key).await;
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
                )
                .await;
            }
            self.flush(false).await?;
        }
        Ok(Ok(format!(
            "Answered /{ruling}: {}",
            oversight::line(&wait, &by, 80)
        )))
    }

    /// Tells the runtime of a held call that its wait was answered,
    /// so asking again may get further.
    pub(crate) async fn wake_call(
        &mut self,
        runtime: &str,
        key: &str,
    ) {
        let Some(index) = self.runtimes.index_of(runtime) else {
            return;
        };
        let key = key.to_owned();
        self.runtimes
            .send(index, |v, id| {
                ServerMessage::Answered(runtime::Answered {
                    id,
                    key,
                    v,
                })
            })
            .await;
    }
}
