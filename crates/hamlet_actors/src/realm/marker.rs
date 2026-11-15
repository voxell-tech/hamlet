//! The `marker` table: what every chief's request and every society
//! tool call that changed the realm made, under the id its repeat
//! carries (S19).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

use anyhow::Result;
use hamlet_messages::bytes::Sha256;
use hamlet_messages::sender::id_reused;
use hamlet_messages::workspace::CallId;
use hamlet_protocol::client::RequestId;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::callers::Witness;
use super::calls::Push;
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
        todo!()
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
        todo!()
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
    todo!()
}

pub(super) async fn record(
    writer: &Writer<'_>,
    key: &Key,
    done: &impl Serialize,
    pushes: &[Push],
) -> Result<()> {
    todo!()
}
