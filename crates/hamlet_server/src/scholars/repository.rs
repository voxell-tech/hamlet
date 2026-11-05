//! A scholar's repository: its character, notes and skills with their
//! whole history, kept through gix and never through a git program.

use std::path::Path;

use anyhow::{Context, Result};
use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::merge::tree::{FileFavor, TreatAsUnresolved, TreeFavor};
use gix::object::tree::EntryKind;
use gix::refs::transaction::PreviousValue;

/// The branch that is the scholar itself.
pub(crate) const MAIN: &str = "main";

/// A change to one file on a branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Write { path: String, bytes: Vec<u8> },
    Remove { path: String },
}

/// Who made a commit, as the record writes it, and when, in seconds
/// since the epoch.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Author<'a> {
    pub name: &'a str,
    pub at: i64,
}

/// A merge's commit, and the paths both sides changed.
#[derive(Debug)]
pub(crate) struct Merged {
    pub commit: ObjectId,
    pub clashes: Vec<String>,
}

/// A bare repository the server alone writes.
pub(crate) struct Repository(gix::Repository);

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
    pub fn tip(&self, branch: &str) -> Result<Option<ObjectId>> {
        let Some(mut found) =
            self.0.try_find_reference(full(branch).as_str())?
        else {
            return Ok(None);
        };
        Ok(Some(found.peel_to_id()?.detach()))
    }

    /// Starts `branch` at `at`. It must not exist yet.
    pub fn branch(&self, branch: &str, at: ObjectId) -> Result<()> {
        self.0.reference(
            full(branch).as_str(),
            at,
            PreviousValue::MustNotExist,
            "branch",
        )?;
        Ok(())
    }

    /// Deletes `branch`, when it exists.
    pub fn delete_branch(&self, branch: &str) -> Result<()> {
        if let Some(found) =
            self.0.try_find_reference(full(branch).as_str())?
        {
            found.delete()?;
        }
        Ok(())
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
    ) -> Result<ObjectId> {
        let parent = self.tip(branch)?;
        let base = match parent {
            Some(commit) => {
                self.0.find_commit(commit)?.tree_id()?.detach()
            }
            None => ObjectId::empty_tree(self.0.object_hash()),
        };
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
        if let Some(parent) = parent
            && tree == base
        {
            return Ok(parent);
        }
        self.commit_tree(
            branch,
            tree,
            parent.into_iter(),
            message,
            by,
        )
    }

    /// Every file of the commit `at`, by path, with its contents.
    pub fn files(
        &self,
        at: ObjectId,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let tree = self.0.find_commit(at)?.tree()?;
        let mut recorder = gix::traverse::tree::Recorder::default();
        tree.traverse().breadthfirst(&mut recorder)?;
        let mut files = Vec::new();
        for entry in recorder.records {
            if !entry.mode.is_blob() {
                continue;
            }
            let path = entry.filepath.to_str()?.to_owned();
            let bytes = self.0.find_blob(entry.oid)?.take_data();
            files.push((path, bytes));
        }
        Ok(files)
    }

    /// The file at `path` in the commit `at`, if there is one.
    pub fn read(
        &self,
        at: ObjectId,
        path: &str,
    ) -> Result<Option<Vec<u8>>> {
        let tree = self.0.find_commit(at)?.tree()?;
        let Some(entry) = tree.lookup_entry_by_path(path)? else {
            return Ok(None);
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        Ok(Some(entry.object()?.detach().data))
    }

    /// Merges the commit `from` into `branch`. Where both changed the
    /// same lines, `branch`'s are kept and the path is a clash.
    pub fn merge(
        &self,
        branch: &str,
        from: ObjectId,
        message: &str,
        by: Author<'_>,
    ) -> Result<Merged> {
        let ours = self.tip(branch)?.with_context(|| {
            format!("merging into {branch}, which is empty")
        })?;
        let options = gix::merge::commit::Options::from(
            self.0
                .tree_merge_options()?
                .with_file_favor(Some(FileFavor::Ours))
                .with_tree_favor(Some(TreeFavor::Ours)),
        );
        let mut outcome = self.0.merge_commits(
            ours,
            from,
            Default::default(),
            options,
        )?;
        let mut clashes = outcome
            .tree_merge
            .conflicts
            .iter()
            // Only what keeping our side settled; the rest merged
            // cleanly.
            .filter(|conflict| {
                conflict.is_unresolved(
                    TreatAsUnresolved::forced_resolution(),
                )
            })
            .filter_map(|conflict| {
                conflict
                    .ours
                    .location()
                    .to_str()
                    .ok()
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        clashes.sort();
        clashes.dedup();
        let tree = outcome.tree_merge.tree.write()?.detach();
        let commit = self.commit_tree(
            branch,
            tree,
            [ours, from].into_iter(),
            message,
            by,
        )?;
        Ok(Merged { commit, clashes })
    }

    fn commit_tree(
        &self,
        branch: &str,
        tree: ObjectId,
        parents: impl Iterator<Item = ObjectId>,
        message: &str,
        by: Author<'_>,
    ) -> Result<ObjectId> {
        let signature = gix::actor::Signature {
            name: by.name.into(),
            email: "".into(),
            time: gix::date::Time::new(by.at, 0),
        };
        let mut time = gix::date::parse::TimeBuf::default();
        let signature = signature.to_ref(&mut time);
        let id = self.0.commit_as(
            signature,
            signature,
            full(branch).as_str(),
            message,
            tree,
            parents,
        )?;
        Ok(id.detach())
    }
}

fn full(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    fn by(at: i64) -> Author<'static> {
        Author {
            name: "chief:nixon",
            at,
        }
    }

    fn write(path: &str, text: &str) -> Change {
        Change::Write {
            path: path.into(),
            bytes: text.into(),
        }
    }

    fn text(
        repo: &Repository,
        at: ObjectId,
        path: &str,
    ) -> Option<String> {
        repo.read(at, path)
            .unwrap()
            .map(|bytes| String::from_utf8(bytes).unwrap())
    }

    #[test]
    fn a_hire_branch_learns_apart_and_takes_in_main() {
        let dir = Scratch::new("repository");
        let repo = Repository::create(&dir.join("repo.git")).unwrap();
        let first = repo
            .commit(
                MAIN,
                &[
                    write("CHARACTER.md", "Careful."),
                    write("NOTES.md", "one\ntwo\nthree\n"),
                    write("skills/rust/SKILL.md", "Use clippy."),
                ],
                "made",
                by(1),
            )
            .unwrap();
        repo.branch("hire/mill-4", first).unwrap();

        // The hire changes its notes' first line; the chief changes
        // the last and gives a skill.
        repo.commit(
            "hire/mill-4",
            &[write("NOTES.md", "ONE\ntwo\nthree\n")],
            "turn",
            by(2),
        )
        .unwrap();
        let main = repo
            .commit(
                MAIN,
                &[
                    write("NOTES.md", "one\ntwo\nTHREE\n"),
                    write("skills/git/SKILL.md", "Small commits."),
                ],
                "edited",
                by(3),
            )
            .unwrap();

        let merged = repo
            .merge("hire/mill-4", main, "took in main", by(4))
            .unwrap();
        assert!(merged.clashes.is_empty());
        assert_eq!(
            text(&repo, merged.commit, "NOTES.md").as_deref(),
            Some("ONE\ntwo\nTHREE\n")
        );
        assert!(
            text(&repo, merged.commit, "skills/git/SKILL.md")
                .is_some()
        );
        // Main never sees what the hire learned.
        assert_eq!(
            text(&repo, repo.tip(MAIN).unwrap().unwrap(), "NOTES.md")
                .as_deref(),
            Some("one\ntwo\nTHREE\n")
        );
    }

    #[test]
    fn a_clash_keeps_the_hires_lines() {
        let dir = Scratch::new("repository");
        let repo = Repository::create(&dir.join("repo.git")).unwrap();
        let first = repo
            .commit(MAIN, &[write("NOTES.md", "a\n")], "made", by(1))
            .unwrap();
        repo.branch("hire/mill-4", first).unwrap();
        repo.commit(
            "hire/mill-4",
            &[write("NOTES.md", "hire\n")],
            "t",
            by(2),
        )
        .unwrap();
        let main = repo
            .commit(MAIN, &[write("NOTES.md", "chief\n")], "e", by(3))
            .unwrap();

        let merged =
            repo.merge("hire/mill-4", main, "m", by(4)).unwrap();
        assert_eq!(merged.clashes, ["NOTES.md"]);
        assert_eq!(
            text(&repo, merged.commit, "NOTES.md").as_deref(),
            Some("hire\n")
        );
    }

    #[test]
    fn a_commit_that_changes_nothing_is_not_made() {
        let dir = Scratch::new("repository");
        let repo = Repository::create(&dir.join("repo.git")).unwrap();
        let first = repo
            .commit(MAIN, &[write("NOTES.md", "a\n")], "made", by(1))
            .unwrap();
        let again = repo
            .commit(MAIN, &[write("NOTES.md", "a\n")], "same", by(2))
            .unwrap();
        assert_eq!(first, again);
        let files = repo.files(first).unwrap();
        assert_eq!(files, [("NOTES.md".to_owned(), b"a\n".to_vec())]);
    }
}
