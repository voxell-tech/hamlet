//! The effects a change keeps for after its commit.

use anyhow::Result;
use hamlet_store::Db;
use hamlet_store::ids::{
    AgentId, DreamId, HamletId, JobId, MergeId, OfferId, ProfileId,
};
use serde::{Deserialize, Serialize};

use crate::realm::change::Change;
use crate::society::dreams::Outcome;

/// Declares [`Job`] with the tag each kind is kept under.
macro_rules! jobs {
    ($(
        $(#[$doc:meta])*
        $name:ident($kind:literal) { $($field:ident: $ty:ty),* $(,)? },
    )*) => {
        /// An effect on files or repositories, kept with the change
        /// that needs it and run until done.
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(tag = "kind")]
        pub(crate) enum Job {
            $(
                $(#[$doc])*
                #[serde(rename = $kind)]
                $name { $($field: $ty),* },
            )*
        }

        impl Job {
            /// The tag the job is kept under.
            pub fn kind(&self) -> &'static str {
                match self {
                    $(Self::$name { .. } => $kind,)*
                }
            }
        }
    };
}

jobs! {
    /// Makes a profile's repository: from its character, or from a
    /// commit of another profile's.
    Plant("plant") {
        profile: ProfileId,
        character: String,
        fork: Option<Fork>,
    },
    /// Starts an agent's branch when it has none, and checks out its
    /// worktree.
    Worktree("worktree") { agent: AgentId },
    /// Commits an agent's worktree to its branch.
    Commit("commit") { agent: AgentId },
    /// Takes `main`, or the agent's pin, into an idle agent's branch
    /// and its worktree.
    TakeIn("take-in") { agent: AgentId },
    /// Deletes an erased agent's worktree and branch.
    EraseAgent("erase-agent") { agent: AgentId },
    /// Deletes an erased profile's repository.
    EraseProfile("erase-profile") { profile: ProfileId },
    /// Deletes the folder of an erased hamlet's worktrees.
    EraseHamlet("erase-hamlet") { hamlet: HamletId },
    /// Brings what a merged agent learned into the kept agent's
    /// branch and worktree, with its inbox.
    Merge("merge") { merge: MergeId },
    /// Undoes a merge's commit on the kept agent's branch.
    Split("split") { merge: MergeId },
    /// Merges an agent's branch into its profile's `main`, and takes
    /// the new `main` in.
    BringHome("bring-home") { agent: AgentId },
    /// Puts a skill the librarian took on the shelves, and takes it
    /// out of the worktree it was offered from.
    Shelve("shelve") { offer: OfferId },
    /// Ends a dream whose turn ended as `last`: commits what it left
    /// and takes the new `main` in.
    DreamEnd("dream-end") {
        dream: DreamId,
        last: Outcome,
        summary: String,
    },
}

/// The commit of another profile a fork starts from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Fork {
    pub from: ProfileId,
    pub commit: String,
}

impl Job {
    /// The key two waiting jobs may not share.
    pub fn key(&self) -> String {
        let of = match self {
            Self::Plant { profile, .. }
            | Self::EraseProfile { profile } => profile.to_string(),
            Self::Worktree { agent }
            | Self::Commit { agent }
            | Self::TakeIn { agent }
            | Self::BringHome { agent }
            | Self::EraseAgent { agent } => agent.to_string(),
            Self::EraseHamlet { hamlet } => hamlet.to_string(),
            Self::Merge { merge } | Self::Split { merge } => {
                merge.to_string()
            }
            Self::Shelve { offer } => offer.to_string(),
            Self::DreamEnd { dream, .. } => dream.to_string(),
        };
        format!("{}:{of}", self.kind())
    }

    /// The worktrees the job, once done, changed, which a remote copy
    /// of each must be sent.
    pub fn worktrees(&self, db: &mut Db) -> Vec<AgentId> {
        match self {
            Self::Worktree { agent }
            | Self::TakeIn { agent }
            | Self::BringHome { agent } => vec![*agent],
            Self::Merge { merge } | Self::Split { merge } => {
                crate::society::merges::get(db, *merge)
                    .map(|found| vec![found.kept, found.absorbed])
                    .unwrap_or_default()
            }
            Self::DreamEnd { dream, .. } => {
                crate::society::dreams::get(db, *dream)
                    .ok()
                    .and_then(|found| found.agent)
                    .into_iter()
                    .collect()
            }
            Self::Shelve { offer } => {
                crate::society::library::get(db, *offer)
                    .map(|found| vec![found.agent])
                    .unwrap_or_default()
            }
            // A commit keeps what the worktree holds already; the
            // rest change no living worktree.
            Self::Commit { .. }
            | Self::Plant { .. }
            | Self::EraseAgent { .. }
            | Self::EraseProfile { .. }
            | Self::EraseHamlet { .. } => Vec::new(),
        }
    }
}

/// Keeps `job` with the change.
pub(crate) fn add(cx: &mut Change<'_>, job: &Job) -> Result<JobId> {
    cx.touched.jobs = true;
    hamlet_store::job::add(
        cx.db,
        job.kind(),
        Some(&job.key()),
        job,
        cx.at,
    )
}
