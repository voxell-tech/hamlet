//! Operations a harness asks of the realm, each made once per
//! identity and kept with the marker of its outcome.

use anyhow::{Result, ensure};
use rusqlite::{
    Connection, OptionalExtension, TransactionBehavior, params,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::store::Store;

/// The persistent name of a harness runtime instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInstance(String);

impl RuntimeInstance {
    /// `name` if it is 1 to 64 lowercase ASCII letters, digits and
    /// hyphens, not starting with a hyphen.
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        // The name is also a folder of the home.
        ensure!(
            (1..=64).contains(&name.len())
                && !name.starts_with('-')
                && name.bytes().all(|b| {
                    b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || b == b'-'
                }),
            "not a runtime instance name: {name:?}"
        );
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The identity of one operation: the runtime instance that asks, and
/// the key its harness keeps the call under, such as pi-durable's
/// task ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationId {
    pub runtime: RuntimeInstance,
    pub key: String,
}

/// A change to the realm, made once per [`OperationId`]. The value is
/// the request.
pub trait Operation: Serialize {
    /// The operation's name, kept in its markers.
    const KIND: &'static str;
    type Done: Serialize + DeserializeOwned;
    type Refusal: Serialize + DeserializeOwned;

    /// The refusal of an actor without the authority to run this now.
    fn authorize(
        &self,
        db: &Connection,
    ) -> Result<Option<Self::Refusal>>;

    /// Makes the change. What it wrote is undone when it refuses.
    fn apply(
        &self,
        db: &Connection,
    ) -> Result<Outcome<Self::Done, Self::Refusal>>;
}

/// An operation's outcome, as its marker keeps it.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome<D, R> {
    Done(D),
    Refused(R),
}

/// The answer to running an operation.
#[derive(Debug, PartialEq)]
pub enum Reply<D, R> {
    /// The marked outcome, decided now or by an earlier run of the
    /// same request.
    Outcome(Outcome<D, R>),
    /// The identity is marked for another request. Nothing ran.
    Conflict,
}

impl Store {
    /// Runs `operation` as `id`. The authority check, the change and
    /// the marker commit together, and an error leaves none of
    /// them.
    pub fn run<O: Operation>(
        &mut self,
        id: &OperationId,
        operation: &O,
    ) -> Result<Reply<O::Done, O::Refusal>> {
        let digest = digest(operation)?;
        let mut tx = self.db.transaction_with_behavior(
            TransactionBehavior::Immediate,
        )?;
        let marked = tx
            .query_row(
                "SELECT digest, outcome FROM marker
                 WHERE runtime = ?1 AND key = ?2",
                params![id.runtime.as_str(), id.key],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, String>(1)?,
                    ))
                },
            )
            .optional()?;
        if let Some((kept, outcome)) = marked {
            // A marked outcome stands even when its authority has
            // since gone.
            return Ok(if kept == digest {
                Reply::Outcome(serde_json::from_str(&outcome)?)
            } else {
                Reply::Conflict
            });
        }
        // Checked here, as it runs: a harness recovering a call can
        // skip its own check, as pi-durable skips
        // `beforeTool` on replay.
        let outcome = match operation.authorize(&tx)? {
            Some(refusal) => Outcome::Refused(refusal),
            None => {
                let change = tx.savepoint()?;
                let outcome = operation.apply(&change)?;
                if let Outcome::Done(_) = outcome {
                    change.commit()?;
                }
                // Otherwise dropped, which rolls the change back.
                outcome
            }
        };
        // A refusal is marked too. The harness may already hold it as
        // the call's result, so a later run of the call must
        // not decide otherwise against a changed realm.
        tx.execute(
            "INSERT INTO marker (runtime, key, kind, digest, \
             outcome)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.runtime.as_str(),
                id.key,
                O::KIND,
                digest,
                serde_json::to_string(&outcome)?
            ],
        )?;
        tx.commit()?;
        Ok(Reply::Outcome(outcome))
    }
}

/// The SHA-256 of the operation's kind and its request in canonical
/// JSON.
fn digest<O: Operation>(operation: &O) -> Result<Vec<u8>> {
    let mut request = String::new();
    canonical(&serde_json::to_value(operation)?, &mut request);
    let mut hash = Sha256::new();
    hash.update(O::KIND);
    hash.update([0]);
    hash.update(request);
    Ok(hash.finalize().to_vec())
}

/// `value` as JSON with each object's keys sorted, so equal requests
/// have equal digests in every process.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(key, _)| *key);
            out.push('{');
            for (i, (key, value)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push(':');
                canonical(value, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}
