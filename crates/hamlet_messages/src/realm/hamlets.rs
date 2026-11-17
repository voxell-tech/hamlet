//! A chief's requests about a hamlet: its name, its site, its
//! folder and mode, and its record of decisions.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{
    ChiefRequest, Done, HamletId, HamletRefusal, SocietyTool,
    ToolCall,
};
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

impl super::Refusable for Record {
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
