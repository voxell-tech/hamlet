//! The numbers of what the agent writes. Its encoder writes an
//! integer from 2^32 as a float, so each one is read as a `u32`, and
//! a whole-number float as an integer, so each float reads from one.

use std::any::TypeId;
use std::collections::BTreeSet;

use ciborium::Value;
use hamlet_actor::action::decode_positional;
use serde::de::DeserializeOwned;

pub type Reads = fn(&[u8]) -> bool;

pub fn reads_args<T: DeserializeOwned>(bytes: &[u8]) -> bool {
    decode_positional::<T>(bytes).is_ok()
}

pub fn reads_answer<T: DeserializeOwned>(bytes: &[u8]) -> bool {
    ciborium::from_reader::<T, _>(bytes).is_ok()
}

pub enum Writer {
    Agent,
    Answerer { answer: TypeId },
}

pub struct Sample {
    pub name: String,
    pub value: Value,
    pub reads: Reads,
    pub writer: Writer,
}

pub fn not_u32(
    samples: &[Sample],
    agent_answers: &BTreeSet<TypeId>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for sample in by_agent(samples, agent_answers) {
        for path in paths(&sample.value, |value| {
            matches!(value, Value::Integer(_))
        }) {
            let reads = |number: u64| {
                sample
                    .reads_with(&path, Value::Integer(number.into()))
            };
            if !reads(u32::MAX.into()) || reads(1 << 32) {
                problems.push(format!(
                    "{} at {} is not a u32",
                    sample.name,
                    dotted(&sample.value, &path)
                ));
            }
        }
    }
    problems
}

pub fn float_only(
    samples: &[Sample],
    agent_answers: &BTreeSet<TypeId>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for sample in by_agent(samples, agent_answers) {
        let floats = paths(&sample.value, |value| {
            matches!(value, Value::Float(_))
        });
        for path in floats {
            if !sample.reads_with(&path, Value::Integer(3.into())) {
                problems.push(format!(
                    "{} at {} reads no whole number written as an \
                     integer",
                    sample.name,
                    dotted(&sample.value, &path)
                ));
            }
        }
    }
    problems
}

fn by_agent<'a>(
    samples: &'a [Sample],
    agent_answers: &'a BTreeSet<TypeId>,
) -> impl Iterator<Item = &'a Sample> {
    samples.iter().filter(|sample| match sample.writer {
        Writer::Agent => true,
        Writer::Answerer { answer } => {
            agent_answers.contains(&answer)
        }
    })
}

impl Sample {
    fn reads_with(&self, path: &[usize], number: Value) -> bool {
        let mut value = self.value.clone();
        *at(&mut value, path) = number;
        let mut bytes = Vec::new();
        ciborium::into_writer(&value, &mut bytes).unwrap();
        (self.reads)(&bytes)
    }
}

type Path = Vec<usize>;

fn paths(value: &Value, number: fn(&Value) -> bool) -> Vec<Path> {
    let mut found = Vec::new();
    walk(value, number, &mut Vec::new(), &mut found);
    found
}

fn walk(
    value: &Value,
    number: fn(&Value) -> bool,
    path: &mut Path,
    found: &mut Vec<Path>,
) {
    if number(value) {
        found.push(path.clone());
        return;
    }
    let items: Box<dyn Iterator<Item = &Value>> = match value {
        Value::Array(items) => Box::new(items.iter()),
        Value::Map(entries) => {
            Box::new(entries.iter().map(|(_, v)| v))
        }
        _ => return,
    };
    for (index, item) in items.enumerate() {
        path.push(index);
        walk(item, number, path, found);
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

fn dotted(mut value: &Value, path: &[usize]) -> String {
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
