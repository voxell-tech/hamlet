//! Strict protocol v0 parsing and the chief's error envelopes.

use anyhow::{Context, Result};
use hamlet_protocol::v0::{
    ClientMessage, Fault, Reply, ReplyKind, ServerMessage,
};
use jsonschema::error::{TypeKind, ValidationErrorKind};
use serde_json::Value;

/// A request refused before any effect.
#[derive(Debug)]
pub enum Refusal {
    Invalid(String),
    Unsupported(&'static str),
}

impl Refusal {
    pub fn text(&self) -> String {
        match self {
            Self::Invalid(text) => text.clone(),
            Self::Unsupported(part) => {
                format!("This server does not yet support {part}.")
            }
        }
    }
}

pub(crate) fn reply(
    id: &str,
    result: std::result::Result<String, Refusal>,
) -> ServerMessage {
    let (kind, text) = match result {
        Ok(text) => (ReplyKind::Notice, text),
        Err(refusal) => (ReplyKind::Error, refusal.text()),
    };
    ServerMessage::Reply(Reply {
        to: id.into(),
        kind,
        text,
        upload: None,
    })
}

pub(crate) fn fault(
    to: Option<&str>,
    text: impl Into<String>,
) -> ServerMessage {
    ServerMessage::Fault(Fault {
        to: to.map(String::from),
        text: text.into(),
    })
}

struct Form {
    schema: Value,
    validator: jsonschema::Validator,
}

/// The published schema's client forms, including fields serde would
/// ignore.
pub struct Protocol {
    forms: Vec<Form>,
}

impl Protocol {
    pub fn new() -> Result<Self> {
        let schema = serde_json::from_str::<Value>(include_str!(
            "../../hamlet_protocol/schema.json"
        ))?;
        let definitions = &schema["$defs"];
        let mut forms = Vec::new();
        for form in definitions["ClientMessage"]["anyOf"]
            .as_array()
            .context("client forms")?
        {
            let form = resolve(form, &schema);
            for form in form["anyOf"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(std::slice::from_ref(form))
            {
                let mut complete = form.clone();
                complete["$defs"] = definitions.clone();
                let validator = jsonschema::options()
                    .should_validate_formats(true)
                    .build(&complete)?;
                forms.push(Form {
                    schema: form.clone(),
                    validator,
                });
            }
        }
        Ok(Self { forms })
    }

    pub fn parse(
        &self,
        text: &str,
    ) -> std::result::Result<ClientMessage, Fault> {
        let value = serde_json::from_str::<Value>(text)
            .unwrap_or(Value::Null);
        let Some(kind) = value.get("type").and_then(Value::as_str)
        else {
            return Err(Fault {
                to: None,
                text: "A message is a JSON object with a `type`."
                    .into(),
            });
        };
        let to = value["id"]
            .as_str()
            .filter(|id| valid_id(id))
            .map(String::from);
        let fail = |text| {
            Err(Fault {
                to: to.clone(),
                text,
            })
        };
        let forms = self
            .forms
            .iter()
            .filter(|f| {
                f.schema["properties"]["type"]["const"] == kind
            })
            .collect::<Vec<_>>();
        if forms.is_empty() {
            return fail(format!("There is no `{kind}` message."));
        }
        if forms.iter().any(|f| f.validator.is_valid(&value)) {
            let kind = kind.to_owned();
            let mut value = value;
            whole_numbers(&mut value);
            return serde_json::from_value(value).map_err(|_| {
                Fault {
                    to,
                    text: format!(
                        "This `{kind}` message does not fit \
                         protocol v0: a number is outside the \
                         supported range."
                    ),
                }
            });
        }
        let form = forms
            .iter()
            .min_by_key(|f| {
                let properties = &f.schema["properties"];
                (
                    strays(&value, &f.schema).len(),
                    value.as_object().map_or(0, |v| {
                        v.keys()
                            .filter(|k| {
                                properties.get(k.as_str()).is_none()
                            })
                            .count()
                    }),
                )
            })
            .expect("nonempty forms");
        let words = if let Some(key) =
            strays(&value, &form.schema).first()
        {
            let mut choices = Vec::new();
            for form in &forms {
                for choice in fixed(&form.schema["properties"][key]) {
                    if !choices.contains(&choice) {
                        choices.push(choice);
                    }
                }
            }
            format!(
                "`{key}` must be {}{}",
                if choices.len() == 1 { "" } else { "one of " },
                quoted(&choices)
            )
        } else {
            form.validator
                .iter_errors(&value)
                .next()
                .map(|e| words(&e))
                .unwrap_or_else(|| "it is not valid".into())
        };
        fail(format!(
            "This `{kind}` message does not fit protocol v0: \
             {words}."
        ))
    }
}

// JSON Schema integers include 1.0 and 1e0; serde's integer fields do
// not.
fn whole_numbers(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            if let Some(n) = number.as_f64()
                && n.fract() == 0.0
                && n >= i64::MIN as f64
                && n < i64::MAX as f64
            {
                *value = Value::from(n as i64);
            }
        }
        Value::Array(values) => {
            values.iter_mut().for_each(whole_numbers)
        }
        Value::Object(values) => {
            values.values_mut().for_each(whole_numbers)
        }
        _ => {}
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 36 && uuid::Uuid::try_parse(id).is_ok()
}

fn resolve<'a>(form: &'a Value, schema: &'a Value) -> &'a Value {
    form["$ref"]
        .as_str()
        .and_then(|r| schema.pointer(r.trim_start_matches('#')))
        .unwrap_or(form)
}

fn fixed(schema: &Value) -> Vec<Value> {
    if let Some(value) = schema.get("const") {
        vec![value.clone()]
    } else {
        schema["enum"].as_array().cloned().unwrap_or_default()
    }
}

fn strays(value: &Value, schema: &Value) -> Vec<String> {
    schema["properties"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, field)| {
            let fixed = fixed(field);
            (!fixed.is_empty() && !fixed.contains(&value[key]))
                .then(|| key.clone())
        })
        .collect()
}

fn quoted(values: &[Value]) -> String {
    values
        .iter()
        .map(|v| {
            format!(
                "`{}`",
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn words(error: &jsonschema::ValidationError<'_>) -> String {
    let path = error.instance_path.to_string();
    let at = if path.is_empty() {
        "it".into()
    } else {
        format!("`{}`", path[1..].replace('/', "."))
    };
    match &error.kind {
        ValidationErrorKind::Required { property } => format!(
            "{at} lacks {}",
            quoted(std::slice::from_ref(property))
        ),
        ValidationErrorKind::AdditionalProperties { unexpected } => {
            format!(
                "{at} has no field {}",
                unexpected
                    .iter()
                    .map(|k| format!("`{k}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        ValidationErrorKind::Format { format }
            if format == "uuid" =>
        {
            format!("{at} must be a UUID")
        }
        ValidationErrorKind::Type {
            kind: TypeKind::Single(kind),
        } => format!("{at} must be {kind}"),
        ValidationErrorKind::AnyOf { context } => context
            .iter()
            .flatten()
            .next()
            .map(words)
            .unwrap_or_else(|| "it is not valid".into()),
        _ => format!("{at} is not valid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_spelling_does_not_change_the_message() {
        let protocol = Protocol::new().unwrap();
        assert!(matches!(
            protocol
                .parse(r#"{"type":"hello","v":0.0,"token":"test"}"#),
            Ok(ClientMessage::Hello(_))
        ));
        let id = "00000000-0000-4000-8000-000000000001";
        let text = format!(
            r#"{{"type":"edit","doc":"settings","id":"{id}","set":{{"reflectEvery":2e0}}}}"#
        );
        assert!(matches!(
            protocol.parse(&text),
            Ok(ClientMessage::Edit(_))
        ));
        let invalid = text.replace("2e0", "2.5");
        let fault = protocol.parse(&invalid).unwrap_err();
        assert_eq!(fault.to.as_deref(), Some(id));
        assert_eq!(
            fault.text,
            "This `edit` message does not fit protocol v0: \
             `set.reflectEvery` must be integer."
        );
    }

    #[test]
    fn malformed_fields_are_correlated_only_to_a_valid_id() {
        let protocol = Protocol::new().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        for (fields, words) in [
            (
                r#""doc":"character","agent":{"hamlet":"home","agent":"mayor"},"text":7"#,
                "`text` must be string",
            ),
            (
                r#""doc":"limits","hamlet":"home","set":{"section":"budget","key":"cost"}"#,
                "`set` lacks `value`",
            ),
            (
                r#""doc":"limits","hamlet":"home","whole":12"#,
                "`whole` must be object",
            ),
            (
                r#""doc":"settings","set":{"model":null}"#,
                "`set.model` must be string",
            ),
        ] {
            let message =
                format!(r#"{{"type":"edit","id":"{id}",{fields}}}"#);
            let fault = protocol.parse(&message).unwrap_err();
            assert_eq!(fault.to.as_deref(), Some(id));
            assert_eq!(
                fault.text,
                format!(
                    "This `edit` message does not fit protocol v0: \
                     {words}."
                )
            );
            assert_eq!(
                protocol
                    .parse(&message.replace(id, "not-an-id"))
                    .unwrap_err()
                    .to,
                None
            );
        }
    }
}
