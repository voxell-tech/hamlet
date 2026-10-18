//! Chief limits, omitted defaults and the record's constraints.

use anyhow::Result;
use hamlet_protocol::v0::{EditLimits, Limits};
use jsonschema::error::ValidationErrorKind;
use serde_json::{Value, json};

use crate::protocol::Refusal;
use crate::record;

pub(crate) struct Rules {
    schema: Value,
    validator: jsonschema::Validator,
}

impl Rules {
    pub fn new() -> Result<Self> {
        let schema = serde_json::from_str::<Value>(include_str!(
            "../assets/limits.schema.json"
        ))?;
        let validator = jsonschema::validator_for(&schema)?;
        Ok(Self { schema, validator })
    }

    pub fn defaults(&self) -> Value {
        let mut value = json!({});
        defaults(&self.schema, &mut value);
        value
    }

    pub fn edit(
        &self,
        current: Value,
        edit: &EditLimits,
    ) -> std::result::Result<Value, Refusal> {
        let mut value = match edit {
            EditLimits::Whole { whole, .. } => {
                serde_json::to_value(whole).expect("JSON object")
            }
            EditLimits::Set { set, .. } => {
                let mut value = current;
                let Some(fields) = value
                    .get_mut(&set.section)
                    .and_then(Value::as_object_mut)
                else {
                    return Err(Refusal::Invalid(format!(
                        "unknown section `{}`",
                        set.section
                    )));
                };
                let reset = set.value.is_null()
                    && !(["budget", "task"]
                        .contains(&set.section.as_str())
                        && ["cost", "minutes", "tokens"]
                            .contains(&set.key.as_str()));
                if reset {
                    if fields.remove(&set.key).is_none()
                        && set.section != "pinned"
                    {
                        return Err(Refusal::Invalid(format!(
                            "unknown field `{}` under {}",
                            set.key, set.section
                        )));
                    }
                } else {
                    fields.insert(set.key.clone(), set.value.clone());
                }
                value
            }
        };
        defaults(&self.schema, &mut value);
        let errors =
            self.validator.iter_errors(&value).collect::<Vec<_>>();
        if let Some(error) = errors
            .iter()
            .find(|e| {
                matches!(
                    e.kind,
                    ValidationErrorKind::AdditionalProperties { .. }
                )
            })
            .or_else(|| errors.first())
        {
            let mut path = error
                .instance_path
                .to_string()
                .trim_start_matches('/')
                .replace('/', ".");
            if let ValidationErrorKind::AdditionalProperties {
                unexpected,
            } = &error.kind
            {
                return Err(Refusal::Invalid(format!(
                    "unknown field `{}` under {}",
                    unexpected[0],
                    if path.is_empty() { "limits" } else { &path }
                )));
            }
            let schema_path = error.schema_path.to_string();
            let constraint =
                if path == "pinned" || path.starts_with("pinned.") {
                    path = "pinned".into();
                    "expected model names by agent name"
                } else if schema_path.contains("/items") {
                    path = path
                        .rsplit_once('.')
                        .map_or(path.clone(), |(p, _)| p.into());
                    "expected a list of names"
                } else if path.ends_with(".cost") {
                    "a cost is a number of zero or more"
                } else if self
                    .schema
                    .pointer(&format!(
                        "/properties/{}",
                        path.replace('.', "/properties/")
                    ))
                    .is_some_and(|s| s["type"] == "array")
                {
                    "expected a list of names"
                } else if path.is_empty()
                    || self
                        .schema
                        .pointer(&format!("/properties/{path}"))
                        .is_some_and(|s| s["type"] == "object")
                {
                    "expected a section"
                } else {
                    "wrong type or value"
                };
            return Err(Refusal::Invalid(format!(
                "{}: {constraint}",
                if path.is_empty() { "limits" } else { &path }
            )));
        }
        if value["votes"]["passOf"].as_u64() == Some(0) {
            return Err(Refusal::Invalid(
                "votes.passOf: must be at least 1".into(),
            ));
        }
        if value["votes"]["passYes"].as_u64()
            > value["votes"]["passOf"].as_u64()
        {
            return Err(Refusal::Invalid(
                "votes.passYes: cannot exceed votes.passOf".into(),
            ));
        }
        Ok(value)
    }
}

fn defaults(schema: &Value, value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        for (key, field) in
            schema["properties"].as_object().into_iter().flatten()
        {
            if !object.contains_key(key)
                && let Some(default) = field.get("default")
            {
                object.insert(key.clone(), default.clone());
            }
            if let Some(value) = object.get_mut(key) {
                defaults(field, value);
            }
        }
    }
}

pub(crate) fn record_limits(
    value: &Limits,
    previous: &record::Limits,
) -> record::Limits {
    record::Limits {
        most_agents: value.most_agents as usize,
        most_roads: value.most_roads as usize,
        models: value.models.clone(),
        road: record::Amount {
            tokens: value.road.tokens as u64,
            micros: (value.road.cost * 1_000_000.0).round() as u64,
            ms: (value.road.minutes as u64).saturating_mul(60_000),
        },
        vote_open_ms: value
            .votes
            .as_ref()
            .map_or(previous.vote_open_ms, |v| {
                (v.open_minutes as u64).saturating_mul(60_000)
            }),
        protected_ms: (value.leaving.protected_minutes as u64)
            .saturating_mul(60_000),
        road_budget: previous.road_budget.clone(),
    }
}
