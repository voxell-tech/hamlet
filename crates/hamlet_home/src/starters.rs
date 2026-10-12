//! Starter skills, built into the binary and written into a folder of
//! skills once. A deleted one is not put back, and a file nobody
//! edited is brought up to date.

use std::path::Path;

use anyhow::Result;
use sha2::{Digest, Sha256};

macro_rules! starter {
    ($name:literal, [$($file:literal),+]) => {
        ($name, &[$(($file, include_str!(concat!("../starters/", $name, "/", $file)))),+])
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

/// SHA-256 of every starter file as an earlier version shipped it.
const SHIPPED: &[&str] = &[
    "058332dd952ac923df1e66ef40d86ddf09231f3ae9f0c375d438b3f54ccf5f10",
    "332b7d2c7c9dff2a539e363c1dd45ef8f0df4b0f5bec81a175776b88aaa60b05",
    "42bc7dcb52ec791f7add8d4c761beb61682dc6cbabc3f9cca5f10af0315b8760",
    "4a485c4b0a24f5897cd9a5dc201e1864e0db0bc9cf0fcc8108d912897e129a95",
    "7014db70c2c5c4e7668b32005e295221921654261eb310dbd4b957ebb21427e8",
    "7fdddf97dee72752a9ce63672afb8394e45d7c5c29a8290fe3972cfadb94b82c",
    "ad0193b00a5ec14f9676d395de721fff1458d12304c293980ab9dfe9dddb79fa",
    "e47ad9997acd2f1709c645539b58493cee503203dc6a91a72a75751311f87b14",
    "e7ff39d4d4bcfdc42e9972b78db7ccee19d8a0756aa5c63aa14a64750440eb31",
];

/// True when `text` is a starter file as an earlier version shipped
/// it, which nobody has edited since.
fn untouched(text: &str) -> bool {
    let hash = Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    SHIPPED.contains(&hash.as_str())
}

/// Names already offered to this workspace, one per line.
const RECORD: &str = ".starters";

/// Writes each starter this workspace has not been offered yet.
/// Returns the names written.
pub fn install(skills: &Path) -> Result<Vec<&'static str>> {
    let record = skills.join(RECORD);
    let mut offered =
        std::fs::read_to_string(&record).unwrap_or_default();
    let mut written = vec![];
    for (name, files) in STARTERS {
        let dir = skills.join(name);
        if offered.lines().any(|l| l == *name) {
            for (file, contents) in *files {
                let path = dir.join(file);
                let old = std::fs::read_to_string(&path);
                if old.is_ok_and(|old| untouched(&old)) {
                    std::fs::write(path, contents)?;
                }
            }
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

/// Writes the starter `name` as the folder `dir`. False when there
/// is no such starter.
pub fn write(name: &str, dir: &Path) -> Result<bool> {
    let Some((_, files)) = STARTERS.iter().find(|(n, _)| *n == name)
    else {
        return Ok(false);
    };
    put(files, dir)?;
    Ok(true)
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
        // A starter as an earlier version shipped it is brought up
        // to date, and an edited one is left.
        let system = skills.join("system-info/SKILL.md");
        let now = std::fs::read_to_string(&system).unwrap();
        let before = now
            .lines()
            .filter(|line| !line.starts_with("usage:"))
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        assert!(untouched(&before));
        std::fs::write(&system, before).unwrap();
        assert!(install(&skills).unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&system).unwrap(), now);
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
