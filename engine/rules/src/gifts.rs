//! A chief's gifts to an agent, copied into its workspace: a file for
//! its inbox, or a skill for its house.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use hamlet_home::starters;
use hamlet_model::log;
use serde_json::json;

use crate::files::{copy_tree, files_under};
use crate::hamlet::Hamlet;

/// The folder of a workspace that holds what a chief handed over.
pub const INBOX: &str = "inbox";

/// The most a chief hands over at once, in bytes.
const MOST: u64 = 32 * 1024 * 1024;

/// A skill an agent could be given, and whose it is. Nobody's for a
/// starter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub skill: String,
    pub from: Option<String>,
}

/// The words that go to an agent with what was put at `at` in its
/// workspace.
pub fn letter(at: &Path) -> String {
    format!(
        "I handed you `{}` in your workspace. Read it and do what \
         it asks. If it asks for nothing, tell me in a few lines \
         what it is.",
        at.display()
    )
}

/// A name in `dir` that nothing has yet, starting from `name`.
fn free(dir: &Path, name: &Path) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let stem = name.file_stem().unwrap_or_default().to_string_lossy();
    let ext = name
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| dir.join(format!("{stem}-{n}{ext}")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

/// The folders of skills in `skills`, by name.
fn folders(skills: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(skills)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

impl Hamlet {
    /// Copies the file or folder at `from` into the inbox of the
    /// agent `to`. Returns where it is, from the agent's workspace.
    pub fn hand(&self, to: &str, from: &Path) -> Result<PathBuf> {
        let Some(to) = self.resolve(to) else {
            bail!("no agent named `{to}`");
        };
        let Some(name) = from.file_name() else {
            bail!("{} has no name to keep", from.display());
        };
        let size = if from.is_dir() {
            files_under(from)
                .iter()
                .filter_map(|file| from.join(file).metadata().ok())
                .map(|meta| meta.len())
                .sum::<u64>()
        } else if from.is_file() {
            from.metadata()?.len()
        } else {
            bail!("{} cannot be read", from.display());
        };
        if size > MOST {
            bail!(
                "{} is larger than the {} MB an inbox takes at once",
                from.display(),
                MOST / (1024 * 1024)
            );
        }
        let home = self.house(&to.name)?;
        let inbox = home.workspace.join(INBOX);
        std::fs::create_dir_all(&inbox)?;
        let dest = free(&inbox, Path::new(name));
        if from.is_dir() {
            std::fs::create_dir_all(&dest)?;
            copy_tree(from, &dest)?;
        } else {
            std::fs::copy(from, &dest)?;
        }
        Ok(dest
            .strip_prefix(&home.workspace)
            .unwrap_or(&dest)
            .to_path_buf())
    }

    /// Copies the skill `skill` into the house of `to` at the word
    /// of `chief`: from the house of `from`, or from the starters.
    pub(crate) fn give_skill(
        &self,
        chief: &str,
        to: &str,
        skill: &str,
        from: Option<&str>,
    ) -> Result<()> {
        let named = !skill.is_empty()
            && skill.chars().all(|c| {
                c.is_ascii_alphanumeric() || c == '-' || c == '_'
            });
        if !named {
            bail!("a skill is named in letters, digits and hyphens");
        }
        let Some(to) = self.resolve(to) else {
            bail!("no agent named `{to}`");
        };
        let dest = self.house(&to.name)?.skills().join(skill);
        if dest.exists() {
            bail!(
                "`{}` already has a skill named `{skill}`",
                to.name
            );
        }
        match from {
            Some(from) => {
                let Some(giver) = self.resolve(from) else {
                    bail!("no agent named `{from}`");
                };
                let source =
                    self.house(&giver.name)?.skills().join(skill);
                if !source.join("SKILL.md").is_file() {
                    bail!("`{from}` has no skill named `{skill}`");
                }
                std::fs::create_dir_all(&dest)?;
                copy_tree(&source, &dest)?;
            }
            None => {
                if !starters::write(skill, &dest)? {
                    bail!(
                        "no starter skill named `{skill}`. Say \
                         whose it is: /give {} {skill} from <agent>",
                        to.name
                    );
                }
            }
        }
        self.append(json!({
            "v": 1,
            "type": "skill_given",
            "ts": log::now_millis(),
            "to": to.name,
            "skill": skill,
            "from": from.unwrap_or_default(),
            "by": chief,
        }))
    }

    /// The skills the agent `to` lacks and could be given: those of
    /// the other agents. The starters are in the library, where
    /// every agent has them already.
    pub fn offers(&self, to: &str) -> Vec<Offer> {
        let Some(to) = self.resolve(to) else {
            return Vec::new();
        };
        let skills_of = |name: &str| {
            self.house(name)
                .map(|home| folders(&home.skills()))
                .unwrap_or_default()
        };
        let has = skills_of(&to.name);
        let mut offers = Vec::<Offer>::new();
        let others = self
            .members()
            .into_iter()
            .filter(|member| member.id != to.id)
            .flat_map(|member| {
                skills_of(&member.name).into_iter().map(
                    move |skill| (skill, Some(member.name.clone())),
                )
            })
            .collect::<Vec<_>>();
        for (skill, from) in others {
            if !has.contains(&skill)
                && !offers.iter().any(|offer| offer.skill == skill)
            {
                offers.push(Offer { skill, from });
            }
        }
        offers
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    fn hamlet(tag: &str) -> (Scratch, Hamlet) {
        let dir = Scratch::new(&format!("gifts-{tag}"));
        let hamlet = Hamlet::new(dir.to_path_buf());
        hamlet
            .create("librarian", "Keeps the list.", "test")
            .unwrap();
        hamlet.create("scout", "Looks things up.", "test").unwrap();
        (dir, hamlet)
    }

    fn workspace(hamlet: &Hamlet, name: &str) -> PathBuf {
        hamlet.house(name).unwrap().workspace
    }

    #[test]
    fn a_file_handed_over_lands_in_the_inbox_and_keeps_what_was_there()
     {
        let (dir, hamlet) = hamlet("hand");
        let desk = dir.join("desk");
        std::fs::create_dir_all(desk.join("folder/deep")).unwrap();
        std::fs::write(desk.join("list.txt"), "Dune").unwrap();
        std::fs::write(desk.join("folder/deep/a.md"), "A").unwrap();

        let at =
            hamlet.hand("scout", &desk.join("list.txt")).unwrap();
        assert_eq!(at, Path::new("inbox/list.txt"));
        let inbox = workspace(&hamlet, "scout").join(INBOX);
        assert_eq!(
            std::fs::read_to_string(inbox.join("list.txt")).unwrap(),
            "Dune"
        );
        assert!(letter(&at).contains("`inbox/list.txt`"));

        // A second of the same name is kept beside the first.
        std::fs::write(desk.join("list.txt"), "Emma").unwrap();
        let again =
            hamlet.hand("scout", &desk.join("list.txt")).unwrap();
        assert_eq!(again, Path::new("inbox/list-2.txt"));
        assert_eq!(
            std::fs::read_to_string(inbox.join("list.txt")).unwrap(),
            "Dune"
        );

        // A folder goes whole, and the mayor has an inbox too.
        let at = hamlet.hand("mayor", &desk.join("folder")).unwrap();
        let hall = hamlet.hall().unwrap().workspace;
        assert!(hall.join(&at).join("deep/a.md").is_file());

        assert!(
            hamlet.hand("nobody", &desk.join("list.txt")).is_err()
        );
        assert!(hamlet.hand("scout", &desk.join("missing")).is_err());
    }
}
