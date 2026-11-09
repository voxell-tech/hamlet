//! Taking a path out of a repository's whole history for good.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::refs::transaction::PreviousValue;

use crate::{CommitId, copy_object};

/// Rewrites every commit of every branch and version of the
/// repository at `path` without `file`, a file or a folder, then
/// copies what the rewritten refs reach into a fresh repository and
/// puts it in the old one's place, so nothing of `file` remains.
/// Returns each branch's old and new tip. A purge cut off part way is
/// finished by the next call for the same path.
pub fn purge(
    path: &Path,
    file: &str,
) -> Result<Vec<(String, CommitId, CommitId)>> {
    settle(path)?;
    let old = gix::open_opts(path, gix::open::Options::isolated())
        .with_context(|| format!("opening {}", path.display()))?;
    let mut rewritten = HashMap::<ObjectId, ObjectId>::new();
    let mut branches = Vec::new();
    for reference in old.references()?.local_branches()? {
        let mut reference =
            reference.map_err(|e| anyhow::anyhow!("{e}"))?;
        let name = reference.name().as_bstr().to_str()?.to_owned();
        let tip = reference.peel_to_commit()?.id;
        let new = rewrite(&old, tip, file, &mut rewritten)?;
        branches.push((name, tip, new));
    }
    let mut versions = Vec::new();
    for reference in old.references()?.tags()? {
        let mut reference =
            reference.map_err(|e| anyhow::anyhow!("{e}"))?;
        let name = reference.name().shorten().to_str()?.to_owned();
        let target =
            reference.target().try_id().map(ToOwned::to_owned);
        let commit = reference.peel_to_commit()?.id;
        let new = rewrite(&old, commit, file, &mut rewritten)?;
        let tag = target
            .filter(|id| *id != commit)
            .map(|id| old.find_tag(id))
            .transpose()?
            .map(|tag| {
                tag.decode().and_then(gix::objs::TagRef::into_owned)
            })
            .transpose()?;
        versions.push((name, new, tag));
    }

    let fresh_path = sibling(path, "purging");
    if fresh_path.exists() {
        std::fs::remove_dir_all(&fresh_path)?;
    }
    let fresh: gix::Repository =
        gix::ThreadSafeRepository::init_opts(
            &fresh_path,
            gix::create::Kind::Bare,
            gix::create::Options::default(),
            gix::open::Options::isolated(),
        )?
        .into();
    for (name, _, new) in &branches {
        copy_object(&old, &fresh, *new)?;
        fresh.reference(
            name.as_str(),
            *new,
            PreviousValue::Any,
            "purged",
        )?;
    }
    for (name, new, tag) in versions {
        copy_object(&old, &fresh, new)?;
        match tag {
            Some(mut tag) => {
                tag.target = new;
                let id = fresh.write_object(&tag)?.detach();
                fresh.tag_reference(&name, id, PreviousValue::Any)?;
            }
            None => {
                fresh.tag_reference(
                    &name,
                    new,
                    PreviousValue::Any,
                )?;
            }
        }
    }
    drop(fresh);
    drop(old);
    // Marks the fresh repository whole: from here a cut-off purge is
    // finished by moving it into place.
    std::fs::write(fresh_path.join("PURGED"), b"")?;
    settle(path)?;
    Ok(branches
        .into_iter()
        .map(|(name, old, new)| {
            let name = name
                .strip_prefix("refs/heads/")
                .unwrap_or(&name)
                .to_owned();
            (name, CommitId(old), CommitId(new))
        })
        .collect())
}

/// Finishes a purge of `path` that was cut off: a whole fresh
/// repository takes the old one's place, and a partial one is
/// dropped.
fn settle(path: &Path) -> Result<()> {
    let fresh = sibling(path, "purging");
    let old = sibling(path, "purged");
    if fresh.join("PURGED").exists() {
        if path.exists() {
            std::fs::rename(path, &old)?;
        }
        std::fs::remove_file(fresh.join("PURGED"))?;
        std::fs::rename(&fresh, path)?;
    } else if fresh.exists() {
        std::fs::remove_dir_all(&fresh)?;
    }
    if old.exists() {
        std::fs::remove_dir_all(&old)?;
    }
    Ok(())
}

fn sibling(path: &Path, tag: &str) -> PathBuf {
    let mut name =
        path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{tag}"));
    path.with_file_name(name)
}

/// The commit `id` rewritten without `file`, with its parents
/// rewritten first.
fn rewrite(
    repo: &gix::Repository,
    id: ObjectId,
    file: &str,
    done: &mut HashMap<ObjectId, ObjectId>,
) -> Result<ObjectId> {
    // Parents before children, without recursion: a history can be
    // long.
    let mut stack = vec![(id, false)];
    while let Some((commit, ready)) = stack.pop() {
        if done.contains_key(&commit) {
            continue;
        }
        let found = repo.find_commit(commit)?;
        let mut owned = found.decode()?.into_owned()?;
        if !ready {
            stack.push((commit, true));
            for parent in &owned.parents {
                if !done.contains_key(parent) {
                    stack.push((*parent, false));
                }
            }
            continue;
        }
        let mut editor = repo.edit_tree(owned.tree)?;
        editor.remove(file)?;
        let tree = editor.write()?.detach();
        let parents = owned
            .parents
            .iter()
            .map(|parent| done[parent])
            .collect::<Vec<_>>();
        let unchanged =
            tree == owned.tree && parents[..] == owned.parents[..];
        let new = if unchanged {
            commit
        } else {
            owned.tree = tree;
            owned.parents = parents.into_iter().collect();
            repo.write_object(&owned)?.detach()
        };
        done.insert(commit, new);
    }
    Ok(done[&id])
}
