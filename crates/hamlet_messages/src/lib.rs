//! The calls Hamlet's actors make to each other, and their answers,
//! one module per actor that takes them.
//!
//! A call's arguments and its answer cross the host as CBOR of the
//! JSON value model. Every message keeps these rules:
//!
//! - An enum whose cases hold data is a map with its case's name, in
//!   snake_case, under `type`, and an enum of bare names is the name
//!   as text.
//! - A field that is not set is absent, never `null`. A reader
//!   ignores a field it does not know and reads `null` as unset, and
//!   a writer sends neither.
//! - Bytes are base64 text and a hash is hex text.
//! - An integer that a TypeScript writer sends is a `u32`, which a
//!   JavaScript number holds exactly.
//! - A list holds maps, numbers or an enum's names, never free text.
//!   [`realm::SecretList`] is the one exception, and only Rust reads
//!   it.
//! - An answer's arm that holds a typed reason holds its words too,
//!   written by [`worded`] and ignored when read.
//! - A union of families is untagged, and no two of its families
//!   share a case's `type`.
//! - An answer `refused` means the taker kept nothing of the request,
//!   so an id that was only ever refused may be sent again with
//!   another request.
//!
//! A taker checks a call in this order:
//!
//! 1. A call from a caller that is not its message's
//!    [`Message::From`] fails with the error `message.wrong_caller`
//!    ([`sender::WrongCaller`]).
//! 2. A request whose shape is refused, such as a script too long or
//!    a path that is not absolute, is answered `refused`, whatever
//!    its id.
//! 3. A call whose id a kept call of another request holds fails with
//!    the error `message.id_reused` ([`sender::IdReused`]).
//!
//! No answer has a case for either error.
//!
//! The client protocol's types a message carries keep that
//! protocol's rules where they differ from these: an enum whose
//! cases hold data is tagged by `kind`, a reader refuses `null` and a
//! field it does not know, and an integer is a `u64`.
//! [`realm::client::Say`], [`realm::client::Answer`] and the
//! attachments of [`agent::Input::Line`] carry them.
//!
//! The realm takes a repeat of a call that carries an id, a chief's
//! [`realm::Asked`] or [`realm::client`] request or an agent's
//! [`realm::Tool`], by its sender, its id and the SHA-256 of the CBOR
//! of the act it carries.
//!
//! `fixtures/` holds the bytes of each one as the host carries it,
//! so a host in another language can check its shapes against them.
//! A change to a shape rewrites its fixture and fails the test once.
//! A stored fixture is never rewritten, so a change to a type a row
//! keeps fails every run until it reads the stored bytes as before.
//! `fixtures/error/` holds each error in the test's own form,
//! `{group, code, message, metadata}`, not the form of
//! `ActorError`'s serde: no wire between hosts carries an error yet.
//!
//! # How a message may change
//!
//! An actor keeps messages in its file as CBOR of the Rust type
//! itself. `fixtures/stored/` holds each case of each enum that a
//! type a row keeps holds in a stored or a message fixture. The
//! workspace keeps an agent's
//! [`workspace::reach::Reach`]; the request of a call that waits for
//! a chief ([`workspace::Run`],
//! [`workspace::files::WriteFile`], [`workspace::files::EditFile`]),
//! what it needs and the [`workspace::files::Hashed`] it read; and a
//! call's end ([`workspace::CommandEnd`],
//! [`workspace::files::ChangeEnd`],
//! [`workspace::refusal::RunNotRun`],
//! [`workspace::refusal::ChangeNotRun`]). A repeat of a call that
//! carries an id is matched by the SHA-256 of its request's stored
//! bytes.
//!
//! Until Hamlet's first release a message may change in any way. The
//! change rewrites its fixtures, and a folder written before it is
//! not read after it: there is no migration. From the first release:
//!
//! 1. A struct grows only by a field that may be absent, an `Option`
//!    skipped when it is `None`. An older row then reads as `None`,
//!    and a request without the field keeps its stored bytes, so its
//!    repeat still matches.
//! 2. An enum grows only by a new case. A reader fails on a `type` it
//!    does not know; the Rust host and the Node host ship as one
//!    install, so a reader is never older than its writer.
//! 3. No field or case is renamed, removed or given another type, and
//!    the fields of a request keep their order: each of these changes
//!    a row's bytes or a request's hash. A message that must change
//!    so becomes a new action under a new name, and the old one is
//!    kept while a row can hold it.
//! 4. A stored fixture is added to, never rewritten. Rewriting one is
//!    a migration, and needs the code that reads the old rows in the
//!    same change. A stored answer is checked by the value it reads
//!    as, so a change of words rewrites none; a stored request is
//!    also checked by its bytes.
//!
//! There is one set of types: what a row keeps is the message.

pub mod address;
pub mod agent;
pub mod bytes;
pub mod profile;
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
/// `words`, its `Display`.
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
