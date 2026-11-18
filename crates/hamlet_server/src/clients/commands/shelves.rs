//! A chief's commands on skills and dreams.

use hamlet_store::ids::Actor;

use super::Command;
use crate::core::Core;
use crate::realm::change::Done;

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
        let by = Actor::Chief(self.realm.chief());
        self.unshelve_skill(by, skill, &reason.join(" "))
    }
}
