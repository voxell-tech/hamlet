//! The folder a hamlet works on, and whether its agents ask the chief
//! before they change it, as the hamlet's record last says.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use hamlet_home::Project;
use hamlet_model::log;
use serde_json::json;

use crate::hamlet::Hamlet;

/// The way the agents of a hamlet change the folder it works on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Every edit and every command waits for the chief.
    #[default]
    Manual,
    /// Edits and commands run; the sandbox is the guard.
    Auto,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Manual => "manual",
            Mode::Auto => "auto",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        match name {
            "manual" => Some(Mode::Manual),
            "auto" => Some(Mode::Auto),
            _ => None,
        }
    }

    /// The mode in a sentence, as a chief reads it.
    pub fn says(self) -> &'static str {
        match self {
            Mode::Manual => {
                "Agents ask you before each edit and command."
            }
            Mode::Auto => {
                "Agents edit and run commands in the folder without \
                 asking."
            }
        }
    }
}

/// The work of a hamlet, as its record last says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub folder: Option<PathBuf>,
    /// Counts only while there is a folder.
    pub mode: Mode,
    /// The folders it worked on before, the latest first.
    pub before: Vec<PathBuf>,
}

impl Work {
    /// True when the agents ask before each edit and each command.
    pub fn manual(&self) -> bool {
        self.folder.is_some() && self.mode == Mode::Manual
    }

    pub fn project(&self) -> Option<Project> {
        Some(Project {
            folder: self.folder.clone()?,
            manual: self.mode == Mode::Manual,
        })
    }
}

/// `typed` as a path, with a leading `~` as the chief's own folder.
fn expand(typed: &str) -> PathBuf {
    let home = std::env::home_dir();
    match (typed.strip_prefix('~'), home) {
        (Some(""), Some(home)) => home,
        (Some(rest), Some(home)) if rest.starts_with(['/', '\\']) => {
            home.join(&rest[1..])
        }
        _ => PathBuf::from(typed),
    }
}

impl Hamlet {
    pub fn work(&self) -> Work {
        let mut work = Work::default();
        for e in self.events() {
            match e["type"].as_str() {
                Some("folder_set") => {
                    let next =
                        e["folder"].as_str().map(PathBuf::from);
                    work.before
                        .retain(|old| Some(old) != next.as_ref());
                    let last =
                        std::mem::replace(&mut work.folder, next);
                    if let Some(last) = last
                        && Some(&last) != work.folder.as_ref()
                    {
                        work.before.insert(0, last);
                    }
                }
                Some("mode_set") => {
                    if let Some(mode) =
                        e["mode"].as_str().and_then(Mode::named)
                    {
                        work.mode = mode;
                    }
                }
                _ => {}
            }
        }
        work
    }

    /// Points the hamlet at `folder`, or at none. Returns the folder
    /// as the record keeps it.
    pub fn work_on(
        &self,
        folder: Option<&Path>,
        by: &str,
    ) -> Result<Option<PathBuf>> {
        let folder = match folder {
            None => None,
            Some(folder) => {
                if !folder.is_absolute() {
                    bail!(
                        "give the folder's full path, such as \
                         /Users/you/code/project"
                    );
                }
                if !folder.is_dir() {
                    bail!("`{}` is not a folder", folder.display());
                }
                let real = folder.canonicalize()?;
                // Agents may write in the folder, and Hamlet's own
                // files must stay out of their reach.
                let home = self
                    .home()
                    .canonicalize()
                    .unwrap_or_else(|_| self.home().to_path_buf());
                if real.starts_with(&home) || home.starts_with(&real)
                {
                    bail!(
                        "a hamlet cannot work on Hamlet's own \
                         folder ({}), or on one that holds it",
                        home.display()
                    );
                }
                Some(real)
            }
        };
        self.append(json!({
            "v": 1,
            "type": "folder_set",
            "ts": log::now_millis(),
            "folder": folder,
            "by": by,
        }))?;
        Ok(folder)
    }

    pub fn set_mode(&self, mode: Mode, by: &str) -> Result<()> {
        self.append(json!({
            "v": 1,
            "type": "mode_set",
            "ts": log::now_millis(),
            "mode": mode.name(),
            "by": by,
        }))
    }

    /// Carries out `/folder <folder|none>`, with `rest` as what
    /// follows the command.
    pub(crate) fn folder_decree(
        &self,
        chief: &str,
        rest: &str,
    ) -> Result<String> {
        let work = self.work();
        match rest {
            "" => Ok(match &work.folder {
                Some(folder) => format!(
                    "This hamlet works on `{}`. {} `/folder none` \
                     stops that.",
                    folder.display(),
                    work.mode.says()
                ),
                None => "This hamlet works on no folder. `/folder \
                         <folder>` points it at one."
                    .to_string(),
            }),
            "none" => self.work_on(None, chief).map(|_| {
                "This hamlet works on no folder now. Its agents are \
                 back in their own workspaces."
                    .to_string()
            }),
            typed => self.work_on(Some(&expand(typed)), chief).map(
                |folder| {
                    format!(
                        "This hamlet works on `{}` now. {}",
                        folder.unwrap_or_default().display(),
                        work.mode.says()
                    )
                },
            ),
        }
    }

    /// Carries out `/mode <manual|auto>`, with `rest` as what follows
    /// the command.
    pub(crate) fn mode_decree(
        &self,
        chief: &str,
        rest: &str,
    ) -> Result<String> {
        let Some(mode) = Mode::named(rest) else {
            bail!("usage: /mode <manual|auto>");
        };
        self.set_mode(mode, chief)?;
        Ok(match self.work().folder {
            Some(_) => mode.says().to_string(),
            None => format!(
                "{} It counts once this hamlet works on a folder.",
                mode.says()
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn the_folder_and_the_mode_are_read_back_from_the_record() {
        let (home, project) =
            (Scratch::new("work-home"), Scratch::new("work-project"));
        let hamlet = Hamlet::new(home.to_path_buf());
        assert_eq!(hamlet.work(), Work::default());
        assert!(!hamlet.work().manual());

        let typed = format!("/folder {}", project.display());
        hamlet.decree("alice", &typed).unwrap().unwrap();
        let work = Hamlet::new(home.to_path_buf()).work();
        assert_eq!(work.folder.as_deref(), Some(&*project));
        // A hamlet pointed at a folder for the first time asks.
        assert!(work.manual());

        hamlet.decree("alice", "/mode auto").unwrap().unwrap();
        assert!(!hamlet.work().manual());
        // The mode is the hamlet's, whichever folder it works on.
        hamlet.decree("alice", "/folder none").unwrap().unwrap();
        assert_eq!(hamlet.work().folder, None);
        assert_eq!(hamlet.work().before, [project.to_path_buf()]);
        hamlet.decree("alice", &typed).unwrap().unwrap();
        assert_eq!(hamlet.work().mode, Mode::Auto);
        assert!(hamlet.work().before.is_empty());
        assert!(
            hamlet.decree("alice", "/mode fast").unwrap().is_err()
        );
    }

    #[test]
    fn a_hamlet_cannot_work_on_hamlets_own_folder() {
        let outer = Scratch::new("work-outer");
        let home = outer.join("home");
        std::fs::create_dir_all(home.join("agents")).unwrap();
        let hamlet = Hamlet::new(home.clone());
        for folder in
            [&home, &home.join("agents"), &outer.to_path_buf()]
        {
            assert!(
                hamlet.work_on(Some(folder), "alice").is_err(),
                "{}",
                folder.display()
            );
        }
        assert!(
            hamlet
                .work_on(Some(Path::new("relative")), "alice")
                .is_err()
        );
        assert_eq!(hamlet.work().folder, None);
    }
}
