//! Roads between agents: laid, agreed, taken up and asked for.

use serde::{Deserialize, Serialize};

use super::{
    AgentName, AgentRefusal, ChiefRequest, Done, HamletId,
    LeadRefusal, Left, Named, Refusable, SocietyTool, ToolCall,
};
use crate::worded;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayRoad {
    pub a: Named,
    pub b: Named,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for LayRoad {
    const NAME: &'static str = "chief_lay_road";
    type Answer = Done<LayRefusal>;
}

impl ToolCall for LayRoad {
    const TOOL: SocietyTool = SocietyTool::LayRoad;
    type Answer = RoadDone<LeadRefusal<LayRefusal>>;
}

answer!(
    /// The answer of an act on the road between `a` and `b`.
    RoadDone<W> {
        Done { a: AgentName, b: AgentName },
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
pub enum LayRefusal {
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("a road needs two different hires")]
    SameEnds,
    #[error("a road joins hires only")]
    NotHires,
    #[error("`{hire}` is in another hamlet")]
    OtherHamlet { hire: AgentName },
    #[error("`{hire}` has left: {left}")]
    Left { hire: AgentName, left: Left },
    #[error("`{a}` and `{b}` already have a road")]
    RoadExists { a: AgentName, b: AgentName },
    #[error("the hamlet `{}` has its most roads, {most}", hamlet.0)]
    Full { hamlet: HamletId, most: u32 },
}

/// A mayor's half of a road between a hire of its hamlet and a hire
/// of another. The road is laid once the other hamlet's mayor agrees
/// to it too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgreeRoad {
    pub mine: Named,
    pub theirs: Named,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ToolCall for AgreeRoad {
    const TOOL: SocietyTool = SocietyTool::AgreeRoad;
    type Answer = Agreed<AgentRefusal<AgreeRefusal>>;
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
pub enum Agreed<W> {
    /// The other mayor has not agreed yet.
    Waiting {
        mine: AgentName,
        theirs: AgentName,
    },
    Laid {
        mine: AgentName,
        theirs: AgentName,
    },
    #[serde(serialize_with = "worded")]
    Refused {
        why: W,
    },
}

impl<W> Refusable for Agreed<W> {
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
pub enum AgreeRefusal {
    #[error("only a mayor may do that")]
    NotAMayor,
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("the chief allows no roads between hamlets")]
    NoRoadsBetween,
    #[error("a road joins hires only")]
    NotHires,
    #[error(
        "both ends of an agreed road must be in different hamlets"
    )]
    SameHamlet,
    #[error("`{a}` and `{b}` already have a road")]
    RoadExists { a: AgentName, b: AgentName },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeUpRoad {
    pub a: Named,
    pub b: Named,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for TakeUpRoad {
    const NAME: &'static str = "chief_take_up_road";
    type Answer = Done<TakeUpRefusal>;
}

impl ToolCall for TakeUpRoad {
    const TOOL: SocietyTool = SocietyTool::TakeUpRoad;
    type Answer = RoadDone<LeadRefusal<TakeUpRefusal>>;
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
pub enum TakeUpRefusal {
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("`{a}` and `{b}` have no road to take up")]
    NoRoad { a: AgentName, b: AgentName },
}

/// Asks the caller's lead for a road to `to`, as a letter to the
/// lead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestRoad {
    pub to: Named,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ToolCall for RequestRoad {
    const TOOL: SocietyTool = SocietyTool::RequestRoad;
    type Answer = Done<AgentRefusal<RequestRoadRefusal>>;
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
pub enum RequestRoadRefusal {
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("a road joins hires only")]
    NoLead,
    #[error("your lead `{lead}` is still being hired")]
    StillHiring { lead: AgentName },
}
