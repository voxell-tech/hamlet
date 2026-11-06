//! The repositories' side of merging two hires and splitting them
//! again (H5).

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use hamlet_git::{
    Author, Change, CommitId, FileChange, Joined, MAIN, Repository,
};
use hamlet_store::Db;
use hamlet_store::ids::{Actor, MergeId};

use super::folder::Folder;
use super::run::{check_out, commit};
use super::{CHARACTER, branch, lodging};
use crate::layout::Layout;
use crate::society::{hires, merges};

/// The mark of a merge that only moved the kept branch forward, kept
/// as `moved:<from>..<to>`.
const MOVED: &str = "moved:";

/// Brings what the absorbed hire of merge `id` learned into the kept
/// hire's branch and lodge, once.
pub(super) fn merge(
    db: &mut Db,
    layout: &Layout,
    id: MergeId,
    at: i64,
) -> Result<()> {
    let found = merges::get(db, id)?;
    if found.merge_commit.is_some() {
        return Ok(());
    }
    commit(db, layout, found.absorbed, at)?;
    commit(db, layout, found.kept, at)?;
    let kept = lodging(db, layout, found.kept)?;
    let absorbed = lodging(db, layout, found.absorbed)?;
    let absorbed_hire = hires::get(db, found.absorbed)?;
    let from =
        format!("{}-{}", absorbed_hire.hamlet, absorbed_hire.number);
    let name = Actor::Realm.to_string();
    let author = Author { name: &name, at };
    let message =
        format!("Took in what {from} learned: {}", found.reason);
    let repo = Repository::open(&layout.repository(kept.scholar))?;
    let kept_branch = branch(found.kept);
    let ours = repo
        .tip(&kept_branch)?
        .context("the kept hire has no branch")?;
    let (mark, tip, clashes) = if kept.scholar == absorbed.scholar {
        match repo.tip(&branch(found.absorbed))? {
            Some(theirs) => {
                let merged = repo.merge(
                    &kept_branch,
                    theirs,
                    &message,
                    author,
                )?;
                let mark = match merged.joined {
                    Joined::FastForward => {
                        format!("{MOVED}{ours}..{}", merged.commit)
                    }
                    _ => merged.commit.to_string(),
                };
                (mark, merged.commit, merged.clashes)
            }
            None => (ours.to_string(), ours, Vec::new()),
        }
    } else {
        let theirs =
            Repository::open(&layout.repository(absorbed.scholar))?;
        let (changes, clashes) = learned(
            &repo,
            ours,
            &theirs,
            &branch(found.absorbed),
            &from,
        )?;
        let tip =
            repo.commit(&kept_branch, &changes, &message, author)?;
        (tip.to_string(), tip, clashes)
    };
    merges::merged(db, id, &mark, &clashes)?;
    check_out(&repo, tip, &Folder::make(&kept.path)?)?;
    move_inbox(&absorbed.path, &kept.path, &from)
}

/// The changes that bring what the hire on `theirs_branch` of
/// `theirs` learned since it last took in its scholar's `main` onto
/// the kept tip `ours` of `repo`, and the paths that clashed. A file
/// both changed is put aside as `<name>.from-<from>`, a skill as
/// `<skill>-from-<from>`.
fn learned(
    repo: &Repository,
    ours: CommitId,
    theirs: &Repository,
    theirs_branch: &str,
    from: &str,
) -> Result<(Vec<Change>, Vec<String>)> {
    let Some(tip) = theirs.tip(theirs_branch)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    let base = match theirs.tip(MAIN)? {
        Some(main) => theirs.merge_base(tip, main)?,
        None => None,
    };
    let kept = repo
        .entries(ours)?
        .into_iter()
        .map(|entry| (entry.path, entry.blob))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    let mut clashes = Vec::new();
    for change in theirs.diff(base, tip)? {
        match change {
            FileChange::Added { path, blob }
            | FileChange::Changed { path, to: blob, .. } => {
                if path == CHARACTER {
                    continue;
                }
                let bytes = theirs.blob(blob)?;
                match kept.get(&path) {
                    Some(own) if *own != blob => {
                        changes.push(Change::Write {
                            path: aside(&path, from),
                            bytes,
                        });
                        clashes.push(path);
                    }
                    Some(_) => {}
                    None => {
                        changes.push(Change::Write { path, bytes });
                    }
                }
            }
            FileChange::Removed { path, blob } => {
                if path != CHARACTER && kept.get(&path) == Some(&blob)
                {
                    changes.push(Change::Remove { path });
                }
            }
        }
    }
    Ok((changes, clashes))
}

/// Where a clashing `path` from `from` is kept.
fn aside(path: &str, from: &str) -> String {
    if let Some(rest) = path.strip_prefix("skills/")
        && let Some((skill, inner)) = rest.split_once('/')
    {
        return format!("skills/{skill}-from-{from}/{inner}");
    }
    format!("{path}.from-{from}")
}

/// Moves what waits in the absorbed lodge's `inbox/` into the kept
/// lodge's, under `from-<from>/`.
fn move_inbox(
    absorbed: &std::path::Path,
    kept: &std::path::Path,
    from: &str,
) -> Result<()> {
    let Ok(source) = Folder::open(absorbed) else {
        return Ok(());
    };
    let gifts = source.list_under("inbox")?;
    if gifts.is_empty() {
        return Ok(());
    }
    let target = Folder::make(kept)?;
    for gift in gifts {
        let rest = gift.path.trim_start_matches("inbox/");
        let bytes = source.read(&gift.path)?;
        target.write(
            &format!("inbox/from-{from}/{rest}"),
            &bytes,
            gift.executable,
        )?;
        source.remove(&gift.path)?;
    }
    Ok(())
}

/// Undoes merge `id` on the kept hire's branch, once, keeping what
/// the kept hire changed since.
pub(super) fn split(
    db: &mut Db,
    layout: &Layout,
    id: MergeId,
    at: i64,
) -> Result<()> {
    let found = merges::get(db, id)?;
    if found.split_commit.is_some() {
        return Ok(());
    }
    let kept = lodging(db, layout, found.kept)?;
    let Some(mark) = found.merge_commit else {
        merges::unmerged(db, id, "")?;
        return Ok(());
    };
    commit(db, layout, found.kept, at)?;
    let repo = Repository::open(&layout.repository(kept.scholar))?;
    let kept_branch = branch(found.kept);
    let name = Actor::Realm.to_string();
    let author = Author { name: &name, at };
    let tip = match mark.strip_prefix(MOVED) {
        Some(range) => {
            let (from, to) = range
                .split_once("..")
                .context("a moved merge's range")?;
            let parse = |id: &str| {
                id.parse::<CommitId>().map_err(anyhow::Error::msg)
            };
            undo_range(
                &repo,
                &kept_branch,
                parse(from)?,
                parse(to)?,
                author,
            )?
        }
        None => {
            let commit = mark
                .parse::<CommitId>()
                .map_err(anyhow::Error::msg)?;
            repo.revert(
                &kept_branch,
                commit,
                "Split the merge",
                author,
            )?
            .commit
        }
    };
    merges::unmerged(db, id, &tip.to_string())?;
    check_out(&repo, tip, &Folder::make(&kept.path)?)
}

/// Undoes on `branch` what changed from `from` to `to`, as one
/// commit, leaving a file alone where the branch changed it since.
fn undo_range(
    repo: &Repository,
    branch: &str,
    from: CommitId,
    to: CommitId,
    author: Author<'_>,
) -> Result<CommitId> {
    let tip =
        repo.tip(branch)?.context("the kept hire has no branch")?;
    let now = repo
        .entries(tip)?
        .into_iter()
        .map(|entry| (entry.path, entry.blob))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    for change in repo.diff(Some(from), to)? {
        match change {
            FileChange::Added { path, blob } => {
                if now.get(&path) == Some(&blob) {
                    changes.push(Change::Remove { path });
                }
            }
            FileChange::Changed { path, from, to } => {
                if now.get(&path) == Some(&to) {
                    changes.push(Change::Write {
                        bytes: repo.blob(from)?,
                        path,
                    });
                }
            }
            FileChange::Removed { path, blob } => {
                if !now.contains_key(&path) {
                    changes.push(Change::Write {
                        bytes: repo.blob(blob)?,
                        path,
                    });
                }
            }
        }
    }
    repo.commit(branch, &changes, "Split the merge", author)
}
