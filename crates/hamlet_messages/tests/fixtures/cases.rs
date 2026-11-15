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

pub fn reads<T: DeserializeOwned>(value: &Value) -> bool {
    T::deserialize(TaggedReader(value)).is_ok()
}

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

pub struct Union {
    pub name: String,
    pub families: Vec<Family>,
}

pub struct Family {
    pub name: String,
    pub cases: Cases,
    found: fn(&str, &Value, &[Union], &mut Found),
}

impl Union {
    /// Whether serde's `message` names this union. It leaves out a
    /// generic union's parameters.
    fn fails_with(&self, message: &str) -> bool {
        let outer = self
            .name
            .split_once('<')
            .map_or(self.name.as_str(), |(outer, _)| outer);
        message.ends_with(&format!("untagged enum {outer}"))
    }
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

#[derive(Default)]
pub struct Found {
    pub cases: Vec<(Cases, String)>,
    pub in_unions: Vec<InUnion>,
    pub problems: Vec<String>,
}

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
        let Value::Text(text) = read_at(value, &path) else {
            unreachable!("a path leads to a text");
        };
        match probed::<T>(value, &path) {
            Err(Probed {
                cases: Some(cases), ..
            }) => found.cases.push((cases, text.clone())),
            Err(Probed {
                cases: None,
                message,
            }) if message.contains("untagged enum") => {
                let Some(union) = union_of::<T>(unions, &message)
                else {
                    found.problems.push(format!(
                        "{name}: the union of \"{message}\" is \
                         missing from unions(), or {} names none of \
                         its instances",
                        short_name::<T>()
                    ));
                    continue;
                };
                let Some(position) =
                    family_map::<T>(value, &path, union)
                else {
                    found.problems.push(format!(
                        "{name}: no family of {} has a type around \
                         {text}",
                        union.name
                    ));
                    continue;
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

/// The union serde's `message` names.
fn union_of<'u, T>(
    unions: &'u [Union],
    message: &str,
) -> Option<&'u Union> {
    let named = unions
        .iter()
        .filter(|union| union.fails_with(message))
        .collect::<Vec<_>>();
    match named.as_slice() {
        [union] => Some(union),
        _ => {
            let probed = short_name::<T>();
            named
                .into_iter()
                .find(|union| probed.contains(&union.name))
        }
    }
}

/// The map around `path` whose own `type`, put out of reach, is what
/// `union` fails to read.
fn family_map<T: DeserializeOwned>(
    value: &Value,
    path: &[usize],
    union: &Union,
) -> Option<Vec<usize>> {
    (0..path.len())
        .map(|end| path[..end].to_vec())
        .find(|position| {
            let Value::Map(entries) = read_at(value, position) else {
                return false;
            };
            let Some(index) = entries
                .iter()
                .position(|(key, _)| key.as_text() == Some("type"))
            else {
                return false;
            };
            let tag = [position.as_slice(), &[index]].concat();
            match probed::<T>(value, &tag) {
                Err(Probed {
                    cases: None,
                    message,
                }) => union.fails_with(&message),
                _ => false,
            }
        })
}

fn probed<T: DeserializeOwned>(
    value: &Value,
    path: &[usize],
) -> Result<T, Probed> {
    let mut probe = value.clone();
    *at(&mut probe, path) = Value::Text(UNKNOWN.into());
    T::deserialize(TaggedReader(&probe))
}

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
    if let Value::Text(_) = value {
        return paths.push(path.clone());
    }
    for (index, item) in items(value).into_iter().enumerate() {
        path.push(index);
        texts(item, path, paths);
        path.pop();
    }
}

/// Each map and list in `value`, `value` first when it is one.
pub fn maps_and_lists(value: &Value) -> Vec<&Value> {
    if !matches!(value, Value::Array(_) | Value::Map(_)) {
        return Vec::new();
    }
    let mut found = vec![value];
    found.extend(items(value).into_iter().flat_map(maps_and_lists));
    found
}

fn items(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        Value::Map(entries) => {
            entries.iter().map(|(_, item)| item).collect()
        }
        _ => Vec::new(),
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

/// A family's case can hold a map whose `type` is one of the family's
/// cases, and a map outside the union can hold the union under a
/// `type` that is one too. Only the map whose own `type` the union
/// fails to read is the family's.
#[test]
#[expect(dead_code, reason = "the test only reads these types")]
fn a_union_is_read_at_the_map_whose_type_it_fails_on() {
    #[derive(serde::Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Outside {
        Wrap { refused: Option<Union> },
    }
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Union {
        Family(Family),
    }
    #[derive(serde::Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Family {
        Wrap { inner: Inner },
    }
    #[derive(serde::Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Inner {
        Wrap,
    }
    let wrap = |entries: Vec<(&str, Value)>| {
        let mut map = vec![(
            Value::Text("type".into()),
            Value::Text("wrap".into()),
        )];
        map.extend(
            entries
                .into_iter()
                .map(|(key, item)| (Value::Text(key.into()), item)),
        );
        Value::Map(map)
    };
    let value =
        wrap(vec![("refused", wrap(vec![("inner", wrap(vec![]))]))]);
    let unions = [union::<Union>(vec![family::<Family>()])];
    let mut found = Found::default();

    self::found::<Outside>("wrap", &value, &unions, &mut found);

    assert_eq!(found.problems, Vec::<String>::new());
    assert_eq!(
        found
            .in_unions
            .iter()
            .map(|place| (
                place.union.as_str(),
                place.family.as_str(),
                place.case.as_str()
            ))
            .collect::<Vec<_>>(),
        [("Union", "Family", "wrap")]
    );
}
