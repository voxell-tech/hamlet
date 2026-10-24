//! Starter skills, built into the binary and written into a folder of
//! skills once. A deleted or edited one is left as it is.

use std::path::Path;

use anyhow::{Context, Result};

macro_rules! starter {
    ($name:literal, [$($file:literal),+]) => {
        ($name, &[$(($file, include_str!(concat!("../assets/starters/", $name, "/", $file)))),+])
    };
}

type Starter =
    (&'static str, &'static [(&'static str, &'static str)]);

const STARTERS: &[Starter] = &[
    starter!("web-search", ["SKILL.md", "scripts/search.py"]),
    starter!("read-webpage", ["SKILL.md", "scripts/read.py"]),
    starter!("weather", ["SKILL.md", "scripts/weather.py"]),
    starter!("call-api", ["SKILL.md", "scripts/api.py"]),
    starter!("system-info", ["SKILL.md", "scripts/info.py"]),
    starter!("write-skill", ["SKILL.md"]),
    starter!("manage-notes", ["SKILL.md"]),
    starter!("write-typst", ["SKILL.md"]),
];

/// Names already offered to this workspace, one per line.
const RECORD: &str = ".starters";

/// Writes each starter this workspace has not been offered yet.
/// Returns the names written.
pub fn install(skills: &Path) -> Result<Vec<&'static str>> {
    let record = skills.join(RECORD);
    let mut offered = match std::fs::read_to_string(&record) {
        Ok(offered) => offered,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            String::new()
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!("reading {}", record.display())
            });
        }
    };
    let mut written = vec![];
    for (name, files) in STARTERS {
        let dir = skills.join(name);
        if offered.lines().any(|l| l == *name) {
            continue;
        }
        // A skill of the same name that is already there wins.
        if !dir.exists() {
            put(files, &dir)?;
            written.push(*name);
        }
        offered.push_str(name);
        offered.push('\n');
    }
    std::fs::write(record, offered)?;
    Ok(written)
}

/// Writes the files of one starter under `dir`.
fn put(files: &[(&str, &str)], dir: &Path) -> Result<()> {
    for (file, contents) in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap_or(dir))?;
        std::fs::write(path, contents)?;
    }
    Ok(())
}

/// The names of the starters.
pub fn names() -> impl Iterator<Item = &'static str> {
    STARTERS.iter().map(|(name, _)| *name)
}

/// The relative paths and bytes shipped for one starter.
pub fn files(
    name: &str,
) -> Option<&'static [(&'static str, &'static str)]> {
    STARTERS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, files)| *files)
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn installs_once_and_respects_edits_and_deletions() {
        let skills = Scratch::new("starters");
        std::fs::create_dir_all(skills.join("weather")).unwrap();
        std::fs::write(skills.join("weather/SKILL.md"), "mine")
            .unwrap();

        let written = install(&skills).unwrap();
        assert_eq!(written.len(), STARTERS.len() - 1);
        assert_eq!(
            std::fs::read_to_string(skills.join("weather/SKILL.md"))
                .unwrap(),
            "mine"
        );
        assert!(skills.join("web-search/scripts/search.py").exists());

        std::fs::remove_dir_all(skills.join("web-search")).unwrap();
        std::fs::write(skills.join("call-api/SKILL.md"), "edited")
            .unwrap();
        assert!(install(&skills).unwrap().is_empty());
        assert!(!skills.join("web-search").exists());
        assert_eq!(
            std::fs::read_to_string(skills.join("call-api/SKILL.md"))
                .unwrap(),
            "edited"
        );
    }

    #[test]
    fn every_starter_has_valid_front_matter() {
        for (name, files) in STARTERS {
            let skill = files
                .iter()
                .find(|(f, _)| *f == "SKILL.md")
                .expect("has SKILL.md")
                .1;
            assert!(skill.starts_with("---\n"), "{name}");
            assert!(
                skill.contains(&format!("name: {name}\n")),
                "{name}"
            );
            assert!(skill.contains("description: "), "{name}");
        }
    }
}
