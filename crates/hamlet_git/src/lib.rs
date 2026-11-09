//! Repositories the server alone writes, through gix and never a git
//! program: a scholar's character, memory and skills with their whole
//! history, its hires' branches and its versions.
//!
//! Files are read and written by the caller, so a lodge on this
//! machine or on a remote workspace is kept the same way: the caller
//! lists its files with their blob ids, and this crate plans the
//! writes that check a commit out, or commits what is listed.

mod purge;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::merge::tree::{FileFavor, TreatAsUnresolved, TreeFavor};
use gix::object::tree::EntryKind;
use gix::objs::Write as _;
use gix::refs::Target;
use gix::refs::transaction::PreviousValue;
pub use purge::purge;
use serde::{Deserialize, Serialize};

/// The branch that is the scholar itself.
pub const MAIN: &str = "main";

macro_rules! object_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
            Serialize, Deserialize,
        )]
        #[serde(into = "String", try_from = "String")]
        pub struct $name(ObjectId);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(text: &str) -> Result<Self, String> {
                ObjectId::from_hex(text.as_bytes())
                    .map(Self)
                    .map_err(|_| format!("not an object id: {text:?}"))
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.to_string()
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(text: String) -> Result<Self, String> {
                text.parse()
            }
        }
    };
}

object_id!(
    /// A commit of a repository.
    CommitId
);
object_id!(
    /// A file's contents as git stores them.
    BlobId
);

/// The id git gives `bytes` as a file.
pub fn blob_id(bytes: &[u8]) -> BlobId {
    BlobId(
        gix::objs::compute_hash(
            gix::hash::Kind::Sha1,
            gix::objs::Kind::Blob,
            bytes,
        )
        .expect("SHA-1 hashes any bytes"),
    )
}

/// A file of a tree or a folder: its path with `/` between names, its
/// contents' id and whether it runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub blob: BlobId,
    pub executable: bool,
}

/// A change to one file on a branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Write { path: String, bytes: Vec<u8> },
    Remove { path: String },
}

/// Who made a commit, as the record names it, and when in
/// milliseconds since the epoch.
#[derive(Clone, Copy, Debug)]
pub struct Author<'a> {
    pub name: &'a str,
    pub at: i64,
}

/// A commit as the history shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub id: CommitId,
    pub parents: Vec<CommitId>,
    pub author: String,
    pub at: i64,
    pub message: String,
}

/// A tag of a scholar's history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub name: String,
    pub commit: CommitId,
    pub note: String,
    pub by: String,
    pub at: i64,
}

/// How a file differs between two commits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileChange {
    Added {
        path: String,
        blob: BlobId,
    },
    Changed {
        path: String,
        from: BlobId,
        to: BlobId,
    },
    Removed {
        path: String,
        blob: BlobId,
    },
}

impl FileChange {
    pub fn path(&self) -> &str {
        match self {
            Self::Added { path, .. }
            | Self::Changed { path, .. }
            | Self::Removed { path, .. } => path,
        }
    }
}

/// What a merge or a revert did to a branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joined {
    /// The branch already held everything.
    UpToDate,
    /// The branch moved forward to the other side.
    FastForward,
    /// A new commit joins both sides.
    Committed,
}

/// The branch's tip after a merge or a revert, and the paths both
/// sides changed, where the branch's side was kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merged {
    pub commit: CommitId,
    pub joined: Joined,
    pub clashes: Vec<String>,
}

/// One write that brings a folder to a commit's files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Write {
        path: String,
        blob: BlobId,
        executable: bool,
    },
    Remove {
        path: String,
    },
}

/// A bare repository.
pub struct Repository(gix::Repository);

impl Repository {
    /// Creates an empty bare repository at `path`.
    pub fn create(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)?;
        let repo = gix::ThreadSafeRepository::init_opts(
            path,
            gix::create::Kind::Bare,
            gix::create::Options::default(),
            gix::open::Options::isolated(),
        )
        .with_context(|| {
            format!("creating the repository {}", path.display())
        })?;
        Ok(Self(repo.into()))
    }

    /// Opens the repository at `path`, reading no configuration of
    /// the machine's or the chief's.
    pub fn open(path: &Path) -> Result<Self> {
        let repo =
            gix::open_opts(path, gix::open::Options::isolated())
                .with_context(|| {
                    format!(
                        "opening the repository {}",
                        path.display()
                    )
                })?;
        Ok(Self(repo))
    }

    /// The commit `branch` points at, or `None` when it has none.
    pub fn tip(&self, branch: &str) -> Result<Option<CommitId>> {
        self.peel(&heads(branch))
    }

    /// The commit a branch, a version or a commit id names.
    pub fn resolve(&self, name: &str) -> Result<Option<CommitId>> {
        if let Some(commit) = self.tip(name)? {
            return Ok(Some(commit));
        }
        if let Some(commit) = self.peel(&tags(name))? {
            return Ok(Some(commit));
        }
        let Ok(id) = name.parse::<CommitId>() else {
            return Ok(None);
        };
        Ok(self
            .0
            .find_object(id.0)
            .ok()
            .filter(|object| object.kind == gix::objs::Kind::Commit)
            .map(|_| id))
    }

    fn peel(&self, name: &str) -> Result<Option<CommitId>> {
        let Some(mut found) = self.0.try_find_reference(name)? else {
            return Ok(None);
        };
        Ok(Some(CommitId(found.peel_to_commit()?.id)))
    }

    /// Starts `branch` at `at`. It must not exist yet.
    pub fn branch(&self, branch: &str, at: CommitId) -> Result<()> {
        self.0.reference(
            heads(branch),
            at.0,
            PreviousValue::MustNotExist,
            "branch",
        )?;
        Ok(())
    }

    /// Moves `branch` to `to`, if it is still at `was`.
    pub fn set_branch(
        &self,
        branch: &str,
        to: CommitId,
        was: CommitId,
    ) -> Result<()> {
        self.0.reference(
            heads(branch),
            to.0,
            PreviousValue::MustExistAndMatch(Target::Object(was.0)),
            "moved",
        )?;
        Ok(())
    }

    /// Deletes `branch`, when it exists.
    pub fn delete_branch(&self, branch: &str) -> Result<()> {
        if let Some(found) =
            self.0.try_find_reference(&heads(branch))?
        {
            found.delete()?;
        }
        Ok(())
    }

    /// Every branch, by name, with its tip.
    pub fn branches(&self) -> Result<Vec<(String, CommitId)>> {
        let mut out = Vec::new();
        for reference in self.0.references()?.local_branches()? {
            let mut reference =
                reference.map_err(|e| anyhow::anyhow!("{e}"))?;
            let name = reference
                .name()
                .shorten()
                .to_str()
                .context("a branch's name is not UTF-8")?
                .to_owned();
            out.push((
                name,
                CommitId(reference.peel_to_commit()?.id),
            ));
        }
        Ok(out)
    }

    /// Tags `at` as the version `name`. The name must be free.
    pub fn tag(
        &self,
        name: &str,
        at: CommitId,
        note: &str,
        by: Author<'_>,
    ) -> Result<()> {
        let mut time = gix::date::parse::TimeBuf::default();
        let signature = signature(by);
        self.0.tag(
            name,
            at.0,
            gix::objs::Kind::Commit,
            Some(signature.to_ref(&mut time)),
            note,
            PreviousValue::MustNotExist,
        )?;
        Ok(())
    }

    /// Deletes the version `name`, when it exists.
    pub fn delete_tag(&self, name: &str) -> Result<()> {
        if let Some(found) = self.0.try_find_reference(&tags(name))? {
            found.delete()?;
        }
        Ok(())
    }

    /// Every version, oldest first.
    pub fn versions(&self) -> Result<Vec<Version>> {
        let mut out = Vec::new();
        for reference in self.0.references()?.tags()? {
            let mut reference =
                reference.map_err(|e| anyhow::anyhow!("{e}"))?;
            let name = reference
                .name()
                .shorten()
                .to_str()
                .context("a version's name is not UTF-8")?
                .to_owned();
            let commit = CommitId(reference.peel_to_commit()?.id);
            let target =
                reference.target().try_id().map(ToOwned::to_owned);
            let (note, by, at) = match target
                .map(|id| self.0.find_tag(id))
                .transpose()
                .ok()
                .flatten()
            {
                Some(tag) => {
                    let decoded = tag.decode()?;
                    let tagger = decoded.tagger()?;
                    (
                        decoded
                            .message
                            .to_str_lossy()
                            .trim()
                            .to_owned(),
                        tagger
                            .map(|t| {
                                t.name.to_str_lossy().into_owned()
                            })
                            .unwrap_or_default(),
                        tagger
                            .map(|t| t.seconds() * 1000)
                            .unwrap_or_default(),
                    )
                }
                None => (String::new(), String::new(), 0),
            };
            out.push(Version {
                name,
                commit,
                note,
                by,
                at,
            });
        }
        out.sort_by(|a, b| a.at.cmp(&b.at).then(a.name.cmp(&b.name)));
        Ok(out)
    }

    /// Commits `changes` on top of `branch`, which starts empty when
    /// it does not exist yet. Returns the new commit, or the tip when
    /// the changes leave the files as they were.
    pub fn commit(
        &self,
        branch: &str,
        changes: &[Change],
        message: &str,
        by: Author<'_>,
    ) -> Result<CommitId> {
        let parent = self.tip(branch)?;
        let base = self.tree_of(parent)?;
        let mut editor = self.0.edit_tree(base)?;
        for change in changes {
            match change {
                Change::Write { path, bytes } => {
                    let blob = self.0.write_blob(bytes)?.detach();
                    editor.upsert(
                        path.as_str(),
                        EntryKind::Blob,
                        blob,
                    )?;
                }
                Change::Remove { path } => {
                    editor.remove(path.as_str())?;
                }
            }
        }
        let tree = editor.write()?.detach();
        match parent {
            Some(parent) if tree == base => Ok(parent),
            _ => self.commit_tree(
                branch,
                tree,
                parent,
                &[],
                message,
                by,
            ),
        }
    }

    /// Keeps `bytes` as a file's contents.
    pub fn write_blob(&self, bytes: &[u8]) -> Result<BlobId> {
        Ok(BlobId(self.0.write_blob(bytes)?.detach()))
    }

    pub fn has_blob(&self, blob: BlobId) -> bool {
        self.0.has_object(blob.0)
    }

    /// The contents of a file.
    pub fn blob(&self, blob: BlobId) -> Result<Vec<u8>> {
        Ok(self.0.find_blob(blob.0)?.take_data())
    }

    /// Commits the files of `listing` on top of `branch` as its whole
    /// tree, but for the paths `keep` names, which stay as the tip
    /// has them. Every listed blob must be kept already. `also` are
    /// further parents, for a merge. Returns `None` when the tree is
    /// the tip's and there are no further parents.
    pub fn commit_listing(
        &self,
        branch: &str,
        listing: &[Entry],
        keep: &dyn Fn(&str) -> bool,
        also: &[CommitId],
        message: &str,
        by: Author<'_>,
    ) -> Result<Option<CommitId>> {
        let parent = self.tip(branch)?;
        let base = self.tree_of(parent)?;
        let mut editor = self
            .0
            .edit_tree(ObjectId::empty_tree(self.0.object_hash()))?;
        for entry in listing {
            if keep(&entry.path) {
                continue;
            }
            if !self.has_blob(entry.blob) {
                bail!("{} is not kept yet", entry.path);
            }
            editor.upsert(
                entry.path.as_str(),
                kind(entry.executable),
                entry.blob.0,
            )?;
        }
        if let Some(parent) = parent {
            for entry in self.entries(parent)? {
                if keep(&entry.path) {
                    editor.upsert(
                        entry.path.as_str(),
                        kind(entry.executable),
                        entry.blob.0,
                    )?;
                }
            }
        }
        let tree = editor.write()?.detach();
        if parent.is_some() && tree == base && also.is_empty() {
            return Ok(None);
        }
        self.commit_tree(branch, tree, parent, also, message, by)
            .map(Some)
    }

    /// Every file of the commit `at`, without its contents.
    pub fn entries(&self, at: CommitId) -> Result<Vec<Entry>> {
        let tree = self.0.find_commit(at.0)?.tree()?;
        let mut recorder = gix::traverse::tree::Recorder::default();
        tree.traverse().breadthfirst(&mut recorder)?;
        let mut out = Vec::new();
        for entry in recorder.records {
            if !entry.mode.is_blob() {
                continue;
            }
            out.push(Entry {
                path: entry.filepath.to_str()?.to_owned(),
                blob: BlobId(entry.oid),
                executable: entry.mode.is_executable(),
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Every file of the commit `at`, by path, with its contents.
    pub fn files(
        &self,
        at: CommitId,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        self.entries(at)?
            .into_iter()
            .map(|entry| Ok((entry.path, self.blob(entry.blob)?)))
            .collect()
    }

    /// The file at `path` in the commit `at`, if there is one.
    pub fn read(
        &self,
        at: CommitId,
        path: &str,
    ) -> Result<Option<Vec<u8>>> {
        let tree = self.0.find_commit(at.0)?.tree()?;
        let Some(entry) = tree.lookup_entry_by_path(path)? else {
            return Ok(None);
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        Ok(Some(entry.object()?.detach().data))
    }

    /// How the files of `to` differ from those of `from`, or from no
    /// files at all, by path.
    pub fn diff(
        &self,
        from: Option<CommitId>,
        to: CommitId,
    ) -> Result<Vec<FileChange>> {
        let old = match from {
            Some(from) => by_path(self.entries(from)?),
            None => BTreeMap::new(),
        };
        let new = by_path(self.entries(to)?);
        let mut out = Vec::new();
        for (path, entry) in &new {
            match old.get(path) {
                None => out.push(FileChange::Added {
                    path: path.clone(),
                    blob: entry.blob,
                }),
                Some(was) if was.blob != entry.blob => {
                    out.push(FileChange::Changed {
                        path: path.clone(),
                        from: was.blob,
                        to: entry.blob,
                    });
                }
                Some(_) => {}
            }
        }
        for (path, entry) in &old {
            if !new.contains_key(path) {
                out.push(FileChange::Removed {
                    path: path.clone(),
                    blob: entry.blob,
                });
            }
        }
        out.sort_by(|a, b| a.path().cmp(b.path()));
        Ok(out)
    }

    /// The commits reachable from `from`, newest first, at most
    /// `limit`.
    pub fn history(
        &self,
        from: CommitId,
        limit: usize,
    ) -> Result<Vec<Commit>> {
        let mut out = Vec::new();
        for info in self.0.rev_walk([from.0]).all()? {
            if out.len() == limit {
                break;
            }
            let info = info?;
            let commit = self.0.find_commit(info.id)?;
            let decoded = commit.decode()?;
            let author = decoded.author()?;
            out.push(Commit {
                id: CommitId(info.id),
                parents: decoded.parents().map(CommitId).collect(),
                author: author.name.to_str_lossy().into_owned(),
                at: author.seconds() * 1000,
                message: decoded
                    .message
                    .to_str_lossy()
                    .trim()
                    .to_owned(),
            });
        }
        Ok(out)
    }

    /// The best common ancestor of `one` and `two`.
    pub fn merge_base(
        &self,
        one: CommitId,
        two: CommitId,
    ) -> Result<Option<CommitId>> {
        Ok(self
            .0
            .merge_base(one.0, two.0)?
            .map(|id| CommitId(id.detach())))
    }

    /// Merges the commit `from` into `branch`. Where both changed the
    /// same lines, `branch`'s are kept and the path is a clash.
    pub fn merge(
        &self,
        branch: &str,
        from: CommitId,
        message: &str,
        by: Author<'_>,
    ) -> Result<Merged> {
        let ours = self.tip(branch)?.with_context(|| {
            format!("merging into {branch}, which is empty")
        })?;
        let base = self.merge_base(ours, from)?;
        if base == Some(from) {
            return Ok(Merged {
                commit: ours,
                joined: Joined::UpToDate,
                clashes: Vec::new(),
            });
        }
        if base == Some(ours) {
            self.set_branch(branch, from, ours)?;
            return Ok(Merged {
                commit: from,
                joined: Joined::FastForward,
                clashes: Vec::new(),
            });
        }
        let options =
            gix::merge::commit::Options::from(self.ours_win()?);
        let mut outcome = self.0.merge_commits(
            ours.0,
            from.0,
            Default::default(),
            options,
        )?;
        let clashes = clashes(&outcome.tree_merge.conflicts);
        let tree = outcome.tree_merge.tree.write()?.detach();
        let commit = self.commit_tree(
            branch,
            tree,
            Some(ours),
            &[from],
            message,
            by,
        )?;
        Ok(Merged {
            commit,
            joined: Joined::Committed,
            clashes,
        })
    }

    /// Undoes `commit` on `branch` with a new commit, by merging its
    /// parent's files over the tip with the commit as the base. A
    /// path changed again since keeps the tip's lines and is a clash.
    pub fn revert(
        &self,
        branch: &str,
        commit: CommitId,
        message: &str,
        by: Author<'_>,
    ) -> Result<Merged> {
        let ours = self.tip(branch)?.with_context(|| {
            format!("reverting on {branch}, which is empty")
        })?;
        let undone = self.0.find_commit(commit.0)?;
        let parent = undone
            .parent_ids()
            .next()
            .map(|id| CommitId(id.detach()));
        let base = undone.tree_id()?.detach();
        let theirs = self.tree_of(parent)?;
        let mut outcome = self.0.merge_trees(
            base,
            self.tree_of(Some(ours))?,
            theirs,
            Default::default(),
            self.ours_win()?,
        )?;
        let clashes = clashes(&outcome.conflicts);
        let tree = outcome.tree.write()?.detach();
        if tree == self.tree_of(Some(ours))? {
            return Ok(Merged {
                commit: ours,
                joined: Joined::UpToDate,
                clashes,
            });
        }
        let commit = self.commit_tree(
            branch,
            tree,
            Some(ours),
            &[],
            message,
            by,
        )?;
        Ok(Merged {
            commit,
            joined: Joined::Committed,
            clashes,
        })
    }

    /// The writes that make a folder holding `listing` hold the files
    /// of `at`, leaving alone the paths `skip` names on both sides.
    pub fn plan(
        &self,
        at: CommitId,
        listing: &[Entry],
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<Step>> {
        let wanted = by_path(
            self.entries(at)?
                .into_iter()
                .filter(|entry| !skip(&entry.path))
                .collect(),
        );
        let held = by_path(
            listing
                .iter()
                .filter(|entry| !skip(&entry.path))
                .cloned()
                .collect(),
        );
        let mut steps = Vec::new();
        for path in held.keys() {
            if !wanted.contains_key(path) {
                steps.push(Step::Remove { path: path.clone() });
            }
        }
        for (path, entry) in &wanted {
            if held.get(path) != Some(entry) {
                steps.push(Step::Write {
                    path: path.clone(),
                    blob: entry.blob,
                    executable: entry.executable,
                });
            }
        }
        Ok(steps)
    }

    /// Starts `branch` in this repository with the files of `at` in
    /// `from` as its one commit, copying what it needs. The branch
    /// must not exist yet.
    pub fn start_from(
        &self,
        branch: &str,
        from: &Repository,
        at: CommitId,
        message: &str,
        by: Author<'_>,
    ) -> Result<CommitId> {
        let tree = from.0.find_commit(at.0)?.tree_id()?.detach();
        copy_object(&from.0, &self.0, tree)?;
        self.commit_tree(branch, tree, None, &[], message, by)
    }

    fn tree_of(&self, commit: Option<CommitId>) -> Result<ObjectId> {
        Ok(match commit {
            Some(commit) => {
                self.0.find_commit(commit.0)?.tree_id()?.detach()
            }
            None => ObjectId::empty_tree(self.0.object_hash()),
        })
    }

    fn ours_win(&self) -> Result<gix::merge::tree::Options> {
        Ok(self
            .0
            .tree_merge_options()?
            .with_file_favor(Some(FileFavor::Ours))
            .with_tree_favor(Some(TreeFavor::Ours)))
    }

    fn commit_tree(
        &self,
        branch: &str,
        tree: ObjectId,
        parent: Option<CommitId>,
        also: &[CommitId],
        message: &str,
        by: Author<'_>,
    ) -> Result<CommitId> {
        let signature = signature(by);
        let mut time = gix::date::parse::TimeBuf::default();
        let signature = signature.to_ref(&mut time);
        let parents =
            parent.iter().chain(also).map(|commit| commit.0);
        let id = self.0.commit_as(
            signature,
            signature,
            heads(branch),
            message,
            tree,
            parents,
        )?;
        Ok(CommitId(id.detach()))
    }
}

fn heads(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

fn tags(name: &str) -> String {
    format!("refs/tags/{name}")
}

fn kind(executable: bool) -> EntryKind {
    if executable {
        EntryKind::BlobExecutable
    } else {
        EntryKind::Blob
    }
}

fn signature(by: Author<'_>) -> gix::actor::Signature {
    gix::actor::Signature {
        name: by.name.into(),
        email: "".into(),
        time: gix::date::Time::new(by.at.div_euclid(1000), 0),
    }
}

fn by_path(entries: Vec<Entry>) -> BTreeMap<String, Entry> {
    entries
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect()
}

/// The paths a merge settled by keeping our side; the rest merged
/// cleanly.
fn clashes(conflicts: &[gix::merge::tree::Conflict]) -> Vec<String> {
    let mut out = conflicts
        .iter()
        .filter(|conflict| {
            conflict
                .is_unresolved(TreatAsUnresolved::forced_resolution())
        })
        .filter_map(|conflict| {
            conflict.ours.location().to_str().ok().map(str::to_owned)
        })
        .collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}

/// Copies the object `id` from `from` into `to`, with every object it
/// reaches.
fn copy_object(
    from: &gix::Repository,
    to: &gix::Repository,
    id: ObjectId,
) -> Result<()> {
    if to.has_object(id) {
        return Ok(());
    }
    let object = from.find_object(id)?;
    match object.kind {
        gix::objs::Kind::Tree => {
            let tree = object.clone().into_tree();
            for entry in tree.decode()?.entries.iter() {
                if !entry.mode.is_commit() {
                    copy_object(from, to, entry.oid.to_owned())?;
                }
            }
        }
        gix::objs::Kind::Commit => {
            let commit = object.clone().into_commit();
            let decoded = commit.decode()?;
            copy_object(from, to, decoded.tree())?;
            for parent in decoded.parents() {
                copy_object(from, to, parent)?;
            }
        }
        gix::objs::Kind::Tag => {
            let tag = object.clone().into_tag();
            copy_object(from, to, tag.target_id()?.detach())?;
        }
        gix::objs::Kind::Blob => {}
    }
    let written = to
        .write_buf(object.kind, &object.data)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if written != id {
        bail!("copying {id} gave {written}");
    }
    Ok(())
}
