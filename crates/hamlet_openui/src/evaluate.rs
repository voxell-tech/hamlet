//! A program, evaluated with the panel's state and reads into the
//! tree it draws.

use std::collections::HashMap;

use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::Granted;
use hamlet_sdk::tree::node::{Kind, Node};
use openui_lang::{Document, Inputs};

use crate::limits::PanelLimits;
use crate::wire::Wire;
use crate::{convert, read};

/// The id of the line that tells what went wrong.
pub const PROBLEM: &str = "problem";

/// The id of the node that holds what the program drew.
pub const CONTENT: &str = "content";

/// What a program draws.
pub struct Evaluated {
    /// A column of the first problem, if any, and the content.
    pub root: Node,
    pub wires: HashMap<String, Wire>,
    /// Every problem, the parser's first.
    pub problems: Vec<String>,
}

/// The program `document` with `state`, answering its queries from
/// `agents` as far as `granted` allows. `raised` are problems from
/// before the draw, which are shown with it.
pub fn evaluate(
    document: &Document,
    state: &Inputs,
    agents: &AgentList,
    granted: &Granted,
    limits: &PanelLimits,
    raised: &[String],
) -> Evaluated {
    let mut problems = document
        .diagnostics()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut evaluation = document.evaluate(state);
    if !evaluation.queries().is_empty() {
        let mut inputs = state.clone();
        for request in evaluation.queries() {
            let (result, problem) =
                read::answer(request, agents, granted);
            inputs.set_query_result(request.statement(), result);
            problems.extend(problem);
        }
        evaluation = document.evaluate(&inputs);
    }
    problems.extend(
        evaluation.diagnostics().iter().map(ToString::to_string),
    );
    problems.extend(raised.iter().cloned());
    let (content, wires) = match evaluation.root() {
        Some(root) => {
            let converted = convert::tree(root, limits);
            problems.extend(converted.problems);
            (converted.root, converted.wires)
        }
        None => (Node::new(Kind::Column), HashMap::new()),
    };
    let mut children = Vec::new();
    if let Some(first) = problems.first() {
        children
            .push(Node::new(Kind::Note(first.clone())).id(PROBLEM));
    }
    children.push(content.id(CONTENT));
    Evaluated {
        root: Node::new(Kind::Column).of(children),
        wires,
        problems,
    }
}
