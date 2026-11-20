//! An owner's repository: the folder its exclusion guards, and the
//! repository a unit opens there on its own thread. A
//! `hamlet_git::Repository` is neither `Send` nor `Sync`, so none is
//! kept between units.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use hamlet_git::{
    Author, BlobId, Change, Commit, CommitId, Entry, MAIN, Merged,
    Repository, Step, Version, Wins,
};
use hamlet_messages::workspace::AgentNumber;

use super::checkout;

/// The message of a save's commit.
const SAVED: &str = "Kept what it learned";

/// The suffix of the folder an erased repository is moved to.
const ERASED: &str = "erased";

/// The folder of an owner's repository.
pub(crate) struct Folder {
    path: PathBuf,
    /// The realm folder, which the checkouts are under.
    home: PathBuf,
}

/// A repository open for one unit, on the unit's thread.
pub(crate) struct Held {
    path: PathBuf,
    home: PathBuf,
    /// `None` only while a purge replaces the folder, and after an
    /// erase.
    git: Option<Reads>,
    /// The time of the unit, for every commit and tag it makes.
    at: i64,
}

/// A repository as a read sees it: no method of it writes.
pub(crate) struct Reads(Repository);

impl Reads {
    pub fn tip(&self, branch: &str) -> Result<Option<CommitId>> {
        self.0.tip(branch)
    }

    pub fn versions(&self) -> Result<Vec<Version>> {
        self.0.versions()
    }

    pub fn read(
        &self,
        at: CommitId,
        path: &str,
    ) -> Result<Option<Vec<u8>>> {
        self.0.read(at, path)
    }

    /// The commit a branch, a version or a full commit id names, in
    /// that order. A name no ref can have, such as `no such`, names
    /// nothing.
    pub fn resolve(&self, name: &str) -> Result<Option<CommitId>> {
        if let Some((_, tip)) = self
            .0
            .branches()?
            .into_iter()
            .find(|(branch, _)| branch == name)
        {
            return Ok(Some(tip));
        }
        if let Some(version) = self
            .0
            .versions()?
            .into_iter()
            .find(|seen| seen.name == name)
        {
            return Ok(Some(version.commit));
        }
        match name.parse::<CommitId>() {
            Ok(id) if self.0.has_commit(id)? => Ok(Some(id)),
            _ => Ok(None),
        }
    }

    pub fn entries(&self, at: CommitId) -> Result<Vec<Entry>> {
        self.0.entries(at)
    }

    pub fn blob(&self, blob: BlobId) -> Result<Vec<u8>> {
        self.0.blob(blob)
    }

    pub fn history(
        &self,
        from: CommitId,
        limit: usize,
    ) -> Result<Vec<Commit>> {
        self.0.history(from, limit)
    }

    pub fn merge_base(
        &self,
        one: CommitId,
        two: CommitId,
    ) -> Result<Option<CommitId>> {
        self.0.merge_base(one, two)
    }

    /// Whether `commit` is `branch`'s tip or one of its ancestors.
    pub fn holds(
        &self,
        branch: &str,
        commit: CommitId,
    ) -> Result<bool> {
        let Some(tip) = self.0.tip(branch)? else {
            return Ok(false);
        };
        Ok(self.merge_base(tip, commit)? == Some(commit))
    }

    /// The newest commit by `by` on `branch` since `since`, walking
    /// first parents only, so a `main` taken into the branch since
    /// does not hide it.
    pub fn made_by(
        &self,
        branch: &str,
        since: CommitId,
        by: By,
    ) -> Result<Option<CommitId>> {
        let Some(tip) = self.0.tip(branch)? else {
            return Ok(None);
        };
        let by = by.name();
        Ok(self
            .0
            .first_parents(tip, Some(since), usize::MAX)?
            .into_iter()
            .find(|commit| commit.author == by)
            .map(|commit| commit.id))
    }

    pub fn has(&self, at: CommitId) -> Result<bool> {
        self.0.has_commit(at)
    }
}

/// The author of a commit or a tag, as the record names it. Every
/// write takes one, so no action builds an author from a free string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum By {
    Chief(u32),
    Agent(AgentNumber),
    Realm,
    Dream(u32),
}

impl By {
    pub fn name(self) -> String {
        match self {
            Self::Chief(chief) => format!("chief:{chief}"),
            Self::Agent(agent) => format!("agent:{}", agent.0),
            Self::Realm => "realm".to_owned(),
            Self::Dream(dream) => format!("dream:{dream}"),
        }
    }
}

impl From<hamlet_messages::profile::By> for By {
    fn from(by: hamlet_messages::profile::By) -> Self {
        match by {
            hamlet_messages::profile::By::Chief { chief } => {
                Self::Chief(chief)
            }
            hamlet_messages::profile::By::Agent { agent } => {
                Self::Agent(agent)
            }
        }
    }
}

/// The repository while a unit writes: its reads, and writes that
/// each take an author and the unit's time, every ref move checked
/// against the tip the unit read.
pub(crate) struct Git<'a> {
    repo: &'a Repository,
    at: i64,
}

impl Git<'_> {
    fn author<'n>(&self, name: &'n str) -> Author<'n> {
        Author { name, at: self.at }
    }

    /// Makes the annotated tag `version`; it must not exist yet.
    pub fn tag(
        &self,
        version: &str,
        at: CommitId,
        note: &str,
        by: By,
    ) -> Result<()> {
        let by = by.name();
        self.repo.tag(version, at, note, self.author(&by))
    }

    /// Makes `branch` at `at`; it must not exist yet.
    pub fn branch(&self, branch: &str, at: CommitId) -> Result<()> {
        self.repo.branch(branch, at)
    }

    /// Merges `from` into `branch`, the branch's side winning
    /// clashes.
    pub fn take_in(
        &self,
        branch: &str,
        from: CommitId,
        message: &str,
    ) -> Result<Merged> {
        let by = By::Realm.name();
        self.repo.merge(branch, from, message, self.author(&by))
    }

    /// Undoes `commit` on `branch`, the tip winning clashes.
    pub fn revert(
        &self,
        branch: &str,
        commit: CommitId,
        message: &str,
        by: By,
    ) -> Result<Merged> {
        let by = by.name();
        self.repo.revert(branch, commit, message, self.author(&by))
    }

    /// A save: `base` plus `changes` as one commit by `by`, joined to
    /// the branch's tip with the save's side winning, one
    /// compare-and-swap on the branch. A tip that already holds the
    /// changes (a repeat a kill cut off) commits nothing and is
    /// returned.
    pub fn save_onto(
        &self,
        branch: &str,
        base: CommitId,
        changes: &[Change],
        by: By,
    ) -> Result<CommitId> {
        match self.join_onto(
            branch,
            base,
            changes,
            SAVED,
            by,
            Wins::From,
        )? {
            Some(tip) => Ok(tip),
            None => self
                .repo
                .tip(branch)?
                .with_context(|| format!("no branch {branch}")),
        }
    }

    /// Commits a dream's checkout as a three-way merge onto `branch`,
    /// `base` the commit the checkout was made from, `wins` the side
    /// kept where both changed a file. `None` when the checkout
    /// changed nothing.
    pub fn commit_checkout(
        &self,
        branch: &str,
        base: CommitId,
        files: &[Change],
        message: &str,
        by: By,
        wins: Wins,
    ) -> Result<Option<CommitId>> {
        self.join_onto(branch, base, files, message, by, wins)
    }

    /// `changes` committed on `base`, then merged into `branch`'s
    /// tip, `wins` the side kept where both changed a file: the
    /// branch's new tip, or `None` when they leave `base`'s files as
    /// they were.
    fn join_onto(
        &self,
        branch: &str,
        base: CommitId,
        changes: &[Change],
        message: &str,
        by: By,
        wins: Wins,
    ) -> Result<Option<CommitId>> {
        let tip = self
            .repo
            .tip(branch)?
            .with_context(|| format!("no branch {branch}"))?;
        let by = by.name();
        let made = self.repo.commit_on(
            Some(base),
            changes,
            message,
            self.author(&by),
        )?;
        if made == base {
            return Ok(None);
        }
        let merged = self.repo.merge_commits(
            tip,
            made,
            wins,
            message,
            self.author(&by),
        )?;
        if merged.commit != tip {
            self.repo.set_branch(branch, merged.commit, tip)?;
        }
        Ok(Some(merged.commit))
    }

    /// Makes `branch` with the files of `at` in the repository at
    /// `source`, which is only read, as its one commit.
    pub fn start_from(
        &self,
        branch: &str,
        source: &Path,
        at: CommitId,
        message: &str,
        by: By,
    ) -> Result<CommitId> {
        let source = Repository::open(source)?;
        let by = by.name();
        self.repo.start_from(
            branch,
            &source,
            at,
            message,
            self.author(&by),
        )
    }

    /// Commits `changes` onto `branch`, whose tip must still be
    /// `was`.
    pub fn commit(
        &self,
        branch: &str,
        was: Option<CommitId>,
        changes: &[Change],
        message: &str,
        by: By,
    ) -> Result<CommitId> {
        let by = by.name();
        self.repo.commit_if(
            branch,
            was,
            changes,
            message,
            self.author(&by),
        )
    }
}

impl Folder {
    /// Readies the repository at `path` as an owner's start does: a
    /// purge a kill cut off is finished and the locks a kill left are
    /// removed. One that is not there is made by `make` in a folder
    /// beside it and moved into place, so a kill leaves none or a
    /// whole one. One that is there and does not open fails the
    /// start: it is never made again over. One that was erased fails
    /// the start too, once what an erase a kill cut off left of it is
    /// removed. Blocks.
    pub fn start(
        path: PathBuf,
        home: PathBuf,
        at: i64,
        make: impl FnOnce(&Git<'_>) -> Result<()>,
    ) -> Result<Self> {
        let erased = beside(&path, ERASED)?;
        if erased.exists() {
            empty(&erased)?;
            bail!("the repository {} was erased", path.display());
        }
        let making = beside(&path, "making")?;
        if making.exists() {
            std::fs::remove_dir_all(&making)?;
        }
        hamlet_git::settle(&path)?;
        if path.exists() {
            remove_locks(&path)?;
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            {
                let repo = Repository::create(&making)?;
                make(&Git { repo: &repo, at })?;
            }
            // Closed before the move: Windows renames no folder that
            // holds an open file.
            std::fs::rename(&making, &path)?;
        }
        let folder = Self { path, home };
        let held = folder.open(at)?;
        ensure!(
            held.reads()?.tip(MAIN)?.is_some(),
            "the repository {} has no main",
            folder.path.display()
        );
        Ok(folder)
    }

    /// Opens the repository for one unit. Blocks.
    pub fn open(&self, at: i64) -> Result<Held> {
        Ok(Held {
            path: self.path.clone(),
            home: self.home.clone(),
            git: Some(Reads(Repository::open(&self.path)?)),
            at,
        })
    }
}

/// Removes the `*.lock` files a kill left under a repository.
fn remove_locks(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            remove_locks(&path)?;
        } else if path.extension().is_some_and(|e| e == "lock") {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Removes everything in the folder `dir`, and leaves the folder.
fn empty(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn beside(path: &Path, what: &str) -> Result<PathBuf> {
    let mut name = path
        .file_name()
        .context("a repository's folder has a name")?
        .to_owned();
    name.push(".");
    name.push(what);
    Ok(path.with_file_name(name))
}

impl Held {
    pub fn reads(&self) -> Result<&Reads> {
        self.git.as_ref().context("the repository is gone")
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub(super) fn writes(&self) -> Result<Git<'_>> {
        Ok(Git {
            repo: &self.reads()?.0,
            at: self.at,
        })
    }

    /// The changes that make the files of `base` those of the folder
    /// `under` the realm folder, both but for the paths `skip` names.
    pub(super) fn changes(
        &self,
        under: &Path,
        base: CommitId,
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<Change>> {
        let folder = checkout::open(&self.home, under)?
            .with_context(|| {
                format!("there is no folder {}", under.display())
            })?;
        let listing = checkout::list(&folder, skip)?;
        let mut held = self
            .reads()?
            .entries(base)?
            .into_iter()
            .filter(|entry| !skip(&entry.path))
            .map(|entry| (entry.path, (entry.blob, entry.executable)))
            .collect::<BTreeMap<_, _>>();
        let mut changes = Vec::new();
        for entry in listing.files {
            let now = (entry.blob, entry.executable);
            if held.remove(&entry.path) != Some(now) {
                let bytes = checkout::read(&folder, &entry.path)?;
                let path = entry.path;
                changes.push(if entry.executable {
                    Change::WriteExecutable { path, bytes }
                } else {
                    Change::Write { path, bytes }
                });
            }
        }
        changes.extend(
            held.into_keys()
                .filter(|path| !listing.alone.contains(path))
                .map(|path| Change::Remove { path }),
        );
        Ok(changes)
    }

    /// Closes the repository before its folder is replaced, since
    /// Windows renames no folder that holds an open file, and opens
    /// the one put in its place.
    pub(super) fn purge(
        &mut self,
        file: &str,
    ) -> Result<Vec<(String, CommitId, CommitId)>> {
        self.git = None;
        let purged = hamlet_git::purge(&self.path, file);
        self.git = Some(Reads(Repository::open(&self.path)?));
        purged
    }

    /// Makes the folder `under` the realm folder hold the files of
    /// `at`, but for the paths `skip` names, making the folder when
    /// it is not there.
    pub(crate) fn check_out(
        &self,
        at: CommitId,
        under: &Path,
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<()> {
        let folder = checkout::make(&self.home, under)?;
        let reads = self.reads()?;
        let listing = checkout::list(&folder, skip)?;
        for step in reads.0.plan(at, &listing.files, skip)? {
            match step {
                Step::Write {
                    path,
                    blob,
                    executable,
                } => checkout::write(
                    &folder,
                    &path,
                    &reads.blob(blob)?,
                    executable,
                )?,
                Step::Remove { path } => {
                    checkout::remove(&folder, &path)?;
                }
            }
        }
        Ok(())
    }

    /// Removes the folder `under` the realm folder with everything in
    /// it, when it is there.
    pub(super) fn remove_folder(&self, under: &Path) -> Result<()> {
        checkout::remove_folder(&self.home, under)
    }

    /// Moves the repository's folder aside by one rename and empties
    /// it there. The empty folder stays as the mark that fails every
    /// later start, so no kill lets the repository be made again.
    /// Nothing reads it after.
    pub(super) fn erase(&mut self) -> Result<()> {
        if self.git.take().is_none() {
            bail!("the repository is gone");
        }
        let erased = beside(&self.path, ERASED)?;
        std::fs::rename(&self.path, &erased)?;
        empty(&erased)
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    fn write(path: &str, text: &str) -> Change {
        Change::Write {
            path: path.into(),
            bytes: text.into(),
        }
    }

    /// A repository whose `main` holds a character and notes, with
    /// the scratch folder as the realm folder.
    fn folder(dir: &Scratch) -> Folder {
        Folder::start(
            dir.join("repository.git"),
            dir.to_path_buf(),
            1000,
            |git| {
                let files = [
                    write("CHARACTER.md", "Careful."),
                    write("memory/notes.md", "one\ntwo\nthree\n"),
                ];
                git.commit(MAIN, None, &files, "made", By::Realm)
                    .map(drop)
            },
        )
        .unwrap()
    }

    fn text(held: &Held, at: CommitId, path: &str) -> Option<String> {
        let bytes = held.reads().unwrap().read(at, path).unwrap()?;
        Some(String::from_utf8(bytes).unwrap())
    }

    fn tip(held: &Held, branch: &str) -> CommitId {
        held.reads().unwrap().tip(branch).unwrap().unwrap()
    }

    #[test]
    fn a_commit_onto_a_moved_tip_fails_and_moves_nothing() {
        let dir = Scratch::new("repo-git-moved");
        let held = folder(&dir).open(2000).unwrap();
        let git = held.writes().unwrap();
        let was = tip(&held, MAIN);
        let moved = git
            .commit(
                MAIN,
                Some(was),
                &[write("a.md", "1")],
                "one",
                By::Chief(1),
            )
            .unwrap();
        let stale = git.commit(
            MAIN,
            Some(was),
            &[write("b.md", "2")],
            "two",
            By::Chief(1),
        );
        assert!(stale.is_err());
        assert_eq!(tip(&held, MAIN), moved);
    }

    #[test]
    fn a_save_onto_a_moved_branch_keeps_both_and_its_side_wins() {
        let dir = Scratch::new("repo-git-save");
        let folder = folder(&dir);
        let held = folder.open(2000).unwrap();
        let base = tip(&held, MAIN);
        held.writes().unwrap().branch("hires/7", base).unwrap();
        let taken = [
            write("memory/notes.md", "uno\ntwo\nthree\n"),
            write("taken.md", "from main"),
        ];
        let git = held.writes().unwrap();
        git.commit(
            "hires/7",
            Some(base),
            &taken,
            "took in",
            By::Realm,
        )
        .unwrap();
        let saved = [
            write("memory/notes.md", "ONE\ntwo\nthree\n"),
            write("saved.md", "from the agent"),
        ];
        let agent = By::Agent(AgentNumber(7));
        let tip_after =
            git.save_onto("hires/7", base, &saved, agent).unwrap();
        assert_eq!(tip(&held, "hires/7"), tip_after);
        assert_eq!(
            text(&held, tip_after, "memory/notes.md").as_deref(),
            Some("ONE\ntwo\nthree\n")
        );
        assert_eq!(
            text(&held, tip_after, "taken.md").as_deref(),
            Some("from main")
        );
        assert_eq!(
            text(&held, tip_after, "saved.md").as_deref(),
            Some("from the agent")
        );
        let reads = held.reads().unwrap();
        assert!(reads.holds("hires/7", base).unwrap());
        assert!(!reads.holds(MAIN, tip_after).unwrap());

        let repeat = folder.open(3000).unwrap();
        let again = repeat
            .writes()
            .unwrap()
            .save_onto("hires/7", base, &saved, agent);
        assert_eq!(again.unwrap(), tip_after);
        assert_eq!(tip(&repeat, "hires/7"), tip_after);
    }

    #[test]
    fn a_checkout_that_changed_nothing_commits_nothing() {
        let dir = Scratch::new("repo-git-checkout");
        let held = folder(&dir).open(2000).unwrap();
        let base = tip(&held, MAIN);
        let git = held.writes().unwrap();
        let none = git.commit_checkout(
            MAIN,
            base,
            &[],
            "dreamt",
            By::Dream(1),
            Wins::From,
        );
        assert_eq!(none.unwrap(), None);
        assert_eq!(tip(&held, MAIN), base);
    }

    #[test]
    fn made_by_finds_a_commit_of_the_branch_after_main_was_taken_in()
    {
        let dir = Scratch::new("repo-git-made-by");
        let held = folder(&dir).open(2000).unwrap();
        let base = tip(&held, MAIN);
        let git = held.writes().unwrap();
        git.branch("hires/7", base).unwrap();
        let dreamt = git
            .commit_checkout(
                "hires/7",
                base,
                &[write("dreamt.md", "x")],
                "dreamt",
                By::Dream(3),
                Wins::From,
            )
            .unwrap()
            .unwrap();
        let on_main = [write("memory/main.md", "y")];
        git.commit(
            MAIN,
            Some(base),
            &on_main,
            "dreamt too",
            By::Dream(5),
        )
        .unwrap();
        git.take_in("hires/7", tip(&held, MAIN), "took in").unwrap();

        let reads = held.reads().unwrap();
        assert_eq!(
            reads.made_by("hires/7", base, By::Dream(3)).unwrap(),
            Some(dreamt)
        );
        // Brought in by the take-in, not made on the branch.
        assert_eq!(
            reads.made_by("hires/7", base, By::Dream(5)).unwrap(),
            None
        );
    }

    #[test]
    fn a_revert_undoes_a_commit_and_keeps_later_ones() {
        let dir = Scratch::new("repo-git-revert");
        let held = folder(&dir).open(2000).unwrap();
        let git = held.writes().unwrap();
        let base = tip(&held, MAIN);
        let undone = git
            .commit(
                MAIN,
                Some(base),
                &[write("a.md", "1")],
                "a",
                By::Chief(1),
            )
            .unwrap();
        git.commit(
            MAIN,
            Some(undone),
            &[write("b.md", "2")],
            "b",
            By::Chief(1),
        )
        .unwrap();
        let reverted = git
            .revert(MAIN, undone, "undid a", By::Chief(1))
            .unwrap();
        assert_eq!(text(&held, reverted.commit, "a.md"), None);
        assert!(text(&held, reverted.commit, "b.md").is_some());
    }

    #[test]
    fn a_start_finishes_a_purge_cut_off_between_its_renames() {
        let dir = Scratch::new("repo-git-settle");
        let path = dir.join("repository.git");
        let made = tip(&folder(&dir).open(2000).unwrap(), MAIN);
        // A purge cut off after the old repository moved aside and
        // before the whole fresh one moved into its place.
        let fresh = dir.join("repository.git.purging");
        std::fs::rename(&path, &fresh).unwrap();
        std::fs::write(fresh.join("PURGED"), b"").unwrap();

        let folder =
            Folder::start(path, dir.to_path_buf(), 3000, |_| {
                anyhow::bail!("made again over a purged repository")
            })
            .unwrap();
        assert_eq!(tip(&folder.open(4000).unwrap(), MAIN), made);
        assert!(!fresh.exists());
    }

    #[test]
    fn a_start_removes_the_locks_a_kill_left() {
        let dir = Scratch::new("repo-git-locks");
        folder(&dir);
        let lock = dir.join("repository.git/refs/heads/main.lock");
        std::fs::write(&lock, b"").unwrap();
        let held = folder(&dir).open(2000).unwrap();
        assert!(!lock.exists());
        let base = tip(&held, MAIN);
        let git = held.writes().unwrap();
        git.commit(
            MAIN,
            Some(base),
            &[write("a.md", "1")],
            "a",
            By::Chief(1),
        )
        .unwrap();
    }

    #[test]
    fn a_checkout_does_not_read_a_file_too_large_to_list_as_removed()
    {
        let dir = Scratch::new("repo-checkout-large");
        let held = folder(&dir).open(2000).unwrap();
        let base = tip(&held, MAIN);
        let large = Change::Write {
            path: "large.bin".into(),
            bytes: vec![7; 5 << 20],
        };
        let at = held
            .writes()
            .unwrap()
            .commit(MAIN, Some(base), &[large], "large", By::Chief(1))
            .unwrap();
        let under = Path::new("dreams/1/main");
        held.check_out(at, under, &|_| false).unwrap();
        assert!(
            held.changes(under, at, &|_| false).unwrap().is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_checkout_keeps_whether_a_file_runs() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = Scratch::new("repo-checkout-runs");
        let held = folder(&dir).open(2000).unwrap();
        let base = tip(&held, MAIN);
        let run = Change::WriteExecutable {
            path: "run.sh".into(),
            bytes: b"echo".to_vec(),
        };
        let at = held
            .writes()
            .unwrap()
            .commit(MAIN, Some(base), &[run], "run", By::Chief(1))
            .unwrap();
        let under = Path::new("dreams/1/main");
        let file = dir.join(under).join("run.sh");
        held.check_out(at, under, &|_| false).unwrap();
        assert!(
            held.changes(under, at, &|_| false).unwrap().is_empty()
        );

        std::fs::set_permissions(
            &file,
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert_eq!(
            held.changes(under, at, &|_| false).unwrap(),
            [write("run.sh", "echo")]
        );

        held.check_out(at, under, &|_| false).unwrap();
        assert!(
            held.changes(under, at, &|_| false).unwrap().is_empty()
        );
    }

    #[test]
    fn a_checkout_holds_the_commit_but_what_it_skips() {
        let dir = Scratch::new("repo-git-folder");
        let held = folder(&dir).open(2000).unwrap();
        let base = tip(&held, MAIN);
        let under = Path::new("dreams/1/main");
        let skip = |path: &str| path == "CHARACTER.md";
        held.check_out(base, under, &skip).unwrap();
        let at = dir.join(under);
        assert!(!at.join("CHARACTER.md").exists());
        assert!(held.changes(under, base, &skip).unwrap().is_empty());

        std::fs::write(
            at.join("memory/notes.md"),
            "ONE\ntwo\nthree\n",
        )
        .unwrap();
        std::fs::write(at.join("new.md"), "new").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            dir.join("repository.git"),
            at.join("link"),
        )
        .unwrap();
        assert_eq!(
            held.changes(under, base, &skip).unwrap(),
            [
                write("memory/notes.md", "ONE\ntwo\nthree\n"),
                write("new.md", "new"),
            ]
        );
        std::fs::remove_file(at.join("memory/notes.md")).unwrap();
        assert_eq!(
            held.changes(under, base, &skip).unwrap(),
            [
                write("new.md", "new"),
                Change::Remove {
                    path: "memory/notes.md".into()
                },
            ]
        );

        held.check_out(base, under, &skip).unwrap();
        assert!(held.changes(under, base, &skip).unwrap().is_empty());
        assert!(!at.join("new.md").exists());
        held.remove_folder(Path::new("dreams/1")).unwrap();
        assert!(!dir.join("dreams/1").exists());
    }
}
