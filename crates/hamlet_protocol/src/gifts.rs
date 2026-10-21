//! Files and skills the chief gives an agent, sent in parts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Address, RequestId};

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum Give {
    Files {
        files: Vec<GiveFilesFiles>,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        to: Address,
    },
    Skill {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        skill: GiveSkillSkill,
        to: Address,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct GiveFilesFiles {
    pub path: String,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub size: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum GiveSkillSkill {
    Starter { starter: String },
    From { from: Address, name: String },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct GivePart {
    pub data: String,
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
    pub offset: u64,
    pub path: String,
    pub upload: String,
}
