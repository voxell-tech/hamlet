//! Giving the agents a tool, and answering the agent that calls it.
//! It needs the capability `agent.tool`.

use crate::bindings::exports::hamlet::plugin::plugin::{
    CallAnswer, CommandCall, Intent,
};
use crate::bindings::hamlet::plugin::host::{self, FieldKind};

/// The sender of a call made by an agent.
pub const AGENT: &str = "agent";

/// One argument of a tool.
pub struct Field<'a> {
    name: &'a str,
    kind: FieldKind,
    description: &'a str,
    required: bool,
}

/// A text argument.
pub fn text<'a>(
    name: &'a str,
    description: &'a str,
    required: bool,
) -> Field<'a> {
    Field {
        name,
        kind: FieldKind::Text,
        description,
        required,
    }
}

/// A number argument.
pub fn number<'a>(
    name: &'a str,
    description: &'a str,
    required: bool,
) -> Field<'a> {
    Field {
        name,
        kind: FieldKind::Number,
        description,
        required,
    }
}

/// A yes or no argument.
pub fn flag<'a>(
    name: &'a str,
    description: &'a str,
    required: bool,
) -> Field<'a> {
    Field {
        name,
        kind: FieldKind::Flag,
        description,
        required,
    }
}

/// Gives the agents a tool called `name`. A call of it reaches the
/// plugin's `step` as a `Command` of that name from [`AGENT`], which
/// [`args`] reads and [`reply`] answers. It needs the capability
/// `agent.tool`. Only `entry` may call it.
pub fn declare(name: &str, description: &str, fields: &[Field]) {
    let fields = fields
        .iter()
        .map(|field| host::ToolField {
            name: field.name.into(),
            kind: field.kind,
            description: field.description.into(),
            required: field.required,
        })
        .collect::<Vec<_>>();
    host::agent_tool(name, description, &fields);
}

/// The arguments of a call, as the agent gave them, checked by the
/// host against the fields of the tool.
pub struct Args(Vec<(String, String)>);

impl Args {
    /// The text of the field `name`, if the agent gave it.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    /// The number in the field `name`, if the agent gave it.
    pub fn number(&self, name: &str) -> Option<f64> {
        self.text(name)?.parse().ok()
    }

    /// The yes or no in the field `name`, if the agent gave it.
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.text(name)?.parse().ok()
    }
}

/// Whether the call is an agent's.
pub fn from_agent(call: &CommandCall) -> bool {
    call.sender == AGENT
}

/// The arguments of the call of a tool.
pub fn args(call: &CommandCall) -> Result<Args, String> {
    postcard::from_bytes(&call.args)
        .map(Args)
        .map_err(|e| e.to_string())
}

/// The answer to the call of a tool: the text the agent is shown,
/// marked as the plugin's, or why there is none. It is to be returned
/// from `step`.
pub fn reply(
    call: &CommandCall,
    result: Result<impl AsRef<str>, impl Into<String>>,
) -> Intent {
    Intent::Reply(CallAnswer {
        call_id: call.call_id,
        outcome: result
            .map(|text| text.as_ref().as_bytes().to_vec())
            .map_err(Into::into),
    })
}
