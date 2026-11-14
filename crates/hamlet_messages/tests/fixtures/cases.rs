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

pub fn tagged<E: DeserializeOwned>() -> Cases {
    cases_of::<E>(&tag_probe())
}

/// `T`'s cases when it is an enum tagged by `type`.
pub fn tagged_cases<T: DeserializeOwned>() -> Option<Cases> {
    match T::deserialize(TaggedReader(&tag_probe())) {
        Err(Probed {
            cases: Some(cases), ..
        }) => Some(cases),
        _ => None,
    }
}

fn tag_probe() -> Value {
    Value::Map(vec![(
        Value::Text("type".into()),
        Value::Text(UNKNOWN.into()),
    )])
}

pub fn named<E: DeserializeOwned>() -> Cases {
    cases_of::<E>(&Value::Text(UNKNOWN.into()))
}

fn cases_of<E: DeserializeOwned>(probe: &Value) -> Cases {
    match E::deserialize(TaggedReader(probe)) {
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

/// An untagged union of families, each tagged by `type`.
pub struct Union {
    pub name: String,
    pub families: Vec<Family>,
}

impl Union {
    /// Its name with no parameters, as serde's errors give it.
    fn outer_name(&self) -> &str {
        self.name
            .split_once('<')
            .map_or(&self.name, |(outer, _)| outer)
    }
}

/// A family of a [`Union`], and the probe of a value of it.
pub struct Family {
    pub name: String,
    pub cases: Cases,
    found: fn(&str, &Value, &[Union], &mut Found),
}

pub fn union<U>(families: Vec<Family>) -> Union {
    Union {
        name: short_name::<U>(),
        families,
    }
}

pub fn family<E: DeserializeOwned>() -> Family {
    Family {
        name: short_name::<E>(),
        cases: tagged::<E>(),
        found: found::<E>,
    }
}

/// A type's name with no module paths, as serde and a reader name it.
pub fn short_name<T>() -> String {
    let mut name = String::new();
    for letter in std::any::type_name::<T>().chars() {
        name.push(letter);
        if name.ends_with("::") {
            name.truncate(name.len() - 2);
            let kept = name
                .trim_end_matches(|letter: char| {
                    letter.is_alphanumeric() || letter == '_'
                })
                .len();
            name.truncate(kept);
        }
    }
    name
}

/// The enum cases fixtures hold, each case of a union's family also
/// where the union holds it, and what kept a case from being found.
#[derive(Default)]
pub struct Found {
    pub cases: Vec<(Cases, String)>,
    pub in_unions: Vec<InUnion>,
    pub problems: Vec<String>,
}

/// A case of a family, held where a union holds that family.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InUnion {
    pub union: String,
    pub family: String,
    pub case: String,
}

/// Adds each enum case `value` holds when read as a `T`, with that
/// enum's cases, to `found`. A case in a union is read as the family
/// that has its `type`, since the union reports only that none of
/// its families read it.
pub fn found<T: DeserializeOwned>(
    name: &str,
    value: &Value,
    unions: &[Union],
    found: &mut Found,
) {
    if let Err(error) = T::deserialize(TaggedReader(value)) {
        found.problems.push(format!(
            "{name} does not read as {}: {error}",
            short_name::<T>()
        ));
        return;
    }
    let mut paths = Vec::new();
    texts(value, &mut Vec::new(), &mut paths);
    let mut read_as_a_family = Vec::new();
    for path in paths {
        let mut probe = value.clone();
        let Value::Text(text) = std::mem::replace(
            at(&mut probe, &path),
            Value::Text(UNKNOWN.into()),
        ) else {
            unreachable!("a path leads to a text");
        };
        match T::deserialize(TaggedReader(&probe)) {
            Err(Probed {
                cases: Some(cases), ..
            }) => found.cases.push((cases, text)),
            Err(Probed {
                cases: None,
                message,
            }) if message.contains("untagged enum") => {
                let (union, position) = match union_at::<T>(
                    &message, value, &path, &text, unions,
                ) {
                    Ok(at) => at,
                    Err(problem) => {
                        found
                            .problems
                            .push(format!("{name}: {problem}"));
                        continue;
                    }
                };
                if !read_as_a_family.contains(&position) {
                    in_union(
                        name, value, &position, union, unions, found,
                    );
                    read_as_a_family.push(position);
                }
            }
            Err(_) | Ok(_) => {}
        }
    }
}

/// The union an untagged enum's error names, and its position around
/// `path`. The error leaves out a generic union's parameters, so of
/// the unions of that name it reads those `T`'s name holds, else all,
/// and takes the one with a family that has the `type` around `path`.
fn union_at<'u, T>(
    message: &str,
    value: &Value,
    path: &[usize],
    text: &str,
    unions: &'u [Union],
) -> Result<(&'u Union, Vec<usize>), String> {
    let probed = short_name::<T>();
    let (held, others) = unions
        .iter()
        .filter(|union| {
            message.ends_with(&format!(
                "untagged enum {}",
                union.outer_name()
            ))
        })
        .partition::<Vec<_>, _>(|union| probed.contains(&union.name));
    let named = if held.is_empty() { others } else { held };
    let Some(first) = named.first() else {
        return Err(format!(
            "the union of \"{message}\" is missing from unions()"
        ));
    };
    let around = named
        .iter()
        .filter_map(|union| {
            Some((*union, position(value, path, union)?))
        })
        .collect::<Vec<_>>();
    match around.as_slice() {
        [(union, position)] => Ok((union, position.clone())),
        [] => Err(format!(
            "no family of {} has a type around {text}",
            first.outer_name()
        )),
        around => Err(format!(
            "{} each have a family with the type around {text}",
            around
                .iter()
                .map(|(union, _)| union.name.as_str())
                .collect::<Vec<_>>()
                .join(" and ")
        )),
    }
}

/// The outermost map around `path` in `value` whose `type` is a case
/// of a family of `union`. A family's value can hold a map whose
/// `type` is another family's case, so the nearest map is not the
/// union's; a map above the union that shares a case is read as the
/// family and fails loudly.
fn position(
    value: &Value,
    path: &[usize],
    union: &Union,
) -> Option<Vec<usize>> {
    (0..path.len())
        .map(|end| path[..end].to_vec())
        .find(|position| {
            tag(read_at(value, position)).is_some_and(|tag| {
                union
                    .families
                    .iter()
                    .any(|family| family.cases.contains(&tag))
            })
        })
}

/// Reads the union's value at `position` as the family that has its
/// `type`.
fn in_union(
    name: &str,
    value: &Value,
    position: &[usize],
    union: &Union,
    unions: &[Union],
    found: &mut Found,
) {
    let value = read_at(value, position);
    let tag = tag(value).expect("a union's position has a type");
    let owners = union
        .families
        .iter()
        .filter(|family| family.cases.contains(&tag))
        .collect::<Vec<_>>();
    match owners.as_slice() {
        [family] => {
            found.in_unions.push(InUnion {
                union: union.name.clone(),
                family: family.name.clone(),
                case: tag.to_owned(),
            });
            (family.found)(
                &format!("{name} at {position:?}"),
                value,
                unions,
                found,
            );
        }
        owners => found.problems.push(format!(
            "{name}: the type {tag} of {} is a case of {}",
            union.name,
            owners
                .iter()
                .map(|family| family.name.as_str())
                .collect::<Vec<_>>()
                .join(" and ")
        )),
    }
}

pub fn tag(value: &Value) -> Option<&str> {
    let Value::Map(entries) = value else {
        return None;
    };
    entries.iter().find_map(|(key, item)| match (key, item) {
        (Value::Text(key), Value::Text(tag)) if key == "type" => {
            Some(tag.as_str())
        }
        _ => None,
    })
}

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

fn read_at<'a>(value: &'a Value, path: &[usize]) -> &'a Value {
    path.iter().fold(value, |value, &index| match value {
        Value::Array(items) => &items[index],
        Value::Map(entries) => &entries[index].1,
        _ => unreachable!("a path goes through lists and maps"),
    })
}

fn at<'a>(value: &'a mut Value, path: &[usize]) -> &'a mut Value {
    path.iter().fold(value, |value, &index| match value {
        Value::Array(items) => &mut items[index],
        Value::Map(entries) => &mut entries[index].1,
        _ => unreachable!("a path goes through lists and maps"),
    })
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
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

struct TaggedReader<'a>(&'a Value);

impl<'de> de::Deserializer<'de> for TaggedReader<'_> {
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
                let mut items = SeqDeserializer::new(
                    items.iter().map(TaggedReader),
                );
                let value = visitor.visit_seq(&mut items)?;
                items.end()?;
                Ok(value)
            }
            Value::Map(entries) => {
                let mut entries = MapDeserializer::new(
                    entries.iter().map(|(key, item)| {
                        (TaggedReader(key), TaggedReader(item))
                    }),
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

impl IntoDeserializer<'_, Probed> for TaggedReader<'_> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}
