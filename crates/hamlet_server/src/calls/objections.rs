//! An agent's objection to a chief's instruction (H10): one per task,
//! a wait the chief rules on, and every call of the turn held until
//! it does.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_protocol::client::Choice;
use hamlet_store::ids::{Actor, TaskId, WaitId};
use hamlet_store::schema::{objection, wait};

use super::{Answer, Call, held, said};
use crate::core::Core;
use crate::oversight::{self, Raise, WaitKind};
use crate::talk::marks::Mark;
use crate::talk::{inputs, tasks};
use crate::work::tools::Outcome;

/// The words of a call held while the chief considers an objection.
pub(super) const OBJECTING: &str =
    "The chief is considering an objection.";

/// An objection as it is kept.
struct Kept {
    wait: Option<WaitId>,
    ruling: Option<String>,
}

fn kept(
    db: &mut hamlet_store::Db,
    task: TaskId,
) -> Result<Option<Kept>> {
    Ok(objection::table
        .find(task)
        .select((objection::wait, objection::ruling))
        .first::<(Option<WaitId>, Option<String>)>(db)
        .optional()?
        .map(|(wait, ruling)| Kept { wait, ruling }))
}

impl Core {
    /// The chief's instruction the turn of `call` works on: its task,
    /// when the turn is a chief's line.
    fn instruction(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Option<TaskId>> {
        let Some(submission) = call.input else {
            return Ok(None);
        };
        let db = self.realm.store().connection();
        let Some(input) = inputs::by_submission(
            db,
            call.conversation.id,
            submission,
        )?
        else {
            return Ok(None);
        };
        Ok(match input.mark()? {
            Mark::Chief { .. } => input.task,
            _ => None,
        })
    }

    /// Whether a chief withdrew the instruction of `call`'s turn, so
    /// none of its calls runs any more.
    pub(super) fn withdrawn(
        &mut self,
        call: &Call<'_>,
    ) -> Result<bool> {
        let Some(task) = self.instruction(call)? else {
            return Ok(false);
        };
        let db = self.realm.store().connection();
        Ok(kept(db, task)?
            .and_then(|found| found.ruling)
            .is_some_and(|ruling| ruling == "withdraw"))
    }

    /// Whether a chief considers an objection to the instruction of
    /// `call`'s turn, holding the call on it until the chief rules.
    pub(super) fn objecting(
        &mut self,
        call: &Call<'_>,
    ) -> Result<bool> {
        let Some(task) = self.instruction(call)? else {
            return Ok(false);
        };
        let db = self.realm.store().connection();
        let Some(Kept {
            wait: Some(on),
            ruling: None,
        }) = kept(db, task)?
        else {
            return Ok(false);
        };
        let open = wait::table
            .find(on)
            .select(wait::open)
            .first::<bool>(db)?;
        if open {
            held::hold(
                db,
                &call.call.runtime,
                &call.call.key,
                on,
                None,
            )?;
        }
        Ok(open)
    }

    /// Objects to the instruction of `call`'s turn, or gives the
    /// chief's ruling once there is one.
    pub(super) async fn object(
        &mut self,
        call: &Call<'_>,
    ) -> Result<std::result::Result<Outcome, Answer>> {
        let Some(task) = self.instruction(call)? else {
            return Ok(Ok(said(
                "Only a chief's instruction can be objected to, in \
                 the turn that works on it.",
                true,
            )));
        };
        let db = self.realm.store().connection();
        if let Some(found) = kept(db, task)? {
            let text = match found.wait {
                Some(on) => wait::table
                    .find(on)
                    .select(wait::text)
                    .first::<Option<String>>(db)?
                    .unwrap_or_default(),
                None => String::new(),
            };
            let said_too = if text.is_empty() {
                String::new()
            } else {
                format!(" The chief says: {text}")
            };
            return Ok(match found.ruling.as_deref() {
                Some("go") => Ok(said(
                    format!(
                        "The chief rules: go on with the \
                         instruction.{said_too}"
                    ),
                    false,
                )),
                Some(_) => Ok(said(
                    format!(
                        "The chief withdrew the instruction. Stop \
                         working on it.{said_too}"
                    ),
                    false,
                )),
                None => Err(Answer::Waiting(OBJECTING.into())),
            });
        }
        let reason = call
            .arguments
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if reason.is_empty() {
            return Ok(Ok(said("Say why you object.", true)));
        }
        let instruction =
            tasks::get(db, task)?.text.unwrap_or_default();
        let reason = self.credentials.scrub_text(&reason);
        let agent = call.conversation.agent;
        let name = crate::society::addresses::name_of(db, agent)?;
        let about = format!(
            "A chief told `{name}`: {instruction}\n`{name}` \
             objects: {reason}"
        );
        let conversation = call.conversation.id;
        let (runtime, key) =
            (call.call.runtime.clone(), call.call.key.clone());
        self.change(Actor::Agent(agent), |cx, _| {
            let on = oversight::raise(
                cx,
                Raise {
                    kind: WaitKind::Objection,
                    danger: None,
                    agent,
                    conversation: Some(conversation),
                    task: Some(task),
                    about,
                    change: None,
                    answers: vec![Choice::Go, Choice::Withdraw],
                    questions: None,
                },
            )?;
            diesel::insert_into(objection::table)
                .values((
                    objection::task.eq(task),
                    objection::agent.eq(agent),
                    objection::source.eq("agent"),
                    objection::reason.eq(&reason),
                    objection::wait.eq(on),
                    objection::at.eq(cx.at),
                ))
                .execute(cx.db)?;
            held::hold(cx.db, &runtime, &key, on, None)
        })
        .await?;
        Ok(Err(Answer::Waiting(OBJECTING.into())))
    }
}

/// Keeps a chief's ruling on the objection `on` waited on.
pub(super) fn rule(
    db: &mut hamlet_store::Db,
    on: WaitId,
    go: bool,
    at: i64,
) -> Result<()> {
    diesel::update(objection::table.filter(objection::wait.eq(on)))
        .set((
            objection::ruling.eq(if go { "go" } else { "withdraw" }),
            objection::ruled_at.eq(at),
        ))
        .execute(db)?;
    Ok(())
}
