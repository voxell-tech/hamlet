//! The answers to a program's queries. A query is a read, and needs
//! the grant of its read.

use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{Capability, Granted, Verdict};
use openui_lang::{ToolRequest, Value};

/// What a query came to.
pub(crate) struct Answered {
    /// A result always has `rows` and `error`, so a program reads
    /// both the same way.
    pub result: Value,
    /// What is wrong with the query.
    pub problem: Option<String>,
    /// The capability the query needed and the chief is yet to
    /// decide.
    pub wants: Option<Capability>,
}

/// The answer to `request`.
pub(crate) fn answer(
    request: &ToolRequest,
    agents: &AgentList,
    granted: &Granted,
) -> Answered {
    let (failed, wants) = match request.tool() {
        "agents" if granted.holds(Capability::ReadAgents) => {
            return Answered {
                result: rows(agents_rows(agents), Value::Null),
                problem: None,
                wants: None,
            };
        }
        "agents" => (
            format!("{} not granted", Capability::ReadAgents.name()),
            (granted.verdict(Capability::ReadAgents)
                == Some(Verdict::Ask))
            .then_some(Capability::ReadAgents),
        ),
        other => (format!("no query named `{other}`"), None),
    };
    Answered {
        result: rows(Vec::new(), Value::from(failed.as_str())),
        problem: Some(failed),
        wants,
    }
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
