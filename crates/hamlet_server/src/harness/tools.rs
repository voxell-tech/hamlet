//! The tools a runtime offers its conversations, run in this server
//! as operations.

use std::collections::BTreeMap;

use anyhow::Result;
use hamlet_runtime_protocol::{ToolSpec, ToolSpecReplay};
use serde::Deserialize;
use serde_json::{Value, json};

use super::bindings::Binding;
use crate::record::{
    Actor, Audience, Change, RecordOperation, RecordResult,
};
use crate::{OperationId, Outcome, Reply, Store};

/// Every tool runtimes are given, each safe to run again under its
/// marker.
pub fn specs() -> Vec<ToolSpec> {
    vec![ToolSpec {
        name: "build_road".into(),
        description: "Build a road between two agents, charged to \
                      the hamlet."
            .into(),
        parameters: object(json!({
            "type": "object",
            "properties": {
                "a": {"type": "string"},
                "b": {"type": "string"},
                "reason": {"type": "string"},
            },
            "required": ["a", "b", "reason"],
            "additionalProperties": false,
        })),
        replay: ToolSpecReplay::Safe,
    }]
}

fn object(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

/// A tool call as this server reads it.
#[derive(Debug)]
pub(crate) enum Call {
    BuildRoad(BuildRoad),
}

/// Who acts comes from the binding alone, so no argument names one.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BuildRoad {
    a: String,
    b: String,
    reason: String,
}

/// The call of `tool` with `arguments`, or the words for the agent of
/// why it is no call.
pub(crate) fn parse(
    tool: &str,
    arguments: &BTreeMap<String, Value>,
) -> std::result::Result<Call, String> {
    let value = Value::Object(
        arguments
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    );
    let invalid = |error: serde_json::Error| {
        format!("The arguments of {tool} are not valid: {error}.")
    };
    match tool {
        "build_road" => serde_json::from_value(value)
            .map(Call::BuildRoad)
            .map_err(invalid),
        _ => Err(format!("There is no tool named {tool}.")),
    }
}

/// The text for the agent of running `call` as `id` for the agent
/// `binding` is for, and whether it failed.
pub(crate) fn run(
    store: &mut Store,
    id: &OperationId,
    binding: &Binding,
    call: &Call,
    at: i64,
) -> Result<(String, bool)> {
    let actor = actor(store, binding)?;
    match call {
        Call::BuildRoad(road) => {
            let operation = RecordOperation {
                hamlet: binding.agent.hamlet.clone(),
                actor,
                at,
                reason: road.reason.clone(),
                task: None,
                change: Change::LayRoad {
                    a: road.a.clone(),
                    b: road.b.clone(),
                },
            };
            Ok(match store.run(id, &operation)? {
                Reply::Outcome(Outcome::Done(
                    RecordResult::Road(_),
                )) => (
                    format!(
                        "Built a road between `{}` and `{}`.",
                        road.a, road.b
                    ),
                    false,
                ),
                Reply::Outcome(Outcome::Done(other)) => {
                    anyhow::bail!("a road built as {other:?}")
                }
                Reply::Outcome(Outcome::Refused(refusal)) => {
                    (refusal.text(Audience::Agent), true)
                }
                Reply::Conflict => (CONFLICT.into(), true),
            })
        }
    }
}

/// The words for a call whose identity was used by another call.
pub(crate) const CONFLICT: &str = "This call's identity was already \
                                   used for another call, so it did \
                                   not run.";

/// The author of a change made in the conversation of `binding`: the
/// mayor when its agent holds the office now, else a member.
fn actor(store: &Store, binding: &Binding) -> Result<Actor> {
    let place = binding.agent.clone();
    let mayor = store
        .record(&place.hamlet)?
        .is_some_and(|record| record.mayor == place.number);
    Ok(if mayor {
        Actor::Mayor(place)
    } else {
        Actor::Member(place)
    })
}
