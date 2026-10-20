//! Shelves of skills: an agent's own and the library every agent
//! shares (C8, H33).

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// A skill as the front matter of its `SKILL.md` tells of it.
pub(crate) struct Skill {
    pub name: String,
    pub description: String,
    usage: Option<String>,
    pub folder: String,
}

/// The skills of the house at `house` on this machine, none where it
/// is not a folder.
pub(crate) fn house_skills(house: &Path) -> Vec<Skill> {
    if real_dir(house) {
        shelf(&house.join("skills"))
    } else {
        Vec::new()
    }
}

pub(super) fn skills_text(house: &Path, found: &[Skill]) -> String {
    let quoted = house.display().to_string().replace('\'', "'\\''");
    let lines = found
        .iter()
        .map(|s| {
            let run = s
                .usage
                .as_ref()
                .map(|u| format!(" Run: `cd '{quoted}' && {u}`"))
                .unwrap_or_default();
            format!(
                "- {}: {}{run} (skills/{}/SKILL.md)",
                s.name, s.description, s.folder
            )
        })
        .collect::<Vec<_>>();
    format!(
        "# Skills\n{}",
        if lines.is_empty() {
            "(none yet)".into()
        } else {
            lines.join("\n")
        }
    )
}

/// The folders of the house's skills.
pub(super) fn names(house: &Path) -> Vec<String> {
    house_skills(house)
        .into_iter()
        .map(|skill| skill.folder)
        .collect()
}

/// The library's part where it is on this machine.
pub(crate) fn library(root: &Path) -> Option<String> {
    library_at(root, root)
}

/// The library's part: where it is, shown as `shown`, and each skill
/// with the command that runs it from there. None while it has no
/// skills.
pub(super) fn library_at(
    root: &Path,
    shown: &Path,
) -> Option<String> {
    let skills = if real_dir(root) {
        shelf(root)
    } else {
        Vec::new()
    };
    if skills.is_empty() {
        return None;
    }
    let root = shown.display();
    let mut lines = vec![format!(
        "# Library\nSkills every agent shares, kept in {root}. Use \
         one as it stands: read it and run it from there, and do \
         not copy it into your workspace. Where its text says \
         `skills/<name>/`, its files are in `{root}/<name>/`. You \
         cannot write there. To share a skill of your own, use \
         `submit_skill`."
    )];
    for skill in skills {
        let there = format!("{root}/{}/", skill.folder);
        let run = skill
            .usage
            .map(|u| {
                let own = format!("skills/{}/", skill.folder);
                format!(" Run: `{}`", u.replace(&own, &there))
            })
            .unwrap_or_default();
        lines.push(format!(
            "- {}: {}{run} ({there}SKILL.md)",
            skill.name, skill.description
        ));
    }
    Some(lines.join("\n"))
}

fn real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// The skills of `dir`, one folder each whose `SKILL.md` is a
/// regular file, in the folders' order.
pub(crate) fn shelf(dir: &Path) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let folder = e.file_name().into_string().ok()?;
            let path = e.path().join("SKILL.md");
            std::fs::symlink_metadata(&path)
                .is_ok_and(|m| m.is_file())
                .then(|| {
                    skill(folder, &paper(&path).unwrap_or_default())
                })
        })
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.folder.cmp(&b.folder));
    found
}

/// The skill a `SKILL.md` of `text` tells of: its front matter runs
/// to the first line opening with `---`, and names it, or the folder
/// does.
pub(crate) fn skill(folder: String, text: &str) -> Skill {
    let front = text
        .strip_prefix("---")
        .map(|t| t.split("\n---").next().unwrap_or_default());
    let field = |key: &str| {
        front?.split('\n').find_map(|line| {
            let (k, v) = line.split_once(':')?;
            (k.trim() == key)
                .then(|| v.trim().trim_matches('"').to_owned())
        })
    };
    match field("name") {
        Some(name) => Skill {
            name,
            description: field("description").unwrap_or_default(),
            usage: field("usage").filter(|u| !u.is_empty()),
            folder,
        },
        None => Skill {
            name: folder.clone(),
            description: String::new(),
            usage: None,
            folder,
        },
    }
}

/// The first 64 KiB of a regular file, opened without following a
/// final link.
fn paper(path: &Path) -> std::io::Result<String> {
    let file = open(path)?;
    if !file.metadata()?.is_file() {
        return Ok(String::new());
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(unix)]
fn open(path: &Path) -> std::io::Result<File> {
    use rustix::fs::{Mode, OFlags};
    Ok(rustix::fs::open(
        path,
        OFlags::RDONLY
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

#[cfg(not(unix))]
fn open(path: &Path) -> std::io::Result<File> {
    File::open(path)
}
