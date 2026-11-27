//! A chief's requests about a hamlet: its name, its site, its
//! folder and mode, its record of decisions, and its removal, return
//! and erasure; and the Hand's `remove_hamlet`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{
    AgentRefusal, ChiefRequest, Done, HamletId, HamletRefusal,
    ProfileId, Refusable, SocietyTool, Tier, ToolCall,
};
use crate::profile::VersionName;
use crate::worded;
use crate::workspace::AgentNumber;
use crate::workspace::reach::Mode;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameHamlet {
    pub hamlet: HamletId,
    pub name: String,
}

impl ChiefRequest for NameHamlet {
    const NAME: &'static str = "chief_name_hamlet";
    type Answer = Done<HamletRefusal>;
}

/// Sets a hamlet down at a spot from the city's middle, in metres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoveHamlet {
    pub hamlet: HamletId,
    pub x: f64,
    pub z: f64,
}

impl ChiefRequest for MoveHamlet {
    const NAME: &'static str = "chief_move_hamlet";
    type Answer = Done<HamletRefusal>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetMode {
    pub hamlet: HamletId,
    pub mode: Mode,
}

impl ChiefRequest for SetMode {
    const NAME: &'static str = "chief_set_mode";
    type Answer = Done<HamletRefusal>;
}

/// Sets the folder a hamlet works on; absent leaves it none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetFolder {
    pub hamlet: HamletId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

impl ChiefRequest for SetFolder {
    const NAME: &'static str = "chief_set_folder";
    type Answer = Done<HamletRefusal>;
}

/// Removes a hamlet and ends its hires.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveHamlet {
    pub hamlet: HamletId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for RemoveHamlet {
    const NAME: &'static str = "chief_remove_hamlet";
    type Answer = Done<HamletRefusal>;
}

/// Only the Hand calls it, and a chief allows it first.
impl ToolCall for RemoveHamlet {
    const TOOL: SocietyTool = SocietyTool::RemoveHamlet;
    type Answer = Done<AgentRefusal<HamletRefusal>>;
}

/// Brings a removed hamlet back with the hires its removal ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreHamlet {
    pub hamlet: HamletId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for RestoreHamlet {
    const NAME: &'static str = "chief_restore_hamlet";
    type Answer = Done<HamletRefusal>;
}

/// Erases a removed hamlet for good, with its hires.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EraseHamlet {
    pub hamlet: HamletId,
}

impl ChiefRequest for EraseHamlet {
    const NAME: &'static str = "chief_erase_hamlet";
    type Answer = Done<HamletRefusal>;
}

/// Reads a hamlet's record of decisions, oldest first, or the
/// realm's own when `hamlet` is absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hamlet: Option<HamletId>,
}

impl ChiefRequest for ReadRecord {
    const NAME: &'static str = "chief_read_record";
    type Answer = Record;
}

/// Only the Hand calls it.
impl ToolCall for ReadRecord {
    const TOOL: SocietyTool = SocietyTool::ReadRecord;
    type Answer = Record;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Record {
    Read {
        lines: Vec<RecordLine>,
    },
    #[serde(serialize_with = "crate::worded")]
    Refused {
        why: HamletRefusal,
    },
}

impl Refusable for Record {
    type Why = HamletRefusal;
    fn refused(why: HamletRefusal) -> Self {
        Self::Refused { why }
    }
}

/// One decision in words: who did what to whom, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordLine {
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub name: String,
    /// The mayor's profile. Absent makes a blank one for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mayor: Option<ProfileId>,
    /// The mayor's version of its profile; absent, `main`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionName>,
    pub tier: Tier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

impl ChiefRequest for Found {
    const NAME: &'static str = "chief_found";
    type Answer = FoundAnswer<FoundRefusal>;
}

/// Only the Hand calls it.
impl ToolCall for Found {
    const TOOL: SocietyTool = SocietyTool::Found;
    type Answer = FoundAnswer<FoundRefusal>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    bound(
        serialize = "W: Serialize + std::fmt::Display",
        deserialize = "W: Deserialize<'de>"
    )
)]
pub enum FoundAnswer<W> {
    /// The mayor is still being hired.
    Founded {
        hamlet: HamletId,
        mayor: AgentNumber,
    },
    #[serde(serialize_with = "worded")]
    Refused { why: W },
}

impl<W> Refusable for FoundAnswer<W> {
    type Why = W;
    fn refused(why: W) -> Self {
        Self::Refused { why }
    }
}

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
pub enum FoundRefusal {
    #[error(
        "`{name}` is not a name: use one line of 1 to 40 characters \
         with no colon"
    )]
    BadName { name: String },
    #[error("no profile has the address `profile#{}`", profile.0)]
    NoSuchProfile { profile: ProfileId },
    #[error("say what it is for")]
    NoPurpose,
    /// `label` is `profile#<id> (<name>)`.
    #[error("`{label}` has no version `{}`", version.0)]
    NoVersion { label: String, version: VersionName },
    /// `label` is `profile#<id> (<name>)`.
    #[error(
        "`{label}` did not answer in time to show its version `{}`; \
         try again",
        version.0
    )]
    Unanswered { label: String, version: VersionName },
}
