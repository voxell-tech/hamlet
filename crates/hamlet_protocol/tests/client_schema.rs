//! The client protocol's shapes, recorded with its version, so a
//! change to them is seen and committed.

use std::path::Path;

use hamlet_protocol::Version;
use hamlet_protocol::client::{
    ClientMessage, ServerMessage, VERSION,
};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// The recorded fingerprint, from the repository's root.
const FINGERPRINT: &str =
    "crates/hamlet_protocol/client_schema.fingerprint";

/// The client protocol's JSON Schema, on the server's settings, in
/// sorted keys and without the doc comments' words.
fn schema() -> String {
    let mut generator = SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator();
    let client = generator.subschema_for::<ClientMessage>();
    let server = generator.subschema_for::<ServerMessage>();
    let schema = shape(json!({
        "anyOf": [client, server],
        "$defs": generator.take_definitions(true),
    }));
    let mut json = Vec::new();
    let formatter =
        serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer =
        serde_json::Serializer::with_formatter(&mut json, formatter);
    serde::Serialize::serialize(&schema, &mut serializer).unwrap();
    String::from_utf8(json).unwrap()
}

/// `value` with its keys sorted and its annotations dropped, so only
/// what a message may hold is left.
fn shape(value: Value) -> Value {
    match value {
        Value::Object(object) => sorted(
            object
                .into_iter()
                .filter(|(key, _)| {
                    key != "description" && key != "title"
                })
                .map(|(key, value)| {
                    let value = match key.as_str() {
                        "properties" | "$defs" => named(value),
                        "const" | "enum" | "default" => value,
                        _ => shape(value),
                    };
                    (key, value)
                }),
        ),
        Value::Array(items) => items.into_iter().map(shape).collect(),
        value => value,
    }
}

/// A map of names to schemas, whose keys are never annotations.
fn named(value: Value) -> Value {
    match value {
        Value::Object(object) => sorted(
            object
                .into_iter()
                .map(|(name, schema)| (name, shape(schema))),
        ),
        value => value,
    }
}

/// An object of `entries` in key order, whichever map serde_json
/// was built with.
fn sorted(entries: impl Iterator<Item = (String, Value)>) -> Value {
    let mut entries = entries.collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Value::Object(entries.into_iter().collect::<Map<_, _>>())
}

/// The fingerprint file for version `v` of the schema `schema`.
fn fingerprint(v: Version, schema: &str) -> String {
    let hash = Sha256::digest(format!("v {v}\n{schema}"));
    format!(
        "# The client protocol's version and a sha256 of its schema \
         with that\n# version, written by `cargo test -p \
         hamlet_protocol`. Do not edit.\nv {v}\nsha256 {hash:x}\n"
    )
}

#[test]
fn the_client_schema_is_recorded() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(FINGERPRINT);
    let expected = fingerprint(VERSION, &schema());
    if std::fs::read_to_string(&path).ok().as_deref()
        == Some(&expected)
    {
        return;
    }
    std::fs::write(&path, expected).unwrap();
    panic!(
        "The client protocol's shapes changed, so {FINGERPRINT} is \
         rewritten; commit it. Its version stays {VERSION} until \
         Hamlet's first release; after it, a change that breaks a \
         peer moves VERSION in crates/hamlet_protocol/src/client.rs \
         by semver."
    );
}
