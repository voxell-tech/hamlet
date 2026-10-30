//! Terminals the server runs: shells the chief and agents type into,
//! and the views of what they print.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{AgentRef, RequestId};

/// The most columns or rows a terminal takes.
pub const TERMINAL_SIZE: u16 = 1000;

/// A terminal the server runs, as the state lists it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TerminalView {
    /// The agent whose terminal it is. None when the chief opened
    /// it.
    #[serde(deserialize_with = "Option::deserialize")]
    pub agent: Option<AgentRef>,
    pub cols: u16,
    /// The folder its shell started in.
    pub cwd: String,
    /// The exit code of its shell, once the shell ended.
    #[serde(deserialize_with = "Option::deserialize")]
    pub exit: Option<i64>,
    /// The hamlet it belongs to. None for an unbounded terminal,
    /// which runs as the server's account with no sandbox.
    #[serde(deserialize_with = "Option::deserialize")]
    pub hamlet: Option<String>,
    pub id: String,
    pub kind: TerminalKind,
    pub name: String,
    pub rows: u16,
    /// Whether the sandbox holds its commands.
    pub sandboxed: bool,
    /// The title its programs last set, if any.
    pub title: String,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum TerminalKind {
    /// A shell that takes input.
    #[serde(rename = "shell")]
    Shell,
    /// The commands an agent's shell tool ran and what they printed.
    /// It takes no input.
    #[serde(rename = "runs")]
    Runs,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Terminal {
    /// Opens a shell in `hamlet`'s folder under the sandbox, or an
    /// unbounded one without a hamlet. Its first output answers `id`
    /// and names it.
    #[serde(rename = "open")]
    Open {
        #[schemars(range(min = 1, max = TERMINAL_SIZE))]
        cols: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "String")]
        hamlet: Option<String>,
        id: RequestId,
        #[schemars(range(min = 1, max = TERMINAL_SIZE))]
        rows: u16,
    },
    /// Types `data` into a terminal, as keys and pastes give it.
    #[serde(rename = "input")]
    Input {
        #[schemars(length(max = 65_536))]
        data: String,
        terminal: String,
    },
    #[serde(rename = "resize")]
    Resize {
        #[schemars(range(min = 1, max = TERMINAL_SIZE))]
        cols: u16,
        #[schemars(range(min = 1, max = TERMINAL_SIZE))]
        rows: u16,
        terminal: String,
    },
    /// Sends what a terminal shows now, then what it prints, until
    /// `unwatch`.
    #[serde(rename = "watch")]
    Watch { id: RequestId, terminal: String },
    #[serde(rename = "unwatch")]
    Unwatch { terminal: String },
    /// Ends a terminal's processes and removes it.
    #[serde(rename = "close")]
    Close { id: RequestId, terminal: String },
}

/// Bytes a watched terminal printed.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TerminalOutput {
    /// The bytes, in base64.
    pub data: String,
    /// Whether `data` redraws the terminal from a blank one: the
    /// answer to a watch, or a catch-up after output was skipped.
    pub reset: bool,
    pub terminal: String,
    /// The open or watch this answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "RequestId")]
    pub to: Option<RequestId>,
}
