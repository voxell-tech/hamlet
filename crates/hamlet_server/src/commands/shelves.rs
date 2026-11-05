//! A chief's commands on skills: giving one to a scholar, and taking
//! one off the library's shelves.

use anyhow::Result;
use hamlet_store::ids::{Actor, HamletId};

use crate::core::Core;
use crate::knowledge::library;
use crate::society::library::{self as shelf, Event, EventKind};
use crate::society::refusal::Refusal;

impl Core {
    /// Runs the command `name` with `words` when it is one of these.
    pub(super) async fn shelves_command(
        &mut self,
        here: Option<&HamletId>,
        name: &str,
        words: &[&str],
    ) -> Result<Option<Result<String, Refusal>>> {
        Ok(Some(match name {
            "give" => self.give_command(here, words).await?,
            "unshelve" => self.unshelve(words)?,
            _ => return Ok(None),
        }))
    }

    /// `/unshelve <skill> [reason]`
    fn unshelve(
        &mut self,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let [skill, reason @ ..] = words else {
            return Ok(Err(super::usage("unshelve")));
        };
        let reason = reason.join(" ");
        let chief = self.realm.chief();
        let layout = self.realm.layout().clone();
        let by = Actor::Chief(chief).to_string();
        let at = crate::clock::now();
        let message = if reason.is_empty() {
            format!("Took {skill} off the shelves")
        } else {
            format!("Took {skill} off the shelves: {reason}")
        };
        let Some(commit) =
            library::unshelve(&layout, skill, &message, &by, at)?
        else {
            return Ok(Err(Refusal::NoSkill {
                name: (*skill).into(),
            }));
        };
        let db = self.realm.store().connection();
        shelf::event(
            db,
            at,
            Event {
                kind: EventKind::Unshelved,
                skill,
                agent: None,
                chief: Some(chief),
                reason: (!reason.is_empty())
                    .then_some(reason.as_str()),
                commit: Some(&commit.to_string()),
            },
        )?;
        Ok(Ok(format!("Took {skill} off the shelves.")))
    }
}
