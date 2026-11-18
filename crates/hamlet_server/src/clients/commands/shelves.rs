//! A chief's commands on skills and dreams: giving a skill to a
//! profile, taking one off the library's shelves, and having an
//! agent or a profile dream.

use hamlet_store::ids::Actor;

use super::Command;
use crate::core::Core;
use crate::knowledge::library;
use crate::realm::change::Done;
use crate::society::library::{self as shelf, Event, EventKind};
use crate::society::rejection::Rejection;

/// The commands about the library's shelves and dreams.
pub(super) const SHELVES: &[Command] = &[
    Command {
        name: "unshelve",
        usage: "/unshelve <skill> [reason]",
        does: "Take a skill off the library's shelves",
        run: |core, line| core.unshelve(&line.words()),
    },
    Command {
        name: "dream",
        usage: "/dream <profile#id|address>",
        does: "Have an agent bring its learning home, or a profile \
               tidy",
        run: |core, line| {
            core.dream_command(line.here.as_ref(), &line.words())
        },
    },
];

impl Core<'_, '_> {
    pub(super) fn unshelve(
        &mut self,
        words: &[&str],
    ) -> Done<String> {
        let [skill, reason @ ..] = words else {
            return Ok(Err(self.actions.usage("unshelve")));
        };
        let reason = reason.join(" ");
        let chief = self.realm.chief();
        let layout = self.realm.layout().clone();
        let by = Actor::Chief(chief).to_string();
        let at = crate::app::clock::now();
        let message = if reason.is_empty() {
            format!("Took {skill} off the shelves")
        } else {
            format!("Took {skill} off the shelves: {reason}")
        };
        let writing = self.writing.clone();
        let held = writing.hold();
        let unshelved =
            library::unshelve(&layout, skill, &message, &by, at)?;
        drop(held);
        let Some(commit) = unshelved else {
            return Ok(Err(Rejection::NoSkill {
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
