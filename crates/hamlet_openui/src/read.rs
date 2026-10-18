//! The answers to a program's queries. A query is a read, and needs
//! the grant of its read.

use hamlet_sdk::agents::AgentList;
use hamlet_sdk::grant::{Granted, READ_AGENTS};
use openui_lang::{ToolRequest, Value};

/// The result of `request`, and what is wrong with it. A result
/// always has `rows` and `error`, so a program reads both the same
/// way.
pub(crate) fn answer(
    request: &ToolRequest,
    agents: &AgentList,
    granted: &Granted,
) -> (Value, Option<String>) {
    let failed = match request.tool() {
        "agents" if granted.holds(READ_AGENTS) => {
            return (rows(agents_rows(agents), Value::Null), None);
        }
        "agents" => format!("{READ_AGENTS} not granted"),
        other => format!("no query named `{other}`"),
    };
    (rows(Vec::new(), Value::from(failed.as_str())), Some(failed))
}

fn rows(rows: Vec<Value>, error: Value) -> Value {
    Value::object([("rows", Value::from(rows)), ("error", error)])
}

fn agents_rows(agents: &AgentList) -> Vec<Value> {
    agents
        .0
        .iter()
        .map(|agent| {
            Value::object([
                ("name", Value::from(agent.name.as_str())),
                ("hamlet", Value::from(agent.hamlet.as_str())),
            ])
        })
        .collect()
}
