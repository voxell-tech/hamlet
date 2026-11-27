//! Merges of one commit into another, and reverts.

use anyhow::{Context, Result};
use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::merge::tree::TreatAsUnresolved;

use crate::{
    Author, CommitId, Join, Joined, Merged, Repository, Wins,
};

impl Repository {
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
        let merged = match self.join(ours, from, Wins::Onto)? {
            Join::Held(merged) => merged,
            Join::Tree { tree, clashes } => self.commit_merge(
                ours, from, tree, clashes, message, by,
            )?,
        };
        if merged.joined != Joined::UpToDate {
            self.set_branch(branch, merged.commit, ours)?;
        }
        Ok(merged)
    }

    /// Merges the commit `from` into the commit `onto`, moving no
    /// branch. Where both changed the same lines, the side `wins`
    /// names keeps its lines and the path is a clash. The answer is
    /// `onto` when it holds `from` already or the merge leaves its
    /// files as they were, `from` when it holds `onto`, and otherwise
    /// a new commit whose first parent is `onto`.
    pub fn merge_commits(
        &self,
        onto: CommitId,
        from: CommitId,
        wins: Wins,
        message: &str,
        by: Author<'_>,
    ) -> Result<Merged> {
        match self.join(onto, from, wins)? {
            Join::Held(merged) => Ok(merged),
            Join::Tree { tree, clashes }
                if tree == self.tree_of(Some(onto))? =>
            {
                Ok(Merged {
                    commit: onto,
                    joined: Joined::UpToDate,
                    clashes,
                })
            }
            Join::Tree { tree, clashes } => self
                .commit_merge(onto, from, tree, clashes, message, by),
        }
    }

    fn join(
        &self,
        onto: CommitId,
        from: CommitId,
        wins: Wins,
    ) -> Result<Join> {
        let base = self.merge_base(onto, from)?;
        if base == Some(from) {
            return Ok(Join::Held(Merged {
                commit: onto,
                joined: Joined::UpToDate,
                clashes: Vec::new(),
            }));
        }
        if base == Some(onto) {
            return Ok(Join::Held(Merged {
                commit: from,
                joined: Joined::FastForward,
                clashes: Vec::new(),
            }));
        }
        let (ours, theirs) = match wins {
            Wins::Onto => (onto, from),
            Wins::From => (from, onto),
        };
        let options =
            gix::merge::commit::Options::from(self.ours_win()?);
        let mut outcome = self.0.merge_commits(
            ours.0,
            theirs.0,
            Default::default(),
            options,
        )?;
        Ok(Join::Tree {
            clashes: clashes(&outcome.tree_merge.conflicts),
            tree: outcome.tree_merge.tree.write()?.detach(),
        })
    }

    fn commit_merge(
        &self,
        onto: CommitId,
        from: CommitId,
        tree: ObjectId,
        clashes: Vec<String>,
        message: &str,
        by: Author<'_>,
    ) -> Result<Merged> {
        Ok(Merged {
            commit: self.write_commit(
                tree,
                [onto, from],
                message,
                by,
            )?,
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
