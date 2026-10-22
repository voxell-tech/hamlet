//! The library: the skills every agent of the home shares, kept once
//! in `<home>/library/`, with `library.jsonl` as the record of what
//! was shelved, refused and taken out.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use hamlet_home::{Home, Skill, parse_skill, skills_in, starters};
use hamlet_model::log;
use serde_json::{Value, json};

use crate::files::{copy_tree, files_under};
use crate::record;

/// The librarian's name where its work is shown and written down. No
/// agent can have the name.
pub const LIBRARIAN: &str = "the librarian";

/// The most of a submitted skill's text the librarian is shown.
const SHOWN: usize = 6000;

/// The tool every agent has for offering a skill to the library.
pub(crate) fn submit_definition() -> Value {
    json!({"type": "function", "function": {
        "name": "submit_skill",
        "description": "Offer one of your own skills to the library, where every agent of every \
                        hamlet can use it. The librarian reads it and takes it or says why not. \
                        Once taken it is moved there and is no longer in your workspace: you use \
                        the library's, as everyone does. Submit what others would use again, not \
                        what only your own work needs.",
        "parameters": {"type": "object", "properties": {
            "skill": {"type": "string", "description": "Its folder under skills/ in your workspace."},
            "reason": {"type": "string", "description": "Who else would use it, and for what."}},
            "required": ["skill", "reason"]}}})
}

/// The tool every agent has for asking the librarian what the
/// library holds.
pub(crate) fn ask_definition() -> Value {
    json!({"type": "function", "function": {
        "name": "ask_librarian",
        "description": "Ask the librarian which skill of the library does what you need. It knows \
                        every skill on the shelves and answers with the one to use and the command \
                        that runs it, or tells you that nothing there does it. Ask before writing a \
                        skill of your own for something others may have solved.",
        "parameters": {"type": "object", "properties": {
            "question": {"type": "string", "description": "What you are trying to do, in a sentence or two."}},
            "required": ["question"]}}})
}

/// The shared skills of a home.
#[derive(Clone, Debug)]
pub struct Library {
    root: PathBuf,
    record: PathBuf,
    /// The house the librarian keeps its notes and its logs in.
    desk: PathBuf,
}

impl Library {
    /// The library of the home at `home`, as it stands.
    pub fn of(home: &Path) -> Self {
        Library {
            root: home.join("library"),
            record: home.join("library.jsonl"),
            desk: home.join("librarian"),
        }
    }

    /// The library of the home at `home`, made if it is not there
    /// and stocked with the starter skills it has not been offered.
    pub fn open(home: &Path) -> Result<Self> {
        let library = Library::of(home);
        std::fs::create_dir_all(&library.root)?;
        starters::install(&library.root)?;
        Ok(library)
    }

    /// The folder its skills stand in, one folder each.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every skill on its shelves, by name.
    pub fn index(&self) -> Vec<Skill> {
        skills_in(&self.root)
    }

    /// True when a skill is shelved as `name`.
    pub fn has(&self, name: &str) -> bool {
        self.root.join(name).join("SKILL.md").is_file()
    }

    /// An agent's brief on the library: its place, and each skill
    /// with the command that runs it from there. A skill's own text
    /// names its files as `skills/<name>/`, as it did in the house it
    /// was written in.
    pub fn prompt(&self) -> String {
        let skills = self.index();
        if skills.is_empty() {
            return String::new();
        }
        let at = self.root.display();
        let mut p = format!(
            "\n# Library\nSkills every agent shares, kept in {at}. \
             Use one as it stands: read it and run it from there, \
             and do not copy it into your workspace. Where its text \
             says `skills/<name>/`, its files are in \
             `{at}/<name>/`. You cannot write there. To share a \
             skill of your own, use `submit_skill`.\n"
        );
        p.push_str(&self.listing());
        p
    }

    /// Each skill on the shelves on a line: its name, what it
    /// does, and the command that runs it from where it stands.
    fn listing(&self) -> String {
        let at = self.root.display();
        let mut p = String::new();
        for skill in self.index() {
            let folder = skill
                .path
                .parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let there = format!("{at}/{folder}/");
            let run = match &skill.usage {
                Some(usage) => format!(
                    " Run: `{}`",
                    usage.replace(
                        &format!("skills/{folder}/"),
                        &there
                    )
                ),
                None => String::new(),
            };
            p.push_str(&format!(
                "- {}: {}{run} ({there}SKILL.md)\n",
                skill.name, skill.description
            ));
        }
        p
    }

    /// The librarian's own house: its notes, where it may keep what
    /// it has learned of judging skills, and the logs of what it
    /// was asked.
    pub fn desk(&self) -> Result<Home> {
        Home::open(self.desk.clone(), None)
    }

    /// The question to the librarian about the skill in the folder
    /// `from`, which `by` offers for `reason`.
    pub fn question(
        &self,
        from: &Path,
        by: &str,
        reason: &str,
    ) -> String {
        let text = std::fs::read_to_string(from.join("SKILL.md"))
            .unwrap_or_default();
        let shown = text.chars().take(SHOWN).collect::<String>();
        let files = files_under(from)
            .iter()
            .map(|file| file.display().to_string())
            .collect::<Vec<_>>();
        let shelved = self
            .index()
            .iter()
            .map(|skill| {
                format!("- {}: {}\n", skill.name, skill.description)
            })
            .collect::<String>();
        format!(
            "You are the librarian. You keep the library: the \
             skills every agent of every hamlet shares. `{by}` \
             offers a skill for it, giving this reason: {}\n\nTake \
             it only if it is all of these: useful to agents other \
             than the one that wrote it; not something a skill on \
             the shelves already does; clear enough that an agent \
             who has never seen it could run it from its text \
             alone; and free of anything private to one agent, such \
             as names, keys or paths of its own.\n\nOn the shelves \
             now:\n{shelved}\nThe skill offered, with its files \
             ({}):\n\n{shown}\n\nAnswer with ACCEPT or REFUSE as \
             your first word, then one or two sentences of why. If \
             you refuse, say what would change your answer.",
            reason.trim(),
            files.join(", "),
        )
    }

    /// The question to the librarian when `by` is looking for a skill
    /// that does `question`.
    pub fn inquiry(&self, question: &str, by: &str) -> String {
        let shelves = match self.listing() {
            told if told.is_empty() => "(nothing yet)\n".to_string(),
            told => told,
        };
        format!(
            "You are the librarian. You keep the library: the \
             skills every agent of every hamlet shares. `{by}` asks \
             you for a skill: {}\n\nThe shelves, each skill with \
             the command that runs it:\n{shelves}\nAnswer in a few \
             sentences. Name the skill or skills that do it and \
             give the exact command to run, as it is written above. \
             If nothing on the shelves does it, say so plainly, and \
             say that it may write a skill of its own and offer it \
             with `submit_skill` afterwards. Do not invent a skill \
             that is not on the shelves.",
            question.trim(),
        )
    }

    fn append(&self, event: Value) -> Result<()> {
        record::append(&self.record, &event)
    }

    /// The skills shelved, refused and taken out, oldest first.
    pub fn events(&self) -> Vec<Value> {
        record::read(&self.record)
    }

    /// Reason the skill in the folder `from` cannot be shelved as it
    /// is, before anyone is asked to judge it.
    pub fn unfit(&self, from: &Path) -> Option<String> {
        let name = from.file_name()?.to_string_lossy().into_owned();
        if parse_skill(&from.join("SKILL.md")).is_none() {
            return Some(format!(
                "`{name}` has no SKILL.md that opens with a name \
                 and a description"
            ));
        }
        self.has(&name).then(|| {
            format!(
                "the library already has a skill named `{name}`. \
                 Use that one, or submit yours under another name \
                 if it does something else"
            )
        })
    }

    /// Moves the skill in the folder `from` on to the shelves, where
    /// it is everyone's. `by` is who wrote it and `reason` is why
    /// the librarian took it.
    pub fn shelve(
        &self,
        from: &Path,
        by: &str,
        reason: &str,
    ) -> Result<()> {
        if let Some(why) = self.unfit(from) {
            bail!("{why}");
        }
        let name = from
            .file_name()
            .context("a skill is a folder")?
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&self.root)?;
        let to = self.root.join(&name);
        // A house and the library may lie on different disks.
        if std::fs::rename(from, &to).is_err() {
            copy_tree(from, &to)?;
            std::fs::remove_dir_all(from)?;
        }
        self.append(json!({
            "v": 1,
            "type": "skill_shelved",
            "ts": log::now_millis(),
            "skill": name,
            "by": by,
            "reason": reason.trim(),
        }))
    }

    /// Writes down that the skill `name` of `by` was not taken, and
    /// why.
    pub fn refuse(
        &self,
        name: &str,
        by: &str,
        reason: &str,
    ) -> Result<()> {
        self.append(json!({
            "v": 1,
            "type": "skill_refused",
            "ts": log::now_millis(),
            "skill": name,
            "by": by,
            "reason": reason.trim(),
        }))
    }

    /// Takes the skill `name` off the shelves for good.
    pub fn remove(
        &self,
        name: &str,
        by: &str,
        reason: &str,
    ) -> Result<()> {
        if !self.has(name) {
            bail!("the library has no skill named `{name}`");
        }
        std::fs::remove_dir_all(self.root.join(name))?;
        self.append(json!({
            "v": 1,
            "type": "skill_removed",
            "ts": log::now_millis(),
            "skill": name,
            "by": by,
            "reason": reason.trim(),
        }))
    }
}

/// True when the librarian's answer `said` takes the skill, with the
/// reason it gave. Anything but a plain yes is a no.
pub fn verdict(said: &str) -> (bool, String) {
    let said = said.trim();
    let first = said
        .split(|c: char| !c.is_ascii_alphabetic())
        .find(|word| !word.is_empty())
        .unwrap_or_default();
    let taken = first.eq_ignore_ascii_case("accept");
    let why = said
        .get(first.len()..)
        .filter(|_| said.starts_with(first))
        .unwrap_or(said)
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim();
    (taken, why.to_string())
}
