//! The commands a chief types about one hamlet: its record, its
//! agents, its roads, its skills and its folder.

use anyhow::{Result, anyhow};

use crate::commands::{self, Handler, OnHamlet};
use crate::hamlet::Hamlet;
use crate::lifecycle::Who;

/// A size in words.
fn bytes(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} bytes"),
        1024..=1_048_575 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

/// The outcome of a chief's command.
#[derive(Debug)]
pub struct Decree {
    /// The words to show the chief.
    pub lines: Vec<String>,
    /// Agents whose open sessions no longer match their houses.
    pub touched: Vec<String>,
}

/// A decree that says one line and leaves every session as it is.
fn said(line: String) -> Decree {
    Decree {
        lines: vec![line],
        touched: vec![],
    }
}

/// The reason a chief gave in `words`, or the one that stands for
/// none.
fn reason<'a>(words: impl Iterator<Item = &'a str>) -> String {
    match words.collect::<Vec<_>>().join(" ") {
        reason if reason.is_empty() => "a chief asked".to_string(),
        reason => reason,
    }
}

impl Hamlet {
    /// Runs `text` if it is a chief's command about this hamlet.
    /// `None` when it is not one.
    pub fn decree(
        &self,
        chief: &str,
        text: &str,
    ) -> Option<Result<Decree>> {
        let typed = commands::read(text)?;
        let Handler::Hamlet(command) = typed.command.by else {
            return None;
        };
        Some(self.carry_out(chief, command, typed.rest))
    }

    /// Runs the command `command` of `chief` about this hamlet.
    /// `rest` is what followed its name.
    pub fn carry_out(
        &self,
        chief: &str,
        command: OnHamlet,
        rest: &str,
    ) -> Result<Decree> {
        let mut words = rest.split_whitespace();
        let by = Who::Chief(chief);
        match command {
            OnHamlet::Record => {
                let mut lines = self.history();
                if lines.is_empty() {
                    lines.push("The record is empty.".into());
                }
                let archived = self.archived();
                if !archived.is_empty() {
                    lines.push(
                        "Archived, and restored with /restore \
                         <name>:"
                            .into(),
                    );
                }
                lines.extend(
                    archived.iter().map(|a| {
                        format!(
                            "  {} ({}) [archive {}]",
                            a.resident.name,
                            match &a.merge {
                                None => "removed".to_string(),
                                Some(m) => format!(
                                    "merged into {}",
                                    m.kept.name
                                ),
                            },
                            a.id
                        )
                    }),
                );
                Ok(Decree {
                    lines,
                    touched: vec![],
                })
            }
            OnHamlet::Restore => {
                let Some(which) = words.next() else {
                    return Err(anyhow!(
                        "usage: /restore <name> [as <new-name>] \
                         [reason]"
                    ));
                };
                let mut rest = words.collect::<Vec<_>>();
                let new_name = (rest.len() >= 2 && rest[0] == "as")
                    .then(|| rest.drain(..2).nth(1))
                    .flatten();
                self.restore(which, new_name, &rest.join(" "), by)
                    .map(|done| Decree {
                        lines: vec![done.report()],
                        touched: std::iter::once(
                            done.resident.name.clone(),
                        )
                        .chain(done.split_from.clone())
                        .collect(),
                    })
            }
            OnHamlet::Erase => {
                let Some(name) = words.next() else {
                    return Err(anyhow!("usage: /erase <name>"));
                };
                self.erase(name, by).map(|freed| Decree {
                    lines: vec![format!(
                        "Deleted `{name}` for good, and {} with it. \
                         It cannot be restored.",
                        bytes(freed)
                    )],
                    touched: vec![name.to_string()],
                })
            }
            OnHamlet::Remove => {
                let Some(name) = words.next() else {
                    return Err(anyhow!(
                        "usage: /remove <name> [reason]"
                    ));
                };
                let reason = reason(words);
                self.remove(name, &reason, by).map(|gone| Decree {
                    lines: vec![format!(
                        "Removed `{name}`. Its house is archived as \
                         {}; /restore {name} brings it back.",
                        gone.id
                    )],
                    touched: vec![name.to_string()],
                })
            }
            OnHamlet::Merge => {
                let (Some(absorb), Some("into"), Some(keep)) =
                    (words.next(), words.next(), words.next())
                else {
                    return Err(anyhow!(
                        "usage: /merge <absorbed> into <kept> \
                         [reason]"
                    ));
                };
                let reason = reason(words);
                // The kept agent goes on with its own purpose, and
                // the notes are joined for it to tidy.
                let purpose = self
                    .living(&self.census(), keep)
                    .map(|kept| kept.purpose);
                purpose
                    .and_then(|purpose| {
                        self.merge(
                            keep, absorb, &purpose, &reason, by, None,
                        )
                    })
                    .map(|done| Decree {
                        lines: vec![done.report()],
                        touched: vec![
                            keep.to_string(),
                            absorb.to_string(),
                        ],
                    })
            }
            OnHamlet::Road => {
                let (Some(a), Some(b)) = (words.next(), words.next())
                else {
                    return Err(anyhow!(
                        "usage: /road <agent> <agent> [reason]"
                    ));
                };
                let reason = reason(words);
                self.chief_road(chief, a, b, &reason).map(|()| {
                    Decree {
                        lines: vec![format!(
                            "Built a road between `{a}` and `{b}`. \
                             They can talk to each other now."
                        )],
                        touched: vec![],
                    }
                })
            }
            OnHamlet::Unroad => {
                let (Some(a), Some(b)) = (words.next(), words.next())
                else {
                    return Err(anyhow!(
                        "usage: /unroad <agent> <agent> [reason]"
                    ));
                };
                let reason = reason(words);
                self.remove_road(chief, a, b, &reason).map(|()| {
                    Decree {
                        lines: vec![format!(
                            "Took up the road between `{a}` and \
                             `{b}`. They reach each other through \
                             the mayor now."
                        )],
                        touched: vec![],
                    }
                })
            }
            OnHamlet::Give => {
                let (Some(to), Some(skill)) =
                    (words.next(), words.next())
                else {
                    return Err(anyhow!(
                        "usage: /give <agent> <skill> [from <agent>]"
                    ));
                };
                let from = match (words.next(), words.next()) {
                    (None, _) => None,
                    (Some("from"), Some(from)) => Some(from),
                    _ => {
                        return Err(anyhow!(
                            "usage: /give <agent> <skill> [from \
                             <agent>]"
                        ));
                    }
                };
                self.give_skill(chief, to, skill, from).map(|()| {
                    Decree {
                        lines: vec![format!(
                            "Gave `{to}` the skill `{skill}`."
                        )],
                        touched: vec![],
                    }
                })
            }
            OnHamlet::ResetBudget => {
                self.reset_budget(chief).map(|()| Decree {
                    lines: vec![
                        "The budget starts again from here. What \
                         was spent before is kept in the ledger."
                            .into(),
                    ],
                    touched: Vec::new(),
                })
            }
            OnHamlet::Folder => {
                self.folder_decree(chief, rest).map(said)
            }
            OnHamlet::Mode => self.mode_decree(chief, rest).map(said),
        }
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::MAYOR;

    fn hamlet(tag: &str) -> (Scratch, Hamlet) {
        let dir = Scratch::new(&format!("decree-{tag}"));
        let hamlet = Hamlet::new(dir.to_path_buf());
        hamlet
            .create("librarian", "Keeps the list.", "test")
            .unwrap();
        hamlet.create("scout", "Looks things up.", "test").unwrap();
        (dir, hamlet)
    }

    /// Writes the chief's limits.
    fn limits(hamlet: &Hamlet, text: &str) {
        std::fs::write(
            hamlet_home::limits::path(hamlet.root()),
            text,
        )
        .unwrap();
    }

    #[test]
    fn a_chief_builds_a_road_by_command_within_the_limits() {
        let (_dir, hamlet) = hamlet("road-decree");
        hamlet.create("poet", "Writes verse.", "test").unwrap();
        limits(&hamlet, "most_roads = 1\n[road]\ntokens = 400\n");
        let say = |text: &str| hamlet.decree("alice", text).unwrap();
        assert!(say("/road scout").is_err());
        assert!(say("/road scout scout").is_err());
        assert!(say("/road scout nobody").is_err());
        let to_mayor = say("/road scout mayor").unwrap_err();
        assert!(to_mayor.to_string().contains("already has a road"));

        let built =
            say("/road scout poet they share sources").unwrap();
        assert!(built.lines[0].contains("`scout` and `poet`"));
        assert!(built.touched.is_empty());
        assert!(hamlet.has_road("poet", "scout"));
        let line = hamlet
            .events()
            .into_iter()
            .rev()
            .find(|e| e["type"] == "road_built")
            .unwrap();
        assert_eq!(line["by"], "alice");
        assert_eq!(line["reason"], "they share sources");
        let told = hamlet.history();
        assert!(
            told.last().unwrap().contains(
                "the chief alice built a road between `scout` and \
                 `poet`: they share sources"
            ),
            "{told:?}"
        );
        // It is charged to the hamlet like the mayor's.
        assert_eq!(hamlet.report("own").hamlet.spent.tokens, 400);

        assert!(say("/road poet scout").is_err());
        let full = say("/road librarian poet").unwrap_err();
        assert!(
            full.to_string().contains("your limits allow (1)"),
            "{full}"
        );
        assert!(!hamlet.has_road("librarian", "poet"));
        limits(&hamlet, "most_roads = 2\n");
        say("/road librarian poet").unwrap();
        assert!(hamlet.has_road("librarian", "poet"));

        // A chief takes one up again, but never the mayor's.
        assert!(say("/unroad scout librarian").is_err());
        assert!(say("/unroad scout mayor").is_err());
        say("/unroad poet librarian no longer needed").unwrap();
        assert!(!hamlet.has_road("librarian", "poet"));
        assert!(hamlet.has_road("scout", "poet"));
        assert!(hamlet.has_road(MAYOR, "poet"));
        assert!(
            hamlet
                .history()
                .last()
                .unwrap()
                .contains("took up the road between `poet` and")
        );
        // The room it left can be built on again.
        say("/road librarian scout").unwrap();
    }
}
