//! An agent's home on disk: `workspace/` is all it can write,
//! `tools/` is read-only in the sandbox, and `sessions/` and
//! `CHARACTER.md` are never mounted into it.

pub mod charter;
pub mod instance;
pub mod limits;
mod prompt;
mod skill;
pub mod starters;

use std::path::PathBuf;

use anyhow::Result;
pub use prompt::Project;
pub use skill::{Skill, parse_skill, skills_in};

/// A home's folder and the workspace its agent writes in.
#[derive(Clone)]
pub struct Home {
    pub root: PathBuf,
    pub workspace: PathBuf,
}

impl Home {
    /// Creates the layout if missing.
    pub fn open(
        root: PathBuf,
        workspace: Option<PathBuf>,
    ) -> Result<Self> {
        let workspace =
            workspace.unwrap_or_else(|| root.join("workspace"));
        let home = Home { root, workspace };
        for dir in [home.skills(), home.tools(), home.sessions()] {
            std::fs::create_dir_all(dir)?;
        }
        if !home.notes().exists() {
            std::fs::write(home.notes(), "# Notes\n")?;
        }
        Ok(home)
    }

    pub fn notes(&self) -> PathBuf {
        self.workspace.join("NOTES.md")
    }

    /// The character the agent was given, as a file outside its
    /// workspace.
    pub fn character(&self) -> PathBuf {
        self.root.join("CHARACTER.md")
    }

    /// Writes `text` as the agent's character.
    pub fn set_character(&self, text: &str) -> std::io::Result<()> {
        std::fs::write(self.character(), format!("{}\n", text.trim()))
    }

    pub fn skills(&self) -> PathBuf {
        self.workspace.join("skills")
    }

    /// The installed plugins, one folder each.
    pub fn plugins(&self) -> PathBuf {
        self.root.join("plugins")
    }

    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    pub fn sessions(&self) -> PathBuf {
        self.root.join("sessions")
    }

    /// The agent's own skills, by name.
    pub fn skill_index(&self) -> Vec<Skill> {
        skills_in(&self.skills())
    }
}
