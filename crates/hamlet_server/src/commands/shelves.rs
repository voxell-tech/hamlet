//! A chief's commands on skills and dreams: giving a skill to a
//! scholar, taking one off the library's shelves, and having a hire
//! or a scholar dream.

use anyhow::Result;
use bevy_ecs::system::In;
use hamlet_store::ids::Actor;

use super::{Answer, Line};
use crate::core::Core;
use crate::knowledge::library;
use crate::society::library::{self as shelf, Event, EventKind};
use crate::society::refusal::Refusal;

/// Runs a command on the library's shelves or a dream.
pub(super) fn run(In(line): In<Line>, mut core: Core) -> Answer {
    let words = line.words();
    match line.name.as_str() {
        "unshelve" => core.unshelve(&words),
        // "dream".
        _ => core.dream_command(line.here.as_ref(), &words),
    }
}

impl Core<'_, '_> {
    fn unshelve(
        &mut self,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let [skill, reason @ ..] = words else {
            return Ok(Err(self.deeds.usage("unshelve")));
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
        let writing = self.writing.clone();
        let held = writing.hold();
        let unshelved =
            library::unshelve(&layout, skill, &message, &by, at)?;
        drop(held);
        let Some(commit) = unshelved else {
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
