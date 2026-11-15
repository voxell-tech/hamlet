//! The integers of what the agent writes. Its encoder writes an
//! integer from 2^32 as a float, so each one is read as a `u32`: the
//! test puts `u32::MAX` and then 2^32 at each integer a fixture holds
//! and reads the fixture back as its type.

use std::any::TypeId;
use std::collections::BTreeSet;

use ciborium::Value;
use hamlet_actor::action::decode_positional;
use serde::de::DeserializeOwned;

/// Whether bytes read as a fixture's type.
pub type Reads = fn(&[u8]) -> bool;

pub fn reads_args<T: DeserializeOwned>(bytes: &[u8]) -> bool {
    decode_positional::<T>(bytes).is_ok()
}

pub fn reads_answer<T: DeserializeOwned>(bytes: &[u8]) -> bool {
    ciborium::from_reader::<T, _>(bytes).is_ok()
}

/// Who writes a fixture.
pub enum Writer {
    Agent,
    /// The one who answers a call whose answer is of this type.
    Answerer(TypeId),
}

pub struct Sample {
    pub name: String,
    pub value: Value,
    pub reads: Reads,
    pub writer: Writer,
}

/// Each integer of a sample the agent writes that does not read as a
/// `u32`, by the fixture and the path to it. `answers` are the types
/// of the answers to the calls the agent takes.
pub fn not_u32(
    samples: &[Sample],
    answers: &BTreeSet<TypeId>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for sample in
        samples.iter().filter(|sample| match sample.writer {
            Writer::Agent => true,
            Writer::Answerer(answer) => answers.contains(&answer),
        })
    {
        let mut paths = Vec::new();
        integers(&sample.value, &mut Vec::new(), &mut paths);
        for path in paths {
            let reads = |number: u64| {
                let mut value = sample.value.clone();
                *at(&mut value, &path) =
                    Value::Integer(number.into());
                let mut bytes = Vec::new();
                ciborium::into_writer(&value, &mut bytes).unwrap();
                (sample.reads)(&bytes)
            };
            if !reads(u32::MAX.into()) || reads(1 << 32) {
                problems.push(format!(
                    "{} at {} is not a u32",
                    sample.name,
                    shown(&sample.value, &path)
                ));
            }
        }
    }
    problems
}

/// The index of each step into a list or a map's entries.
type Path = Vec<usize>;

fn integers(value: &Value, path: &mut Path, found: &mut Vec<Path>) {
    let items: Box<dyn Iterator<Item = &Value>> = match value {
        Value::Integer(_) => {
            found.push(path.clone());
            return;
        }
        Value::Array(items) => Box::new(items.iter()),
        Value::Map(entries) => {
            Box::new(entries.iter().map(|(_, v)| v))
        }
        _ => return,
    };
    for (index, item) in items.enumerate() {
        path.push(index);
        integers(item, path, found);
        path.pop();
    }
}

fn at<'a>(value: &'a mut Value, path: &[usize]) -> &'a mut Value {
    path.iter().fold(value, |value, &index| match value {
        Value::Array(items) => &mut items[index],
        Value::Map(entries) => &mut entries[index].1,
        _ => unreachable!("a path steps only into lists and maps"),
    })
}

/// `path` with each map's key named, as `items.0.tool.exit`.
fn shown(mut value: &Value, path: &[usize]) -> String {
    let mut steps = Vec::new();
    for &index in path {
        value = match value {
            Value::Array(items) => {
                steps.push(index.to_string());
                &items[index]
            }
            Value::Map(entries) => {
                let (key, item) = &entries[index];
                steps.push(match key {
                    Value::Text(key) => key.clone(),
                    key => format!("{key:?}"),
                });
                item
            }
            _ => {
                unreachable!("a path steps only into lists and maps")
            }
        };
    }
    steps.join(".")
}
