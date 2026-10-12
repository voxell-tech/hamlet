//! The system prompt of an agent: the charter, its machine, the
//! project its hamlet works on, its character, its notes and its
//! skills.

use std::path::{Path, PathBuf};

use crate::Home;

/// The machine's shell tools, as the prompt names them.
const SYSTEM: &str = if cfg!(target_os = "macos") {
    "System: macOS, whose shell tools are the BSD ones. Flags that \
     only GNU has, such as `grep -P`, `cat -A` and `sed -r`, fail."
} else if cfg!(windows) {
    "System: Windows."
} else {
    "System: Linux."
};

/// The most of `NOTES.md` put in the prompt.
const NOTES_SHOWN: usize = 8000;

/// The most of `CHARACTER.md` put in the prompt.
const CHARACTER_SHOWN: usize = 4000;

/// The files of a project that hold its own instructions for
/// agents. The first that exists is put in the prompt.
const INSTRUCTIONS: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// The folder an agent's hamlet works on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub folder: PathBuf,
    /// Each edit and each command waits for the chief.
    pub manual: bool,
}

impl Project {
    /// The part of the prompt that says where the agent works.
    fn prompt(&self, workspace: &Path) -> String {
        let (folder, workspace) =
            (self.folder.display(), workspace.display());
        let mut p = format!(
            "\n# The project\nYour hamlet works on the folder \
             {folder}. The work a chief asks for is done there. \
             This changes what is said above about where commands \
             run:\n- `bash` runs with {folder} as its working \
             directory. A command may write there and in your own \
             workspace, and nowhere else. It can read the rest of \
             the machine, so the project's own tools (cargo, npm, \
             make and the like) run as they do for the chief.\n- \
             `edit` takes a path relative to the project, or a full \
             path.\n- `read`, `search` and `find` look at files \
             without running a command. Use them rather than cat, \
             grep and find.\n- Your own workspace is {workspace}. \
             NOTES.md and skills/ are there and stay there: never \
             put notes, skills or files of your own in the project. \
             Give `edit` their full path, and run a skill from your \
             workspace, as shown below.\n- `.git/hooks` and \
             `.git/config` cannot be written.\n"
        );
        if self.folder.join(".git").exists() {
            p.push_str(
                "- The project is a git repository. When two agents \
                 must work apart, the mayor may have one of them \
                 use a git worktree inside the project folder, for \
                 example under `.hamlet/worktrees/<name>`: only the \
                 project folder and an agent's own workspace can be \
                 written.\n",
            );
        }
        if self.manual {
            p.push_str(
                "- The chief approves each edit and each command \
                 before it runs, so each one waits. `read`, \
                 `search` and `find` never wait: learn what you \
                 need with them, then make few, well-aimed edits \
                 and commands.\n",
            );
        }
        let own = INSTRUCTIONS.iter().find_map(|name| {
            let text =
                std::fs::read_to_string(self.folder.join(name))
                    .ok()?;
            Some((name, text))
        });
        if let Some((name, text)) = own {
            p.push_str(&format!(
                "\n# The project's {name}\n{}\n",
                shown(&text, name, NOTES_SHOWN)
            ));
        }
        p
    }
}

/// The first `most` characters of `text`, the file `name`, with a
/// note where it is cut.
fn shown(text: &str, name: &str, most: usize) -> String {
    let mut cut = text.chars().take(most).collect::<String>();
    if text.chars().count() > most {
        cut.push_str(&format!(
            "\n[{name} is longer than {most} characters and is cut \
             here.]"
        ));
    }
    cut
}

impl Home {
    /// The agent's character under its heading. Empty when it has
    /// none.
    fn character_says(&self) -> String {
        let text = std::fs::read_to_string(self.character())
            .unwrap_or_default();
        match text.trim() {
            "" => String::new(),
            text => format!(
                "\n# Your character\n{}\n",
                shown(text, "CHARACTER.md", CHARACTER_SHOWN)
            ),
        }
    }

    /// The home's `charter`, `environment`, the project the agent's
    /// hamlet works on, its character, the notes and the skill list.
    pub fn system_prompt(
        &self,
        charter: &str,
        environment: &str,
        project: Option<&Project>,
    ) -> String {
        let notes =
            std::fs::read_to_string(self.notes()).unwrap_or_default();
        let long = notes.chars().count() > NOTES_SHOWN;
        let mut notes =
            notes.chars().take(NOTES_SHOWN).collect::<String>();
        if long {
            notes.push_str(&format!(
                "\n[NOTES.md is longer than {NOTES_SHOWN} \
                 characters and is cut here. Shorten it so that all \
                 of it is shown.]"
            ));
        }
        let project_says = project
            .map(|project| project.prompt(&self.workspace))
            .unwrap_or_default();
        let mut p = format!(
            "{}\n\n# Environment\n{environment}\n{SYSTEM}\\
             nWorkspace: {}\n{project_says}{}\n# \
             NOTES.md\n{notes}\n\n# Skills\n",
            charter.trim_end(),
            self.workspace.display(),
            self.character_says()
        );
        let skills = self.skill_index();
        if skills.is_empty() {
            p.push_str("(none yet)\n");
        }
        for s in skills {
            let rel = s
                .path
                .strip_prefix(&self.workspace)
                .unwrap_or(&s.path);
            let run = match (&s.usage, project) {
                // Its command is written for the workspace, and
                // commands start in the project.
                (Some(usage), Some(_)) => format!(
                    " Run: `cd '{}' && {usage}`",
                    self.workspace.display()
                ),
                (Some(usage), None) => format!(" Run: `{usage}`"),
                (None, _) => String::new(),
            };
            p.push_str(&format!(
                "- {}: {}{run} ({})\n",
                s.name,
                s.description,
                rel.display()
            ));
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::charter;

    #[test]
    fn layout_and_prompt() {
        let dir = Scratch::new("home");
        let home = Home::open(dir.to_path_buf(), None).unwrap();
        assert!(home.notes().exists());
        let skill = home.skills().join("forecast");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: forecast\ndescription: \"Get the \
             forecast\"\nusage: sh \
             skills/forecast/scripts/get.sh\n---\nRun \
             scripts/get.sh\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.skills().join("broken"))
            .unwrap();
        std::fs::write(
            home.skills().join("broken/SKILL.md"),
            "no front matter",
        )
        .unwrap();
        std::fs::write(home.notes(), "# Notes\nUser likes tea.\n")
            .unwrap();

        let p =
            home.system_prompt(charter::DEFAULT, "test env", None);
        assert!(p.contains("User likes tea."));
        assert!(p.contains(
            "- forecast: Get the forecast Run: `sh \
             skills/forecast/scripts/get.sh` \
             (skills/forecast/SKILL.md)"
        ));
        assert!(!p.contains("broken"));
    }
    #[test]
    fn a_project_is_told_with_its_own_instructions() {
        let (dir, folder) =
            (Scratch::new("home-own"), Scratch::new("home-project"));
        let home = Home::open(dir.to_path_buf(), None).unwrap();
        let skill = home.skills().join("forecast");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: forecast\nusage: sh get.sh\n---\n",
        )
        .unwrap();
        let mut project = Project {
            folder: folder.to_path_buf(),
            manual: false,
        };
        let p = home.system_prompt("", "", Some(&project));
        assert!(p.contains(&format!(
            "works on the folder {}",
            folder.display()
        )));
        assert!(p.contains(&format!(
            "Run: `cd '{}' && sh get.sh`",
            home.workspace.display()
        )));
        assert!(!p.contains("git worktree"));
        assert!(!p.contains("The chief approves each edit"));
        assert!(!p.contains("# The project's"));

        std::fs::create_dir(folder.join(".git")).unwrap();
        std::fs::write(folder.join("CLAUDE.md"), "Use tabs.")
            .unwrap();
        project.manual = true;
        let p = home.system_prompt("", "", Some(&project));
        assert!(p.contains("git worktree"));
        assert!(p.contains("The chief approves each edit"));
        assert!(p.contains("# The project's CLAUDE.md\nUse tabs."));
        // AGENTS.md comes before CLAUDE.md, and is cut like the
        // notes.
        std::fs::write(
            folder.join("AGENTS.md"),
            "y".repeat(NOTES_SHOWN + 1),
        )
        .unwrap();
        let p = home.system_prompt("", "", Some(&project));
        assert!(p.contains("# The project's AGENTS.md\ny"));
        assert!(p.contains("[AGENTS.md is longer"));
        assert!(!p.contains("Use tabs."));
    }

    #[test]
    fn a_character_stands_between_the_charter_and_the_notes() {
        let dir = Scratch::new("home-character");
        let home = Home::open(dir.to_path_buf(), None).unwrap();
        let told =
            |home: &Home| home.system_prompt("Charter.", "", None);
        assert!(!told(&home).contains("# Your character"));
        std::fs::write(home.character(), " \n").unwrap();
        assert!(!told(&home).contains("# Your character"));
        std::fs::write(home.character(), "Speak plainly.\n").unwrap();
        let p = told(&home);
        let at = |words: &str| p.find(words).unwrap();
        assert!(
            at("Charter.") < at("# Your character\nSpeak plainly.")
        );
        assert!(at("Speak plainly.") < at("# NOTES.md"));
        std::fs::write(
            home.character(),
            "z".repeat(CHARACTER_SHOWN + 1),
        )
        .unwrap();
        assert!(told(&home).contains("[CHARACTER.md is longer"));
    }
}
