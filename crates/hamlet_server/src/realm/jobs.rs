//! The effects a change keeps for after its commit.

use anyhow::Result;
use hamlet_store::Db;
use hamlet_store::ids::{
    AgentId, DreamId, HamletId, JobId, MergeId, OfferId, ScholarId,
};
use serde::{Deserialize, Serialize};

use crate::realm::change::Change;
use crate::society::dreams::Outcome;

/// Declares [`Job`] with each job's kind written once: the tag it is
/// kept under, which [`Job::kind`] gives back.
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
    /// Makes a scholar's repository: from its character, or from a
    /// commit of another scholar's.
    Plant("plant") {
        scholar: ScholarId,
        character: String,
        fork: Option<Fork>,
    },
    /// Starts a hire's branch when it has none, and checks out its
    /// lodge.
    Lodge("lodge") { hire: AgentId },
    /// Commits a hire's lodge to its branch.
    Commit("commit") { hire: AgentId },
    /// Takes `main`, or the hire's pin, into an idle hire's branch
    /// and its lodge.
    TakeIn("take-in") { hire: AgentId },
    /// Deletes an erased hire's lodge and branch.
    EraseHire("erase-hire") { hire: AgentId },
    /// Deletes an erased scholar's repository.
    EraseScholar("erase-scholar") { scholar: ScholarId },
    /// Deletes the folder of an erased hamlet's lodges.
    EraseHamlet("erase-hamlet") { hamlet: HamletId },
    /// Brings what a merged hire learned into the kept hire's branch
    /// and lodge, with its inbox.
    Merge("merge") { merge: MergeId },
    /// Undoes a merge's commit on the kept hire's branch.
    Split("split") { merge: MergeId },
    /// Puts a skill the librarian took on the shelves, and takes it
    /// out of the lodge it was offered from.
    Shelve("shelve") { offer: OfferId },
    /// Ends a dream whose turn ended as `last`: commits what it left
    /// and takes the new `main` in.
    DreamEnd("dream-end") {
        dream: DreamId,
        last: Outcome,
        summary: String,
    },
}

/// The commit of another scholar a fork starts from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Fork {
    pub from: ScholarId,
    pub commit: String,
}

impl Job {
    /// The key two waiting jobs may not share.
    pub fn key(&self) -> String {
        let of = match self {
            Self::Plant { scholar, .. }
            | Self::EraseScholar { scholar } => scholar.to_string(),
            Self::Lodge { hire }
            | Self::Commit { hire }
            | Self::TakeIn { hire }
            | Self::EraseHire { hire } => hire.to_string(),
            Self::EraseHamlet { hamlet } => hamlet.to_string(),
            Self::Merge { merge } | Self::Split { merge } => {
                merge.to_string()
            }
            Self::Shelve { offer } => offer.to_string(),
            Self::DreamEnd { dream, .. } => dream.to_string(),
        };
        format!("{}:{of}", self.kind())
    }

    /// The lodges the job, once done, changed, which a remote copy of
    /// each must be sent.
    pub fn lodges(&self, db: &mut Db) -> Vec<AgentId> {
        match self {
            Self::Lodge { hire } | Self::TakeIn { hire } => {
                vec![*hire]
            }
            Self::Merge { merge } | Self::Split { merge } => {
                crate::society::merges::get(db, *merge)
                    .map(|found| vec![found.kept, found.absorbed])
                    .unwrap_or_default()
            }
            Self::DreamEnd { dream, .. } => {
                crate::society::dreams::get(db, *dream)
                    .ok()
                    .and_then(|found| found.hire)
                    .into_iter()
                    .collect()
            }
            Self::Shelve { offer } => {
                crate::society::library::get(db, *offer)
                    .map(|found| vec![found.agent])
                    .unwrap_or_default()
            }
            // A commit keeps what the lodge holds already; the rest
            // change no living lodge.
            Self::Commit { .. }
            | Self::Plant { .. }
            | Self::EraseHire { .. }
            | Self::EraseScholar { .. }
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
