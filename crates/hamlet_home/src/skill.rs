//! A skill: a folder whose `SKILL.md` opens with a front matter that
//! names it.

use std::path::{Path, PathBuf};

/// A skill as its `SKILL.md` tells of it.
pub struct Skill {
    pub name: String,
    pub description: String,
    /// The command that runs it, from `usage` in its front matter.
    pub usage: Option<String>,
    pub path: PathBuf,
}

/// The skills that stand in `dir`, one folder each, by name.
pub fn skills_in(dir: &Path) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skills = entries
        .flatten()
        .filter_map(|e| parse_skill(&e.path().join("SKILL.md")))
        .collect::<Vec<_>>();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// The skill whose `SKILL.md` is at `path`. None when that is not
/// a skill's: it has to open with a front matter that names it.
pub fn parse_skill(path: &Path) -> Option<Skill> {
    let text = std::fs::read_to_string(path).ok()?;
    let front = text.strip_prefix("---")?.split("\n---").next()?;
    let field = |key: &str| {
        front.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim() == key)
                .then(|| v.trim().trim_matches('"').to_string())
        })
    };
    Some(Skill {
        name: field("name")?,
        description: field("description").unwrap_or_default(),
        usage: field("usage").filter(|usage| !usage.is_empty()),
        path: path.to_path_buf(),
    })
}
