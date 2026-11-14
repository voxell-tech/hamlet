//! The enum case each text of a fixture is. The probe puts a name no
//! enum has in place of one text and reads the fixture back as its
//! type: the enum at that place fails with the list of its cases.

use std::fmt::Display;

use ciborium::Value;
use serde::de::value::{MapDeserializer, SeqDeserializer};
use serde::de::{self, DeserializeOwned, IntoDeserializer, Visitor};
use serde::forward_to_deserialize_any;

/// An enum's cases, as its `Deserialize` names them.
pub type Cases = &'static [&'static str];

const UNKNOWN: &str = "?";

/// The cases of an enum tagged by `type`.
pub fn tagged<E: DeserializeOwned>() -> Cases {
    cases_of::<E>(&Value::Map(vec![(
        Value::Text("type".into()),
        Value::Text(UNKNOWN.into()),
    )]))
}

/// The cases of an enum of bare names.
pub fn named<E: DeserializeOwned>() -> Cases {
    cases_of::<E>(&Value::Text(UNKNOWN.into()))
}

fn cases_of<E: DeserializeOwned>(probe: &Value) -> Cases {
    match E::deserialize(Node(probe)) {
        Err(Probed {
            cases: Some(cases), ..
        }) => cases,
        other => panic!(
            "{} does not read {probe:?} as an unknown case: {:?}",
            std::any::type_name::<E>(),
            other.err()
        ),
    }
}

/// Each enum case that `value`, read as a `T`, holds: the enum's
/// cases and the name of the one it holds.
pub fn found<T: DeserializeOwned>(
    value: &Value,
) -> Vec<(Cases, String)> {
    let mut paths = Vec::new();
    texts(value, &mut Vec::new(), &mut paths);
    let mut found = Vec::new();
    for path in paths {
        let mut probe = value.clone();
        let Value::Text(name) = std::mem::replace(
            at(&mut probe, &path),
            Value::Text(UNKNOWN.into()),
        ) else {
            unreachable!("a path leads to a text");
        };
        if let Err(Probed {
            cases: Some(cases), ..
        }) = T::deserialize(Node(&probe))
        {
            found.push((cases, name));
        }
    }
    found
}

/// The paths to every text that is a value, not a key.
fn texts(
    value: &Value,
    path: &mut Vec<usize>,
    paths: &mut Vec<Vec<usize>>,
) {
    let items = match value {
        Value::Text(_) => return paths.push(path.clone()),
        Value::Array(items) => items.iter().collect::<Vec<_>>(),
        Value::Map(entries) => {
            entries.iter().map(|(_, item)| item).collect()
        }
        _ => return,
    };
    for (index, item) in items.into_iter().enumerate() {
        path.push(index);
        texts(item, path, paths);
        path.pop();
    }
}

fn at<'a>(value: &'a mut Value, path: &[usize]) -> &'a mut Value {
    path.iter().fold(value, |value, &index| match value {
        Value::Array(items) => &mut items[index],
        Value::Map(entries) => &mut entries[index].1,
        _ => unreachable!("a path goes through lists and maps"),
    })
}

/// A read's failure, with the cases of the enum that met an unknown
/// one.
#[derive(Debug)]
struct Probed {
    cases: Option<Cases>,
    message: String,
}

impl de::Error for Probed {
    fn custom<T: Display>(message: T) -> Self {
        Self {
            cases: None,
            message: message.to_string(),
        }
    }

    fn unknown_variant(name: &str, cases: Cases) -> Self {
        Self {
            cases: Some(cases),
            message: format!("unknown case {name}"),
        }
    }
}

impl Display for Probed {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Probed {}

/// A fixture's value read as serde reads the JSON value model.
struct Node<'a>(&'a Value);

impl<'de> de::Deserializer<'de> for Node<'_> {
    type Error = Probed;

    fn deserialize_any<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Probed> {
        match self.0 {
            Value::Null => visitor.visit_unit(),
            Value::Bool(value) => visitor.visit_bool(*value),
            Value::Integer(value) => {
                let value = i128::from(*value);
                match u64::try_from(value) {
                    Ok(value) => visitor.visit_u64(value),
                    Err(_) => visitor.visit_i64(
                        i64::try_from(value)
                            .map_err(de::Error::custom)?,
                    ),
                }
            }
            Value::Float(value) => visitor.visit_f64(*value),
            Value::Text(text) => visitor.visit_str(text),
            Value::Array(items) => {
                let mut items =
                    SeqDeserializer::new(items.iter().map(Node));
                let value = visitor.visit_seq(&mut items)?;
                items.end()?;
                Ok(value)
            }
            Value::Map(entries) => {
                let mut entries = MapDeserializer::new(
                    entries
                        .iter()
                        .map(|(key, item)| (Node(key), Node(item))),
                );
                let value = visitor.visit_map(&mut entries)?;
                entries.end()?;
                Ok(value)
            }
            other => Err(de::Error::custom(format!(
                "a fixture holds {other:?}"
            ))),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Probed> {
        match self.0 {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Probed> {
        match self.0 {
            Value::Text(name) => visitor.visit_enum(
                IntoDeserializer::<Probed>::into_deserializer(
                    name.as_str(),
                ),
            ),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Probed> {
        visitor.visit_newtype_struct(self)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str
        string bytes byte_buf unit unit_struct seq tuple tuple_struct map
        struct identifier ignored_any
    }
}

impl IntoDeserializer<'_, Probed> for Node<'_> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}
