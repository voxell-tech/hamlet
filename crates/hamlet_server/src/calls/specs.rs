//! The tools the server offers every runtime besides its own, and
//! whether a conversation is offered a tool.

use std::collections::BTreeMap;

use anyhow::Result;
use hamlet_protocol::runtime::{
    ConfigureTools, ToolSpec, ToolSpecReplay,
};
use serde_json::{Value, json};

use super::society;
use crate::conversations::{Conversation, questions};
use crate::tools::builtin::Outcome;

/// The tools the server adds to every runtime's own: `ask_chief` and
/// the society's.
pub(crate) fn specs() -> Vec<ToolSpec> {
    let mut specs = vec![spec(
        "ask_chief",
        "Put a question to the chief that you cannot go on without. \
         Waits for the answer, so use it only when no agent and no \
         file can tell you. Use object to argue against an \
         instruction.",
        json!({
            "type": "object",
            "properties": {
                "message": {"type": "string"},
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": questions::MOST_QUESTIONS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {"type": "string"},
                            "description": {"type": "string"},
                            "options": {
                                "type": "array",
                                "maxItems": questions::MOST_OPTIONS,
                                "items": {"anyOf": [
                                    {"type": "string"},
                                    {
                                        "type": "object",
                                        "properties": {
                                            "title": {"type": "string"},
                                            "description": {"type": "string"}
                                        },
                                        "required": ["title"]
                                    }
                                ]}
                            },
                            "text_input": {
                                "type": "boolean",
                                "description": "Allow custom words. Defaults to true."
                            }
                        },
                        "required": ["question"]
                    }
                }
            },
            "required": ["questions"]
        }),
        ToolSpecReplay::Safe,
    )];
    specs.extend(society::args::specs());
    specs
}

/// A tool's spec from its JSON Schema.
fn spec(
    name: &str,
    description: &str,
    parameters: Value,
    replay: ToolSpecReplay,
) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: description.into(),
        parameters: match parameters {
            Value::Object(map) => map.into_iter().collect(),
            _ => BTreeMap::new(),
        },
        replay,
        retain: None,
    }
}

/// Whether `conversation` is offered `tool`.
pub(super) fn is_offered(
    conversation: &Conversation,
    tool: &str,
) -> Result<bool> {
    Ok(match conversation.tools()? {
        None => true,
        Some(ConfigureTools::Only { only }) => {
            only.iter().any(|name| name == tool)
        }
        Some(ConfigureTools::Remove { remove }) => {
            !remove.iter().any(|name| name == tool)
        }
    })
}

/// The result of a call to a tool its agent is not offered.
pub(super) fn unavailable(tool: &str) -> Outcome {
    Outcome::thrown(
        "tool_unavailable",
        format!("The tool {tool} is not yours to use."),
    )
}
