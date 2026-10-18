//! The answers to a program's queries. A query is a read, and needs
//! the grant of its read.

use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{Capability, Granted, Verdict};
use hamlet_sdk::papers::Papers;
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
    papers: &Papers,
    granted: &Granted,
) -> Answered {
    let (needs, rows_of): (_, fn(&AgentList, &Papers) -> Vec<Value>) =
        match request.tool() {
            "agents" => (Capability::ReadAgents, agents_rows),
            "papers" => (Capability::ReadPapers, papers_rows),
            other => {
                return refused(
                    format!("no query named `{other}`"),
                    None,
                );
            }
        };
    if granted.holds(needs) {
        return Answered {
            result: rows(rows_of(agents, papers), Value::Null),
            problem: None,
            wants: None,
        };
    }
    refused(
        format!("{} not granted", needs.name()),
        (granted.verdict(needs) == Some(Verdict::Ask))
            .then_some(needs),
    )
}

fn refused(failed: String, wants: Option<Capability>) -> Answered {
    Answered {
        result: rows(Vec::new(), Value::from(failed.as_str())),
        problem: Some(failed),
        wants,
    }
}

fn rows(rows: Vec<Value>, error: Value) -> Value {
    Value::object([("rows", Value::from(rows)), ("error", error)])
}

fn agents_rows(agents: &AgentList, _: &Papers) -> Vec<Value> {
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

/// One row, of the selected agent's notes, character and the names
/// of its skills.
fn papers_rows(_: &AgentList, papers: &Papers) -> Vec<Value> {
    let skills = papers
        .skills
        .iter()
        .map(|(name, _)| Value::from(name.as_str()))
        .collect::<Vec<_>>();
    vec![Value::object([
        ("notes", Value::from(papers.notes.as_str())),
        ("character", Value::from(papers.character.as_str())),
        ("skills", Value::from(skills)),
    ])]
}
