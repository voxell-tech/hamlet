//! Running the action of a node: the steps that change the panel's
//! own state.

use openui_lang::{ActionPlan, ActionStep, Document, Inputs, Value};

/// Runs the steps of `plan` that write state, in order. A step of any
/// other kind is left undone and comes back as a problem.
pub(crate) fn run(
    plan: &ActionPlan,
    document: &Document,
    state: &mut Inputs,
) -> Vec<String> {
    let mut problems = Vec::new();
    for step in plan.steps() {
        match step {
            ActionStep::Set { target, value } => {
                let value = document.resolve(value, state);
                state.set_state(target.as_str(), value);
            }
            ActionStep::Reset { targets } => {
                for target in targets {
                    let value = document
                        .state_defaults()
                        .find(|(name, _)| name == target)
                        .map_or(Value::Null, |(_, value)| {
                            value.clone()
                        });
                    state.set_state(target.as_str(), value);
                }
            }
            ActionStep::Run { statement, .. } => {
                problems.push(not_run(&format!("@Run({statement})")))
            }
            ActionStep::ToAssistant { .. } => {
                problems.push(not_run("@ToAssistant"))
            }
            ActionStep::OpenUrl { .. } => {
                problems.push(not_run("@OpenUrl"))
            }
            ActionStep::Custom { name, .. } => {
                problems.push(not_run(&format!("@{name}")))
            }
            _ => problems.push(not_run("this action")),
        }
    }
    problems
}

fn not_run(what: &str) -> String {
    format!("{what} is not supported in a panel yet")
}
