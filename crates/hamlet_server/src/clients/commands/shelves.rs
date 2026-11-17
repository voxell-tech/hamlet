//! A chief's commands on skills and dreams: giving a skill to a
//! scholar, taking one off the library's shelves, and having a hire
//! or a scholar dream.

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
        usage: "/dream <scholar#id|address>",
        does: "Have a hire bring its learning home, or a scholar \
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
            return Ok(Err(self.deeds.usage("unshelve")));
        };
        let by = Actor::Chief(self.realm.chief());
        self.unshelve_skill(by, skill, &reason.join(" "))
    }
}
