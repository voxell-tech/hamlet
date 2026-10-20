//! Operations a harness asks of the realm, each made once per
//! identity and kept with the marker of its outcome.

use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::store::Store;

/// The persistent name of a harness runtime instance.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

    /// The persistent runtime name used in markers and harness
    /// folders.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The identity of one operation: the runtime instance that asks, and
/// the key its harness keeps the call under, such as pi-durable's
/// task ID.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OperationId {
    pub runtime: RuntimeInstance,
    pub key: String,
}

/// A change to the realm, made once per [`OperationId`]. The value is
/// the request.
pub trait Operation: Serialize {
    /// The operation's name, kept in its markers.
    const KIND: &'static str;
    /// The completed change's result, retained for replay.
    type Done: Serialize + DeserializeOwned;
    /// The reason the change was refused, retained for replay.
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

/// A request as its marker keeps it: its kind and the SHA-256 of the
/// kind and the request in canonical JSON.
pub(crate) struct Request {
    kind: &'static str,
    digest: Vec<u8>,
}

impl Request {
    pub(crate) fn new(
        kind: &'static str,
        request: &impl Serialize,
    ) -> Result<Self> {
        let mut canonical = String::new();
        self::canonical(
            &serde_json::to_value(request)?,
            &mut canonical,
        );
        let mut hash = Sha256::new();
        hash.update(kind);
        hash.update([0]);
        hash.update(canonical);
        Ok(Self {
            kind,
            digest: hash.finalize().to_vec(),
        })
    }

    /// The tool call of `tool` with `arguments`.
    pub(crate) fn tool(
        tool: &str,
        arguments: &impl Serialize,
    ) -> Result<Self> {
        Self::new("tool", &(tool, arguments))
    }
}

/// What the marker of an identity keeps for a request of its kind.
#[derive(Debug, PartialEq)]
pub(crate) enum Marked<T> {
    /// The request began and its outcome is not kept yet.
    Begun,
    Outcome(T),
    /// The identity is marked for another request.
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
        let request = Request::new(O::KIND, operation)?;
        let tx = self.immediate()?;
        #[cfg(feature = "faults")]
        if crate::fault::split_marker() {
            // Broken on purpose: the test of this order must catch
            // the result it loses.
            if let Some(kept) = kept(&tx, id)? {
                return Ok(match reply_of(kept, &request)? {
                    Marked::Outcome(outcome) => {
                        Reply::Outcome(outcome)
                    }
                    Marked::Begun | Marked::Conflict => {
                        Reply::Conflict
                    }
                });
            }
            let outcome = change(&tx, operation)?;
            crate::fault::point("before_commit");
            tx.commit()?;
            crate::fault::point("after_commit");
            mark(self.connection(), id, &request, &outcome)?;
            return Ok(Reply::Outcome(outcome));
        }
        // A refusal is marked too. The harness may already hold it as
        // the call's result, so a later run of the call must not
        // decide otherwise against a changed realm.
        let outcome =
            once(&tx, id, &request, |db| change(db, operation))?;
        crate::fault::point("before_commit");
        tx.commit()?;
        crate::fault::point("after_commit");
        Ok(outcome.map_or(Reply::Conflict, Reply::Outcome))
    }

    /// [`once`] in a transaction of its own.
    pub(crate) fn once<T: Serialize + DeserializeOwned>(
        &mut self,
        id: &OperationId,
        request: &Request,
        make: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<Option<T>> {
        let tx = self.immediate()?;
        let outcome = once(&tx, id, request, make)?;
        tx.commit()?;
        Ok(outcome)
    }
}

/// The outcome marked for `id`, else the one `make` decides now in
/// the transaction `db`, marked there for `request`. `None` when `id`
/// is marked for another request, and then nothing runs.
pub(crate) fn once<T: Serialize + DeserializeOwned>(
    db: &Connection,
    id: &OperationId,
    request: &Request,
    make: impl FnOnce(&Connection) -> Result<T>,
) -> Result<Option<T>> {
    let marked = kept(db, id)?
        .map(|kept| reply_of(kept, request))
        .transpose()?;
    let begun = match marked {
        // A marked outcome stands even when its authority has since
        // gone.
        Some(Marked::Outcome(outcome)) => return Ok(Some(outcome)),
        Some(Marked::Conflict) => return Ok(None),
        Some(Marked::Begun) => true,
        None => false,
    };
    let outcome = make(db)?;
    if begun {
        settle(db, id, &outcome)?;
    } else {
        mark(db, id, request, &outcome)?;
    }
    Ok(Some(outcome))
}

/// Runs `operation` in the transaction `db`, whose owner keeps any
/// marker: the authority check, then the change, undone when it
/// refuses.
pub(crate) fn change<O: Operation>(
    db: &Connection,
    operation: &O,
) -> Result<Outcome<O::Done, O::Refusal>> {
    // Checked here, as it runs: a harness recovering a call can skip
    // its own check, as pi-durable skips `beforeTool` on replay.
    if let Some(refusal) = operation.authorize(db)? {
        return Ok(Outcome::Refused(refusal));
    }
    db.execute_batch("SAVEPOINT operation")?;
    let outcome = operation.apply(db);
    if matches!(outcome, Ok(Outcome::Done(_))) {
        db.execute_batch("RELEASE operation")?;
    } else {
        db.execute_batch("ROLLBACK TO operation; RELEASE operation")?;
    }
    outcome
}

/// What the marker of `id` keeps for requests of `request`'s kind;
/// `None` when it has no marker of that kind.
pub(crate) fn marked<T: DeserializeOwned>(
    db: &Connection,
    id: &OperationId,
    request: &Request,
) -> Result<Option<Marked<T>>> {
    kept(db, id)?
        .filter(|(kind, ..)| kind == request.kind)
        .map(|kept| reply_of(kept, request))
        .transpose()
}

/// Marks `id` as begun for `request`, before an effect that must not
/// run again.
pub(crate) fn begin(
    db: &Connection,
    id: &OperationId,
    request: &Request,
) -> Result<()> {
    db.execute(
        "INSERT INTO marker (runtime, key, kind, digest, outcome) \
         VALUES (?1, ?2, ?3, ?4, '')",
        params![
            id.runtime.as_str(),
            id.key,
            request.kind,
            request.digest
        ],
    )?;
    Ok(())
}

/// Keeps `outcome` as the marked outcome of `id`.
pub(crate) fn settle(
    db: &Connection,
    id: &OperationId,
    outcome: &impl Serialize,
) -> Result<()> {
    db.execute(
        "UPDATE marker SET outcome = ?3 WHERE runtime = ?1 AND key \
         = ?2",
        params![
            id.runtime.as_str(),
            id.key,
            serde_json::to_string(outcome)?
        ],
    )?;
    Ok(())
}

/// Whether the call `key` of `runtime` is marked, begun or done.
pub(crate) fn is_marked(
    db: &Connection,
    runtime: &str,
    key: &str,
) -> Result<bool> {
    Ok(db
        .query_row(
            "SELECT 1 FROM marker WHERE runtime = ?1 AND key = ?2",
            params![runtime, key],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// The kind, digest and outcome marked for `id`.
fn kept(
    db: &Connection,
    id: &OperationId,
) -> Result<Option<(String, Vec<u8>, String)>> {
    Ok(db
        .query_row(
            "SELECT kind, digest, outcome FROM marker WHERE runtime \
             = ?1 AND key = ?2",
            params![id.runtime.as_str(), id.key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}

/// What the marker `kept` keeps for `request`.
fn reply_of<T: DeserializeOwned>(
    (_, digest, outcome): (String, Vec<u8>, String),
    request: &Request,
) -> Result<Marked<T>> {
    Ok(if digest != request.digest {
        Marked::Conflict
    } else if outcome.is_empty() {
        Marked::Begun
    } else {
        Marked::Outcome(serde_json::from_str(&outcome)?)
    })
}

fn mark(
    db: &Connection,
    id: &OperationId,
    request: &Request,
    outcome: &impl Serialize,
) -> Result<()> {
    db.execute(
        "INSERT INTO marker (runtime, key, kind, digest, outcome) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.runtime.as_str(),
            id.key,
            request.kind,
            request.digest,
            serde_json::to_string(outcome)?
        ],
    )?;
    Ok(())
}

/// `value` as JSON with each object's keys sorted, so equal requests
/// have equal digests in every process.
pub(crate) fn canonical(value: &Value, out: &mut String) {
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

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;
    use serde_json::json;

    use super::*;

    #[test]
    fn a_changed_request_under_a_used_key_runs_nothing() {
        let scratch = Scratch::new("operation-changed");
        let mut store =
            Store::open(&scratch.join("realm.sqlite")).unwrap();
        let id = OperationId {
            runtime: RuntimeInstance::new("pi-1").unwrap(),
            key: "7".into(),
        };
        let asked = |name: &str| {
            Request::tool("set_model", &json!({"name": name}))
                .unwrap()
        };
        let first =
            store
                .once(&id, &asked("ana"), |_| {
                    Ok("ana's model".to_owned())
                })
                .unwrap();
        assert_eq!(first.as_deref(), Some("ana's model"));
        let again = store
            .once(&id, &asked("ana"), |_| -> Result<String> {
                panic!("a marked call ran again")
            })
            .unwrap();
        assert_eq!(again.as_deref(), Some("ana's model"));
        let changed = store
            .once(&id, &asked("bo"), |_| -> Result<String> {
                panic!("a changed request ran under a used key")
            })
            .unwrap();
        assert_eq!(changed, None);
        assert_eq!(
            marked::<String>(store.connection(), &id, &asked("bo"))
                .unwrap(),
            Some(Marked::Conflict)
        );
    }
}
