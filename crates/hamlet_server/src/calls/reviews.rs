//! A lead's review of a chief's instruction to a hire under it (H10):
//! an errand run alongside the hire's work once, which may object for
//! the chief to rule on; and the hold of the hire's next model call
//! while a chief considers an objection.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::ids::{Actor, AgentId, TaskId};
use hamlet_store::schema::{objection, review, wait};

use super::objections::OBJECTING;
use crate::core::Core;
use crate::providers::{FAUX, is_local};
use crate::society::limits::{self, Review};
use crate::society::{addresses, hires};
use crate::talk::conversations::{Conversation, Kind};
use crate::talk::errands::{self, Errand};
use crate::talk::marks::Mark;
use crate::talk::{inputs, models};

/// The errand a lead's review is.
pub(crate) const REVIEW: &str = "review";

/// The key a turn's model call waits under while its instruction is
/// held.
pub(crate) fn hold_key(task: TaskId) -> String {
    format!("task:{task}")
}

impl Core {
    /// Before a model call of `conversation` in the turn of
    /// `submission`: the lead's review of a chief's instruction,
    /// begun once, and the hold while a chief considers an objection
    /// to it, as its key and words.
    pub(crate) async fn before_prompt(
        &mut self,
        conversation: &Conversation,
        submission: Option<&str>,
    ) -> Result<Option<(String, String)>> {
        let Some(submission) = submission else {
            return Ok(None);
        };
        let db = self.realm.store().connection();
        let Some(input) =
            inputs::by_submission(db, conversation.id, submission)?
        else {
            return Ok(None);
        };
        let (Mark::Chief { chief, text, .. }, Some(task)) =
            (input.mark()?, input.task)
        else {
            return Ok(None);
        };
        let open = objection::table
            .inner_join(wait::table)
            .filter(objection::task.eq(task))
            .filter(objection::ruling.is_null())
            .filter(wait::open.eq(true))
            .count()
            .get_result::<i64>(db)?
            > 0;
        if open {
            return Ok(Some((hold_key(task), OBJECTING.into())));
        }
        if conversation.kind != Kind::Chat {
            return Ok(None);
        }
        let Some(hire) = hires::find(db, conversation.agent)? else {
            return Ok(None);
        };
        let Some(lead) = hire.parent else {
            return Ok(None);
        };
        let begun =
            review::table.find(task).count().get_result::<i64>(db)?
                > 0;
        if begun || !self.reviews(lead, &hire.hamlet)? {
            return Ok(None);
        }
        let db = self.realm.store().connection();
        let name = addresses::name_of(db, hire.agent)?;
        let lead_name = addresses::name_of(db, lead)?;
        let mut under = Vec::new();
        for each in crate::society::tree::under(db, lead)? {
            let found = hires::get(db, each)?;
            under.push(format!(
                "- {}: {}",
                addresses::name_of(db, each)?,
                found.purpose
            ));
        }
        let instructions = format!(
            "You are `{lead_name}`, and you lead `{name}`, hired to \
             {}. A chief has just instructed `{name}` directly. The \
             work has begun already, and the chief always decides. \
             If the instruction is a mistake for the work you lead, \
             answer `OBJECT: <reason>`. Otherwise answer `PASS`. \
             Most instructions are fine.\n\n# The hires you lead\n{}",
            hire.purpose.trim_end_matches('.'),
            under.join("\n")
        );
        let question = format!("{chief} told `{name}`: {text}");
        let owner = format!("review:{task}");
        let at = crate::clock::now();
        self.change(Actor::Agent(lead), |cx, env| {
            let id = errands::run(
                cx,
                env,
                Errand {
                    kind: REVIEW,
                    owner: &owner,
                    agent: lead,
                    instructions: &instructions,
                    question: &question,
                    task: Some(task),
                },
            )?;
            diesel::insert_into(review::table)
                .values((
                    review::task.eq(task),
                    review::lead.eq(lead),
                    review::conversation.eq(id),
                    review::at.eq(at),
                ))
                .execute(cx.db)?;
            Ok(())
        })
        .await?;
        Ok(None)
    }

    /// Whether `lead` reviews instructions to its hires in `hamlet`:
    /// always, never, or when its model is served from another
    /// machine.
    fn reviews(
        &mut self,
        lead: AgentId,
        hamlet: &hamlet_store::ids::HamletId,
    ) -> Result<bool> {
        let db = self.realm.store().connection();
        Ok(match limits::of(db, hamlet)?.asking.review {
            Review::Always => true,
            Review::Never => false,
            Review::Remote => {
                let choice = models::choose(
                    db,
                    &self.env.settings,
                    &self.env.overrides,
                    lead,
                )?;
                choice.model.provider != FAUX
                    && !is_local(choice.model.base_url.as_deref())
            }
        })
    }
}
