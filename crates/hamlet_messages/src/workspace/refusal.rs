//! The reasons the workspace does not carry out a call, by family.
//!
//! A family holds the cases the same actions can send, each written
//! once with its words. An answer holds a union of the families its
//! action can send and of no other. A union lists families, never
//! another union, so a reader switches on one `type`.

use std::fmt::Display;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::files::Hashed;
use super::reach::{Need, needs_words};
use super::{CallId, CommandId, Lapse, ReachVersion};

/// A call that carries an id, which the workspace does not keep.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallRefusal {
    #[error(
        "Invalid call id: {:?} is not 1 to 128 of A-Z, a-z, 0-9, \
         '.', '_', ':' and '-'",
        call.0
    )]
    #[serde(rename = "invalid_call_id")]
    InvalidId { call: CallId },
    /// A call that waits for a chief would keep one of the realm's
    /// secrets in its text, which is never stored.
    #[error(
        "the call holds one of the realm's secrets, so it cannot \
         wait for a chief"
    )]
    HoldsSecret,
}

/// A `Run` whose program the workspace does not start.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InvalidProgram {
    #[error("Invalid command: longer than 1 MiB")]
    ScriptTooLong,
    #[error("Invalid timeout: must be a positive number of seconds")]
    ZeroTimeout,
    #[error("Invalid arguments: longer than 1 MiB")]
    ArgumentsTooLong,
}

/// A tool added by hand that does not run.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type")]
pub enum ToolRefusal {
    #[error("Tool \"{name}\" not found")]
    #[serde(rename = "tool_not_found")]
    NotFound { name: String },
    /// The arguments are not a JSON object its manifest's schema
    /// takes.
    #[error("Validation failed for tool {name}")]
    #[serde(rename = "tool_arguments")]
    Arguments { name: String },
}

/// A file call's path that is not written as the workspace takes it.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InvalidPath {
    #[error("{} is not an absolute path", path.display())]
    NotAbsolute { path: PathBuf },
}

/// A path the gate does not let a file call touch. `A` is the access
/// the call asks for.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PathRefusal<A: Default + Display> {
    #[error("{} is out of this agent's reach to {access}", path.display())]
    OutsideReach { path: PathBuf, access: A },
    /// Only the agent itself changes its filesystem, and only a
    /// worker reads its starter's through the workspace.
    #[error("{} is out of this agent's reach to {}", path.display(), A::default())]
    AgentFilesystem { path: PathBuf },
}

/// The access of a read, a search and a find.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToRead {
    #[default]
    Read,
}

/// The access of a write and an edit.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToChange {
    #[default]
    Change,
}

impl Display for ToRead {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str("read")
    }
}

impl Display for ToChange {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str("change")
    }
}

/// An agent the gate lets nothing through for, whatever the call.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReachRefusal {
    #[error("this agent has no reach on this machine")]
    NoReach,
    #[error(
        "the folder this agent works on, {}, is in the realm's \
         folder, where no agent's commands or file tools run",
        folder.display()
    )]
    ProjectInRealm { folder: PathBuf },
}

/// A `WriteFile` whose content the workspace does not take.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InvalidContent {
    /// The content is past [`super::files::MOST_CONTENT`].
    #[error(
        "the content is {bytes} bytes, more than the {limit} bytes \
         a write may hold"
    )]
    TooLarge { bytes: u64, limit: u64 },
}

/// A `CommandStatus` the workspace has no answer for.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StatusRefusal {
    #[error("this agent has no command {}", command.0)]
    NoSuchCommand { command: CommandId },
}

/// The end of a call's wait for a chief with no allow (H11, H34).
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WaitEnd {
    #[error("a chief refused the call{}", reason.as_ref().map_or_else(String::new, |reason| format!(": {reason}")))]
    ChiefRefused {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[error("the call's wait for a chief lapsed: {}", match lapse {
        Lapse::Stopped => "the turn was stopped",
        Lapse::Rewound => "the conversation was taken back past it",
        Lapse::Ended => "the agent ended",
    })]
    Lapsed { lapse: Lapse },
}

/// A chief allowed the call, but the reach changed while it waited
/// and no longer lets it run (S18). `G` is what the gate refuses the
/// call with.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReachChanged<G> {
    #[error(
        "a chief allowed the call under reach version {}, but under \
         version {} it is refused: {why}",
        since.0,
        now.0
    )]
    ReachRefuses {
        since: ReachVersion,
        now: ReachVersion,
        why: G,
    },
    /// Under the reach in force it needs what the chief was not
    /// asked for.
    #[error(
        "a chief allowed the call under reach version {}, but under \
         version {} it needs {}, which the chief was not asked for",
        since.0,
        now.0,
        needs_words(needs)
    )]
    ReachNeeds {
        since: ReachVersion,
        now: ReachVersion,
        needs: Vec<Need>,
    },
}

/// A change not made because its file no longer held what it held
/// when the change was asked for, or when a chief was shown it.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Stale {
    #[error(
        "{} changed after the change was asked for: it held {was} and \
         now holds {now}. Read it again and ask again",
        path.display()
    )]
    FileChanged {
        path: PathBuf,
        was: Hashed,
        now: Hashed,
    },
}

/// The refusal of a fresh `Run`.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum RunRefusal {
    #[error(transparent)]
    Call(#[from] CallRefusal),
    #[error(transparent)]
    Program(#[from] InvalidProgram),
    #[error(transparent)]
    Tool(#[from] ToolRefusal),
    #[error(transparent)]
    Reach(#[from] ReachRefusal),
}

/// The gate's refusal of an allowed change under a newer reach: the
/// `why` of its [`ReachChanged`].
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum FileGate {
    #[error(transparent)]
    Path(#[from] PathRefusal<ToChange>),
    #[error(transparent)]
    Reach(#[from] ReachRefusal),
}

/// The refusal of a `ReadFile`, a `Search` or a `Find`.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum ReadRefusal {
    #[error(transparent)]
    Invalid(#[from] InvalidPath),
    #[error(transparent)]
    Path(#[from] PathRefusal<ToRead>),
    #[error(transparent)]
    Reach(#[from] ReachRefusal),
}

/// The refusal of a fresh `EditFile`.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum ChangeRefusal {
    #[error(transparent)]
    Call(#[from] CallRefusal),
    #[error(transparent)]
    Invalid(#[from] InvalidPath),
    #[error(transparent)]
    Path(#[from] PathRefusal<ToChange>),
    #[error(transparent)]
    Reach(#[from] ReachRefusal),
}

/// The refusal of a fresh `WriteFile`.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum WriteRefusal {
    #[error(transparent)]
    Call(#[from] CallRefusal),
    #[error(transparent)]
    Invalid(#[from] InvalidPath),
    #[error(transparent)]
    Path(#[from] PathRefusal<ToChange>),
    #[error(transparent)]
    Reach(#[from] ReachRefusal),
    #[error(transparent)]
    Content(#[from] InvalidContent),
}

/// The reason a `Run` the workspace kept did not run.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum RunNotRun {
    #[error(transparent)]
    Wait(#[from] WaitEnd),
    #[error(transparent)]
    Reach(#[from] ReachChanged<ReachRefusal>),
    #[error(transparent)]
    Tool(#[from] ToolRefusal),
}

/// The reason a change the workspace kept was not made.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum ChangeNotRun {
    #[error(transparent)]
    Wait(#[from] WaitEnd),
    #[error(transparent)]
    Reach(#[from] ReachChanged<FileGate>),
    #[error(transparent)]
    Stale(#[from] Stale),
}
