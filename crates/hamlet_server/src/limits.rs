//! Chief limits, their omitted defaults and a record's limits.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use anyhow::{Result, bail};
use hamlet_protocol::client::{
    EditLimits, HamletLimits, LimitsAsking, LimitsBudget,
    LimitsLeaving, LimitsMayor, LimitsPermissions, LimitsRoad,
    LimitsTask, Permission, VoteLimits,
};
use jsonschema::error::ValidationErrorKind;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::protocol::{Refusable, Refusal};
use crate::record::{self, Agent};

/// The limits schema, whose defaults fill what a document omits.
static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/limits.schema.json"))
        .expect("the bundled limits schema")
});

/// A hamlet's limits, as `assets/limits.schema.json` describes them.
///
/// Read it with [`Limits::of`], which fills what a document omits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub votes: VoteLimits,
    pub hamlets: HamletLimits,
    pub budget: LimitsBudget,
    pub task: LimitsTask,
    pub road: LimitsRoad,
    #[serde(rename = "mostAgents")]
    pub most_agents: i64,
    #[serde(rename = "mostRoads")]
    pub most_roads: i64,
    pub models: Vec<String>,
    pub model: Option<String>,
    /// Models by agent name; [`hamlet_protocol::MAYOR`] names the
    /// office.
    pub pinned: BTreeMap<String, String>,
    /// How hard each agent's model thinks, by agent name, over the
    /// settings' effort.
    pub efforts: BTreeMap<String, hamlet_protocol::Effort>,
    pub mayor: LimitsMayor,
    pub leaving: LimitsLeaving,
    pub asking: LimitsAsking,
    pub permissions: LimitsPermissions,
}

impl From<Limits> for hamlet_protocol::client::Limits {
    fn from(limits: Limits) -> Self {
        Self {
            votes: limits.votes,
            hamlets: limits.hamlets,
            budget: limits.budget,
            task: limits.task,
            road: limits.road,
            most_agents: limits.most_agents,
            most_roads: limits.most_roads,
            models: limits.models,
            model: limits.model,
            pinned: limits.pinned,
            efforts: limits.efforts,
            mayor: limits.mayor,
            leaving: limits.leaving,
            asking: limits.asking,
            permissions: limits.permissions,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        let mut value = json!({});
        defaults(&SCHEMA, &mut value);
        serde_json::from_value(value).expect("the schema's defaults")
    }
}

impl Limits {
    /// The limits `value` holds, with what it omits from the schema.
    pub fn of(mut value: Value) -> Result<Self> {
        defaults(&SCHEMA, &mut value);
        Ok(serde_json::from_value(value)?)
    }

    /// The most agents a hamlet holds besides its mayor.
    pub fn agent_room(&self) -> usize {
        usize::try_from(self.most_agents).unwrap_or(0)
    }

    /// The most roads between agents a mayor may build.
    pub fn road_room(&self) -> usize {
        usize::try_from(self.most_roads).unwrap_or(0)
    }

    /// The price of a road.
    pub fn road_price(&self) -> record::Amount {
        record::Amount {
            tokens: u64::try_from(self.road.tokens).unwrap_or(0),
            micros: u64::try_from(crate::spend::micros(
                self.road.cost,
            ))
            .unwrap_or(0),
            ms: u64::try_from(crate::clock::minutes(
                self.road.minutes,
            ))
            .unwrap_or(0),
        }
    }

    /// How long a vote stays open, in milliseconds.
    pub fn vote_open_ms(&self) -> i64 {
        crate::clock::minutes(self.votes.open_minutes)
    }

    /// How long a new agent is protected from removal, in
    /// milliseconds.
    pub fn protected_ms(&self) -> i64 {
        crate::clock::minutes(self.leaving.protected_minutes)
    }

    /// Whether `name`, or the mayor when `mayor`, holds `permission`
    /// by these limits.
    pub fn is_granted(
        &self,
        permission: &Permission,
        name: &str,
    ) -> bool {
        let names = match permission {
            Permission::Network => &self.permissions.network,
            Permission::Unconfined => &self.permissions.unconfined,
        };
        names.iter().any(|n| n == name)
    }

    /// How hard `agent`'s model thinks: its own effort, else `home`.
    pub fn effort_of(
        &self,
        agent: &Agent,
        mayor: bool,
        home: Option<hamlet_protocol::Effort>,
    ) -> Option<hamlet_protocol::Effort> {
        let name = if mayor {
            hamlet_protocol::MAYOR
        } else {
            &agent.name
        };
        self.efforts.get(name).copied().or(home)
    }

    /// The model `agent` runs: pinned for it, else its own choice
    /// when allowed, else the hamlet's, else `home`.
    pub fn model_of(
        &self,
        agent: &Agent,
        mayor: bool,
        home: &str,
    ) -> String {
        let name = if mayor {
            hamlet_protocol::MAYOR
        } else {
            &agent.name
        };
        self.pinned
            .get(name)
            .map(String::as_str)
            .or_else(|| {
                agent.model.as_deref().filter(|model| {
                    self.models.iter().any(|m| m == model)
                })
            })
            .or(self.model.as_deref())
            .unwrap_or(home)
            .into()
    }
}

/// The limits of `hamlet`, as its record keeps them.
pub(crate) fn of(
    db: &rusqlite::Connection,
    hamlet: &str,
) -> Result<Limits> {
    let Some(record) = record::read(db, hamlet)? else {
        bail!("There is no hamlet `{hamlet}` to read the limits of.");
    };
    Ok(*record.limits.chief)
}

/// The bundled limits schema, compiled to check a chief's edit.
pub(crate) struct LimitsSchema {
    validator: jsonschema::Validator,
}

impl LimitsSchema {
    pub fn new() -> Result<Self> {
        Ok(Self {
            validator: jsonschema::validator_for(&SCHEMA)?,
        })
    }

    /// `current` with the chief's edit, when the schema allows it.
    pub fn edit(
        &self,
        current: &Limits,
        edit: &EditLimits,
    ) -> Refusable<Value> {
        let mut value = match edit {
            EditLimits::Whole { whole, .. } => {
                Value::Object(whole.clone().into_iter().collect())
            }
            EditLimits::Set { set, .. } => {
                let mut value = serde_json::to_value(current).expect(
                    "Limits serializes: its maps have string keys",
                );
                let Some(fields) = value
                    .get_mut(&set.section)
                    .and_then(Value::as_object_mut)
                else {
                    return Err(Refusal::UnknownSection(
                        set.section.clone(),
                    ));
                };
                let reset = set.value.is_null()
                    && !(["budget", "task"]
                        .contains(&set.section.as_str())
                        && ["cost", "minutes", "tokens"]
                            .contains(&set.key.as_str()));
                if reset {
                    if fields.remove(&set.key).is_none()
                        && !["pinned", "efforts"]
                            .contains(&set.section.as_str())
                    {
                        return Err(Refusal::UnknownField {
                            field: set.key.clone(),
                            under: set.section.clone(),
                        });
                    }
                } else {
                    fields.insert(set.key.clone(), set.value.clone());
                }
                value
            }
        };
        defaults(&SCHEMA, &mut value);
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
                return Err(Refusal::UnknownField {
                    field: unexpected[0].clone(),
                    under: if path.is_empty() {
                        "limits".into()
                    } else {
                        path
                    },
                });
            }
            let schema_path = error.schema_path.to_string();
            let constraint =
                if path == "pinned" || path.starts_with("pinned.") {
                    path = "pinned".into();
                    "expected model names by agent name"
                } else if path == "efforts"
                    || path.starts_with("efforts.")
                {
                    path = "efforts".into();
                    "expected off, minimal, low, medium or high by \
                     agent name"
                } else if schema_path.contains("/items") {
                    path = path
                        .rsplit_once('.')
                        .map_or(path.clone(), |(p, _)| p.into());
                    "expected a list of names"
                } else if path.ends_with(".cost") {
                    "a cost is a number of zero or more"
                } else if SCHEMA
                    .pointer(&format!(
                        "/properties/{}",
                        path.replace('.', "/properties/")
                    ))
                    .is_some_and(|s| s["type"] == "array")
                {
                    "expected a list of names"
                } else if path.is_empty()
                    || SCHEMA
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
