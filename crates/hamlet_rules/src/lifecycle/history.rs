//! The record in words, for a chief to read (H17).

use crate::hamlet::{Hamlet, by_whom};
use crate::record::when;

impl Hamlet {
    /// The record of who was created, removed, merged and restored
    /// and which roads were built, one line each with its reason,
    /// oldest first (H17).
    pub fn history(&self) -> Vec<String> {
        self.events()
            .iter()
            .filter_map(|e| {
                let text =
                    |key: &str| e[key].as_str().unwrap_or_default();
                let at = when(e["ts"].as_u64().unwrap_or_default());
                let by = by_whom(text("by"));
                Some(match e["type"].as_str()? {
                    "agent_created" => format!(
                        "{at}  the mayor created `{}` ({}): {}",
                        text("name"),
                        text("purpose"),
                        text("reason")
                    ),
                    "road_built" => format!(
                        "{at}  {by} built a road between `{}` and \
                         `{}`: {}",
                        text("a"),
                        text("b"),
                        text("reason")
                    ),
                    "road_removed" => format!(
                        "{at}  {by} took up the road between `{}` \
                         and `{}`: {}",
                        text("a"),
                        text("b"),
                        text("reason")
                    ),
                    "skill_given" => format!(
                        "{at}  {by} gave `{}` the skill `{}`{}",
                        text("to"),
                        text("skill"),
                        match text("from") {
                            "" => String::new(),
                            from => format!(" of `{from}`"),
                        }
                    ),
                    "agent_removed" => format!(
                        "{at}  {by} removed `{}`: {} [archive {}]",
                        text("name"),
                        text("reason"),
                        text("archive")
                    ),
                    "agents_merged" => format!(
                        "{at}  {by} merged `{}` into `{}`, which is \
                         now for: {} Reason: {} [archive {}, notes \
                         {}]",
                        text("gone"),
                        text("kept"),
                        text("purpose"),
                        text("reason"),
                        text("archive"),
                        text("notes")
                    ),
                    "agent_erased" => format!(
                        "{at}  {by} deleted `{}` for good [archive \
                         {}]",
                        text("name"),
                        text("archive")
                    ),
                    "agent_restored" => format!(
                        "{at}  {by} restored `{}` from archive {}: \
                         {}",
                        text("name"),
                        text("archive"),
                        text("reason")
                    ),
                    "mayor_changed" => format!(
                        "{at}  `{}` became mayor in place of `{}` \
                         ({}): {}",
                        text("to"),
                        text("from"),
                        text("by"),
                        text("reason")
                    ),
                    _ => return None,
                })
            })
            .collect()
    }
}
