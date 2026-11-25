//! The acts on two hires at once and on a hire that ended: a chief's
//! `/merge`, `/restore` and `/erase`, and a lead's `merge_hires`.

use serde::{Deserialize, Serialize};

use super::{
    AgentName, ChiefRequest, Done, HamletId, Named, SocietyTool,
    ToolCall,
};

/// Makes one hire of two: `into` takes what `absorbed` learned, its
/// roads and its hires, and `absorbed` ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Merge {
    pub absorbed: Named,
    pub into: Named,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for Merge {
    const NAME: &'static str = "chief_merge";
    type Answer = Done<MergeRefusal>;
}

impl ToolCall for Merge {
    const TOOL: SocietyTool = SocietyTool::MergeHires;
    type Answer = MergedTo<super::LeadRefusal<MergeRefusal>>;
}

answer!(
    /// The answer of a lead's [`Merge`]: the two hires by name.
    MergedTo<W> {
        Done { absorbed: AgentName, into: AgentName },
    }
);

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
pub enum MergeRefusal {
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("`{name}` is not a hire")]
    NotAHire { name: String },
    #[error("a road needs two different hires")]
    Same,
    /// `name` is the absorbed hire's.
    #[error("`{name}` is in another hamlet")]
    OtherHamlet { name: AgentName },
    #[error("a mayor cannot be ended; seat another mayor first")]
    TheMayor,
    #[error(
        "`{name}` is on a chief's task; a lead may end or merge it \
         once it has answered and its protected minutes have passed"
    )]
    OnChiefsTask { name: AgentName },
    #[error(
        "`{absorbed}` could not be merged into `{into}`, so it is \
         back"
    )]
    NotMerged {
        absorbed: AgentName,
        into: AgentName,
    },
    #[error(
        "`{absorbed}` was merged into `{into}`, but not all that \
         waited in its inbox could be moved; what was not stays \
         with `{absorbed}`"
    )]
    InboxKept {
        absorbed: AgentName,
        into: AgentName,
    },
}

/// Brings an ended hire back: one merged into another is split from
/// it again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restore {
    pub hire: Named,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for Restore {
    const NAME: &'static str = "chief_restore";
    type Answer = Restored;
}

answer!(
    /// The answer of a [`Restore`].
    Restored<EndedRefusal> {
        Back,
        /// It was merged, and what it brought is taken out again.
        Split,
    }
);

/// Erases an ended hire for good: its branch and its files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EraseHire {
    pub hire: Named,
}

impl ChiefRequest for EraseHire {
    const NAME: &'static str = "chief_erase";
    type Answer = Done<EndedRefusal>;
}

/// Why an act on an ended hire was not carried out.
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
pub enum EndedRefusal {
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("`{name}` is not a hire")]
    NotAHire { name: String },
    #[error("`{hire}` has not left")]
    NotLeft { hire: AgentName },
    #[error("the hamlet `{}` was removed; restore it first", hamlet.0)]
    HamletRemoved { hamlet: HamletId },
    /// `label` is `scholar#<id> (<name>)`.
    #[error("the scholar `{label}` no longer lives in the city")]
    ProfileGone { label: String },
    #[error("the hamlet `{}` holds its most hires, {most}", hamlet.0)]
    Full { hamlet: HamletId, most: u32 },
    #[error(
        "`{hire}` is back, but what it brought could not be taken \
         out of `{into}`"
    )]
    NotSplit { hire: AgentName, into: AgentName },
    #[error("`{hire}` could not be erased, so it stays ended")]
    NotErased { hire: AgentName },
}
