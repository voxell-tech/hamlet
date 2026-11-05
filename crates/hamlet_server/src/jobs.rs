//! The effects a change keeps for after its commit.

use anyhow::Result;
use hamlet_store::ids::{AgentId, HamletId, JobId, ScholarId};
use serde::{Deserialize, Serialize};

use crate::change::Change;

/// An effect on files or repositories, kept with the change that
/// needs it and run until done.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub(crate) enum Job {
    /// Makes a scholar's repository: from its character, or from a
    /// commit of another scholar's.
    Plant {
        scholar: ScholarId,
        character: String,
        fork: Option<Fork>,
    },
    /// Starts a hire's branch when it has none, and checks out its
    /// lodge.
    Lodge { hire: AgentId },
    /// Commits a hire's lodge to its branch.
    Commit { hire: AgentId },
    /// Takes `main`, or the hire's pin, into an idle hire's branch
    /// and its lodge.
    TakeIn { hire: AgentId },
    /// Deletes an erased hire's lodge and branch.
    EraseHire { hire: AgentId },
    /// Deletes an erased scholar's repository.
    EraseScholar { scholar: ScholarId },
    /// Deletes the folder of an erased hamlet's lodges.
    EraseHamlet { hamlet: HamletId },
}

/// The commit of another scholar a fork starts from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Fork {
    pub from: ScholarId,
    pub commit: String,
}

impl Job {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Plant { .. } => "plant",
            Self::Lodge { .. } => "lodge",
            Self::Commit { .. } => "commit",
            Self::TakeIn { .. } => "take-in",
            Self::EraseHire { .. } => "erase-hire",
            Self::EraseScholar { .. } => "erase-scholar",
            Self::EraseHamlet { .. } => "erase-hamlet",
        }
    }

    /// The key two waiting jobs may not share.
    fn key(&self) -> String {
        match self {
            Self::Plant { scholar, .. }
            | Self::EraseScholar { scholar } => {
                format!("{}:{scholar}", self.kind())
            }
            Self::Lodge { hire }
            | Self::Commit { hire }
            | Self::TakeIn { hire }
            | Self::EraseHire { hire } => {
                format!("{}:{hire}", self.kind())
            }
            Self::EraseHamlet { hamlet } => {
                format!("{}:{hamlet}", self.kind())
            }
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
