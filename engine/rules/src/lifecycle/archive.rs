//! The houses of agents that left, under `archive/<id>/house`, moved
//! to where the record says they are.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use hamlet_model::log;
use serde_json::{Value, json};

use crate::files::{
    copy_tree, digest, files_under, folders, same_tree,
};
use crate::hamlet::{Archived, Census, Hamlet};
use crate::record::when;

impl Hamlet {
    pub(crate) fn shelf(&self, id: &str) -> PathBuf {
        self.root().join("archive").join(id)
    }

    /// A new, empty folder under `archive/` for `name`.
    pub(super) fn new_shelf(
        &self,
        name: &str,
    ) -> Result<(String, PathBuf)> {
        std::fs::create_dir_all(self.root().join("archive"))?;
        let stamp = log::now_millis();
        let mut id = format!("{name}-{stamp}");
        let mut n = 1;
        // Making the folder is what claims the id, so two sessions
        // deciding in the same moment get different ones.
        loop {
            let dir = self.shelf(&id);
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok((id, dir)),
                Err(e)
                    if e.kind()
                        == std::io::ErrorKind::AlreadyExists =>
                {
                    n += 1;
                    id = format!("{name}-{stamp}-{n}");
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Moves a house standing at `name` that the record gives to
    /// nobody into the archive.
    pub(crate) fn set_aside(
        &self,
        census: &Census,
        name: &str,
    ) -> Result<()> {
        let home = self.house_in(census, name);
        if home.exists() {
            let (_, dir) = self.new_shelf(name)?;
            std::fs::rename(&home, dir.join("house"))?;
        }
        Ok(())
    }

    /// Brings the houses in step with the record, after a crash
    /// between a line written and its files moved.
    pub fn repair(&self) {
        self.mend();
    }

    pub(crate) fn mend(&self) {
        let census = self.census();
        for a in &census.archive {
            let dir = self.shelf(&a.id);
            let moved = match &a.back {
                None => self.shelve(&census, a, &dir),
                Some(name) => self.unshelve(&census, a, name, &dir),
            };
            if let Err(e) = moved {
                eprintln!(
                    "hamlet: archive {} does not match the record \
                     yet: {e:#}",
                    a.id
                );
            }
        }
    }

    fn shelve(
        &self,
        census: &Census,
        a: &Archived,
        dir: &Path,
    ) -> Result<()> {
        let name = &a.resident.name;
        let (house, home) =
            (dir.join("house"), self.house_in(census, name));
        let reused = census.residents.iter().any(|r| r.name == *name);
        if !house.exists() && home.exists() && !reused {
            std::fs::create_dir_all(dir)?;
            let moved = std::fs::rename(&home, &house);
            // Another session may have moved it first.
            if home.exists() {
                moved.with_context(|| {
                    format!("moving the house of `{name}`")
                })?;
            }
        }
        let Some(merge) = &a.merge else {
            return Ok(());
        };
        if dir.join("applied").exists() || !house.exists() {
            return Ok(());
        }
        match census.residents.iter().find(|r| r.id == merge.kept.id)
        {
            Some(kept) => self.bring(census, &kept.name, name, dir),
            None => Ok(()),
        }
    }

    fn unshelve(
        &self,
        census: &Census,
        a: &Archived,
        name: &str,
        dir: &Path,
    ) -> Result<()> {
        let (house, home) =
            (dir.join("house"), self.house_in(census, name));
        if !house.exists() || home.exists() {
            return Ok(());
        }
        if let Some(merge) = &a.merge
            && !dir.join("parted").exists()
        {
            if let Some(kept) = census
                .residents
                .iter()
                .find(|r| r.id == merge.kept.id)
            {
                self.part(census, &kept.name, &a.resident.name, dir)?;
            }
            std::fs::write(dir.join("parted"), "")?;
        }
        if let Some(parent) = home.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let moved = std::fs::rename(&house, &home);
        // Another session may have moved it first.
        if house.exists() {
            moved.with_context(|| {
                format!("moving the house of `{name}`")
            })?;
        }
        Ok(())
    }

    /// Copies what the archived house in `dir` held into the house
    /// of `kept` and installs the merged notes. Nothing of `kept`'s
    /// is overwritten but its notes.
    fn bring(
        &self,
        census: &Census,
        kept: &str,
        gone: &str,
        dir: &Path,
    ) -> Result<()> {
        let from = dir.join("house");
        let to = self.house_in(census, kept);
        let (from_ws, to_ws) =
            (from.join("workspace"), to.join("workspace"));
        let aside = Path::new("merged").join(gone);
        let mut brought = Vec::<Value>::new();
        let mut clashes = Vec::<String>::new();
        // Copies one file in. `renamed` is for a file of a skill
        // that goes in under another name.
        let mut put = |src: &Path,
                       rel: &Path,
                       renamed: Option<(&str, &str)>|
         -> Result<()> {
            let dst = to_ws.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(src, &dst)?;
            if let Some((name, renamed)) = renamed
                && let Ok(text) = std::fs::read_to_string(&dst)
            {
                std::fs::write(
                    &dst,
                    renamed_skill(&text, name, renamed),
                )?;
            }
            brought.push(json!({"path": rel, "sha": digest(&dst)}));
            Ok(())
        };

        // Both notes as they were, where the merged agent can read
        // them.
        put(
            &dir.join("NOTES.before.md"),
            &aside.join(format!("NOTES.{kept}.md")),
            None,
        )?;
        if from_ws.join("NOTES.md").exists() {
            put(
                &from_ws.join("NOTES.md"),
                &aside.join("NOTES.md"),
                None,
            )?;
        }

        for skill in folders(&from_ws.join("skills")) {
            let Some(name) =
                skill.file_name().and_then(|n| n.to_str())
            else {
                continue;
            };
            let rel = Path::new("skills").join(name);
            let target = to_ws.join(&rel);
            if !target.exists() {
                for file in files_under(&skill) {
                    put(&skill.join(&file), &rel.join(&file), None)?;
                }
            } else if !same_tree(&skill, &target) {
                // A skill of the same name that differs is kept
                // beside it under another name.
                let renamed = format!("{name}-from-{gone}");
                let rel = Path::new("skills").join(&renamed);
                for file in files_under(&skill) {
                    put(
                        &skill.join(&file),
                        &rel.join(&file),
                        Some((name, &renamed)),
                    )?;
                }
                clashes.push(format!(
                    "skill `{name}` (the absorbed one is \
                     `{renamed}`)"
                ));
            }
        }

        for file in files_under(&from_ws) {
            if file == Path::new("NOTES.md")
                || file.starts_with("skills")
            {
                continue;
            }
            let (src, dst) = (from_ws.join(&file), to_ws.join(&file));
            if !dst.exists() {
                put(&src, &file, None)?;
            } else if digest(&src) != digest(&dst) {
                put(&src, &aside.join(&file), None)?;
                clashes.push(format!(
                    "file `{}` (the absorbed one is `{}`)",
                    file.display(),
                    aside.join(&file).display()
                ));
            }
        }

        let mut tools = Vec::<String>::new();
        for tool in folders(&from.join("tools")) {
            let Some(name) =
                tool.file_name().and_then(|n| n.to_str())
            else {
                continue;
            };
            let target = to.join("tools").join(name);
            if !target.exists() {
                copy_tree(&tool, &target)?;
                tools.push(name.to_string());
            } else if !same_tree(&tool, &target) {
                clashes.push(format!(
                    "tool `{name}` (the absorbed one stays in the \
                     archive)"
                ));
            }
        }

        std::fs::copy(dir.join("NOTES.md"), to_ws.join("NOTES.md"))?;
        std::fs::write(
            dir.join("manifest.json"),
            json!({"brought": brought, "tools": tools, "clashes": clashes})
                .to_string(),
        )?;
        std::fs::write(dir.join("applied"), "")?;
        Ok(())
    }

    /// Takes back out of `kept`'s house what the merge in `dir`
    /// brought, where `kept` has not changed it since.
    fn part(
        &self,
        census: &Census,
        kept: &str,
        gone: &str,
        dir: &Path,
    ) -> Result<()> {
        let to = self.house_in(census, kept);
        let to_ws = to.join("workspace");
        let notes = to_ws.join("NOTES.md");
        let read =
            |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
        // Notes untouched since the merge go back to what they
        // were. Notes written since are kept, with a line added.
        let clean = read(&notes) == read(&dir.join("NOTES.md"));
        let said = format!("`{gone}` was split off again");
        if clean {
            std::fs::copy(dir.join("NOTES.before.md"), &notes)?;
        } else if !read(&notes).contains(&said) {
            let mut text = read(&notes).trim_end().to_string();
            text.push_str(&format!(
                "\n\n## Split\n- {}: {said} and has its own house. \
                 What these notes hold of its work is its business \
                 now. My notes from before the merge are in \
                 merged/{gone}/NOTES.{kept}.md.\n",
                when(log::now_millis())
            ));
            std::fs::write(&notes, text)?;
        }
        let manifest = serde_json::from_str::<Value>(&read(
            &dir.join("manifest.json"),
        ))
        .unwrap_or_default();
        let aside = Path::new("merged").join(gone);
        for item in
            manifest["brought"].as_array().into_iter().flatten()
        {
            let Some(rel) = item["path"].as_str().map(Path::new)
            else {
                continue;
            };
            let at = to_ws.join(rel);
            let kept_notes = !clean
                && rel.starts_with(&aside)
                && rel.parent() == Some(&aside);
            if kept_notes
                || digest(&at)
                    != item["sha"].as_str().map(String::from)
            {
                continue;
            }
            std::fs::remove_file(&at)?;
            // Folders the merge made go when they are empty.
            let mut folder = at.parent();
            while let Some(f) = folder.filter(|f| *f != to_ws) {
                if std::fs::remove_dir(f).is_err() {
                    break;
                }
                folder = f.parent();
            }
        }
        for name in manifest["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let (mine, theirs) = (
                to.join("tools").join(name),
                dir.join("house/tools").join(name),
            );
            if same_tree(&mine, &theirs) {
                std::fs::remove_dir_all(&mine)?;
            }
        }
        Ok(())
    }
}

/// A skill's file with the skill renamed: its `name:` line and the
/// paths that go through its folder.
fn renamed_skill(text: &str, name: &str, renamed: &str) -> String {
    let mut named = false;
    text.replace(
        &format!("skills/{name}/"),
        &format!("skills/{renamed}/"),
    )
    .split_inclusive('\n')
    .map(|line| {
        let is_name = !named
            && line.split_once(':').is_some_and(|(k, v)| {
                k.trim() == "name"
                    && v.trim().trim_matches('"') == name
            });
        if is_name {
            named = true;
            format!("name: {renamed}\n")
        } else {
            line.to_string()
        }
    })
    .collect()
}
