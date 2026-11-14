//! The calls Hamlet's actors make to each other, and their answers,
//! one module per actor that takes them.
//!
//! A call's arguments and its answer cross the host as CBOR of the
//! JSON value model. Every message keeps these rules:
//!
//! - An enum whose cases hold data is a map with its case's name, in
//!   snake_case, under `type`, and an enum of bare names is the name
//!   as text.
//! - A field that is not set is absent, never `null`.
//! - Bytes are base64 text and a hash is hex text.
//! - A list holds maps, numbers or an enum's names, never free text.
//!   [`realm::SecretList`] is the one exception, and only Rust reads
//!   it.
//! - An answer's arm that holds a typed reason holds its words too,
//!   written by [`worded`] and ignored when read.
//! - A union of families is untagged, and no two of its families
//!   share a case's `type`.
//! - An answer `refused` means the taker kept nothing of the call, so
//!   a call's id may be sent again with another request.
//!
//! A call from a caller that is not its message's [`Message::From`]
//! fails with the error `message.wrong_caller`
//! ([`sender::WrongCaller`]), and a call whose id a kept call of
//! another request holds fails with `message.id_reused`
//! ([`sender::IdReused`]). No answer has a case for either.
//!
//! `fixtures/` holds the bytes of each one as the host carries it,
//! so a host in another language can check its shapes against them.
//! A change to a shape rewrites its fixture and fails the test once.

pub mod address;
pub mod agent;
pub mod bytes;
pub mod realm;
pub mod sender;
pub mod workspace;

use std::fmt::Display;

use hamlet_actor::Action;
use serde::ser::SerializeStruct as _;
use serde::{Serialize, Serializer};

/// An action with the actor that takes it and the kind of actor that
/// sends it. The taker checks its caller with [`sender::sender`]
/// before anything else.
pub trait Message: Action {
    type To: address::Party;
    type From: sender::Sender;
}

/// Writes an answer's arm that holds the reason `why` as `why` and
/// `words`, its `Display`. Named by the arm's `serialize_with`; the
/// arm's derived `Deserialize` ignores `words`.
pub fn worded<S, W>(why: &W, to: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    W: Serialize + Display,
{
    let mut arm = to.serialize_struct("worded", 2)?;
    arm.serialize_field("why", why)?;
    arm.serialize_field("words", &why.to_string())?;
    arm.end()
}
