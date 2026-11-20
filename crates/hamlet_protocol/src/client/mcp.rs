//! The MCP servers the realm lists (C36): how each is, and what a
//! chief changes in the list.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::RequestId;

/// The MCP servers the realm lists, as a client shows them.
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct McpView {
    pub servers: Vec<McpServerView>,
    /// What in the list does not load, and what keeps a server's
    /// tools from being offered.
    pub faults: Vec<String>,
    /// The servers other programs on the server's machine list that
    /// the realm's list does not, as found when a chief last asked.
    pub found: Vec<McpFound>,
}

/// A server of the list, and how it is.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct McpServerView {
    pub name: String,
    pub entry: McpEntry,
    pub state: McpState,
    /// The tools it last listed, offered as they are named.
    pub tools: Vec<McpToolView>,
}

/// A server's entry in `mcp/servers.json`.
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct McpEntry {
    /// The program and its arguments.
    #[schemars(length(min = 1, max = 256))]
    pub command: Vec<String>,
    /// Its environment. The server never sends a value: each is
    /// `null` in a view, and `null` in an [`McpAct::Put`] keeps the
    /// value the entry has.
    #[schemars(length(max = 256))]
    pub env: BTreeMap<String, Option<String>>,
    /// Names copied from the environment of the server.
    #[serde(rename = "inheritEnv")]
    #[schemars(length(max = 256))]
    pub inherit_env: Vec<String>,
    /// Paths it may read, from its own folder.
    #[serde(rename = "fsRo")]
    #[schemars(length(max = 256))]
    pub fs_ro: Vec<String>,
    /// Paths it may write, from its own folder.
    #[serde(rename = "fsRw")]
    #[schemars(length(max = 256))]
    pub fs_rw: Vec<String>,
    pub net: bool,
    /// How long a call may take, when not the default.
    #[serde(rename = "timeoutSecs")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "u64", range(min = 1))]
    pub timeout_secs: Option<u64>,
    /// Tools whose calls run without asking, though the server does
    /// not say they only read; `*` for every tool.
    #[schemars(length(max = 1024))]
    pub allow: Vec<String>,
    /// Tools whose calls always wait for a chief; `*` for every
    /// tool.
    #[schemars(length(max = 1024))]
    pub ask: Vec<String>,
}

/// How a server is.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "now", deny_unknown_fields)]
pub enum McpState {
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "up")]
    Up,
    /// It stopped, and starts again at `again`, in milliseconds since
    /// the epoch.
    #[serde(rename = "down")]
    Down { reason: String, again: i64 },
}

/// A tool of a server.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct McpToolView {
    /// The name models call it by.
    pub name: String,
    /// Its name on the server, which `allow` and `ask` take.
    pub tool: String,
    pub description: String,
    /// Each call waits for a chief.
    pub acts: bool,
}

/// A server another program lists.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct McpFound {
    /// The program, as a person reads its name.
    pub source: String,
    /// Its name in the realm's list.
    pub name: String,
    /// Its entry, with no value of its environment; none when Hamlet
    /// cannot run it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "McpEntry")]
    pub entry: Option<McpEntry>,
    /// Why Hamlet cannot run it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub unrunnable: Option<String>,
}

/// A chief's change to the list of MCP servers.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Mcp {
    pub id: RequestId,
    pub act: McpAct,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "do", deny_unknown_fields)]
pub enum McpAct {
    /// Adds the server `name`, or changes its entry.
    #[serde(rename = "put")]
    Put {
        #[schemars(length(min = 1, max = 24))]
        name: String,
        entry: McpEntry,
    },
    #[serde(rename = "remove")]
    Remove {
        #[schemars(length(min = 1, max = 24))]
        name: String,
    },
    /// Stops the server and starts it again at once.
    #[serde(rename = "restart")]
    Restart {
        #[schemars(length(min = 1, max = 24))]
        name: String,
    },
    /// Looks for the servers other programs list, into
    /// [`McpView::found`].
    #[serde(rename = "find")]
    Find,
    /// Adds the found servers `names`, their values with them, or
    /// every one found that runs when it names none.
    #[serde(rename = "import")]
    Import {
        #[schemars(length(max = 256))]
        names: Vec<String>,
    },
}
