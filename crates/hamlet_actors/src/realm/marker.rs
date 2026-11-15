//! The `marker` table: what every chief's request and every society
//! tool call that changed the realm made, under the id its repeat
//! carries (S19).

use anyhow::{Result, bail};
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_messages::bytes::Sha256;
use hamlet_messages::sender::id_reused;
use hamlet_messages::workspace::CallId;
use hamlet_protocol::client::RequestId;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::Digest as _;

use super::callers::Witness;
use super::calls::Push;
use super::cbor;
use super::change::{Rows, Writer};

/// The id a repeat carries.
pub(super) enum Id {
    Request(RequestId),
    Call(CallId),
}

/// The SHA-256 of a request's bytes: the CBOR of the act the call
/// carries, as its Rust type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Digest(Sha256);

impl Digest {
    pub fn of(call: &impl Serialize) -> Result<Self> {
        Ok(Self(Sha256(sha2::Sha256::digest(cbor(call)?).into())))
    }
}

pub(super) struct Key {
    caller: String,
    id: String,
    action: &'static str,
    digest: Digest,
}

impl Key {
    pub fn of<By: Witness>(
        by: &By,
        id: Id,
        action: &'static str,
        call: &impl Serialize,
    ) -> Result<Self> {
        Ok(Self {
            caller: by.name(),
            id: match id {
                Id::Request(id) => id.as_str().to_owned(),
                Id::Call(CallId(id)) => id,
            },
            action,
            digest: Digest::of(call)?,
        })
    }

    pub fn reused(&self) -> anyhow::Error {
        id_reused(self.action, &CallId(self.id.clone()))
    }
}

/// A row found under a key's caller and id.
pub(super) enum Found<D> {
    /// The same request: what it made, and the pushes it set off.
    Same { done: D, pushes: Vec<Push> },
    /// Another action, or other arguments.
    Other,
}

pub(super) async fn find<D: DeserializeOwned>(
    rows: &Rows<'_>,
    key: &Key,
) -> Result<Option<Found<D>>> {
    let found = rows
        .select(
            "SELECT action, digest, done, pushes FROM marker WHERE \
             caller = ?1 AND id = ?2",
            vec![
                BindParam::Text(key.caller.clone()),
                BindParam::Text(key.id.clone()),
            ],
        )
        .await?;
    let Some(row) = found.first() else {
        return Ok(None);
    };
    let [
        ColumnValue::Text(action),
        ColumnValue::Blob(digest),
        ColumnValue::Blob(done),
        ColumnValue::Blob(pushes),
    ] = row.as_slice()
    else {
        bail!("a marker row of another form: {row:?}");
    };
    if action != key.action || digest[..] != key.digest.0.0 {
        return Ok(Some(Found::Other));
    }
    Ok(Some(Found::Same {
        done: ciborium::from_reader(done.as_slice())?,
        pushes: ciborium::from_reader(pushes.as_slice())?,
    }))
}

pub(super) async fn record(
    writer: &Writer<'_>,
    key: &Key,
    done: &impl Serialize,
    pushes: &[Push],
) -> Result<()> {
    writer
        .execute(
            "INSERT INTO marker (caller, id, action, digest, done, \
             pushes, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            vec![
                BindParam::Text(key.caller.clone()),
                BindParam::Text(key.id.clone()),
                BindParam::Text(key.action.to_owned()),
                BindParam::Blob(key.digest.0.0.to_vec()),
                BindParam::Blob(cbor(done)?),
                BindParam::Blob(cbor(&pushes)?),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    Ok(())
}
