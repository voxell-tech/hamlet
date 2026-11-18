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
//!   [`realm::SecretList`] is the one exception: only a scrub reads
//!   it, and nothing logs or stores it.
//! - An answer's arm that holds a typed reason holds its words too,
//!   written by [`worded`] and ignored when read.
//! - A union of families is untagged, and no two of its families
//!   share a case's `type`.
//! - An answer `refused` means the act did not run. Unless its reason
//!   is [`realm::Gate::Reserved`], the taker kept nothing of the
//!   request, so an id that was only ever refused may be sent again
//!   with another request.
//! - A call's arguments and its answer are each at most [`MOST_CALL`]
//!   as the host carries them. No host checks it, so the sender does:
//!   what is larger goes in parts of at most [`MOST_PART`], and the
//!   taker refuses a part that holds more. Six answers are not split:
//!   [`profile::VersionList`], whose versions nothing bounds and
//!   whose commits are at most [`profile::MOST_COMMITS_SEEN`];
//!   [`profile::PurgeAnswer`]'s `still_hold`, at most one for each
//!   hire; [`workspace::ParcelAnswer`], whose files the workspace
//!   bounds; and [`profile::PaperList`], [`profile::RevertAnswer`]'s
//!   clashes and [`library::ShelfList`], which nothing bounds.
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
//! protocol's rules where they differ from these. A reader refuses a
//! field it does not know, and refuses `null` in a field that may be
//! absent. An integer is a `u64` or an `i64`. The pick of
//! [`realm::client::Answer`] is tagged by `kind`, and a sign-in's
//! `Login` and `LoginStep` are untagged: their fields tell the case.
//! Rust writes them: in a chief's request ([`realm::client::Say`],
//! [`realm::client::Answer`], the calls of [`agent::chat`] and of
//! [`realm::choices`], and [`realm::documents::EditSettings`]), in
//! the attachments of [`agent::Input::Line`]
//! and the effort of [`agent::copies::ModelChoice`], and as the
//! [`RequestId`](hamlet_protocol::client::RequestId) of a chief's or
//! the realm's request. An agent writes four, which hold no integer:
//! the kind of an item and of a chat, the step of
//! [`agent::chat::LoginAnswer`], and the effort of
//! [`realm::choices::SetModel`] as a society tool.
//!
//! The realm takes a repeat of a call that carries an id, a chief's
//! [`realm::Asked`] or [`realm::client`] request or an agent's
//! [`realm::Tool`], by its sender, its id and the SHA-256 of the CBOR
//! of the act it carries. A profile and the library take a repeat by
//! what the message says of itself ([`Changes::repeat`]), its sender
//! and the SHA-256 of the CBOR of the message.
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
//! bytes. The host keeps a profile's [`profile::ProfileInput`]; a
//! profile and the library keep the answer of each call that carries
//! an id; and a profile keeps the [`profile::DreamEnd`] of each dream
//! in the dream's row.
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

/// Makes `$name` a [`Message`] from `$from` to `$to` answered
/// `$output`, and a [`Changes`] that tells its repeat by `$repeat`.
macro_rules! message {
    ($name:ident, $wire:literal, $from:ty => $to:ty, $output:ty) => {
        impl ::hamlet_actor::Action for $name {
            type Output = $output;
            const NAME: &'static str = $wire;
        }
        impl $crate::Message for $name {
            type To = $to;
            type From = $from;
        }
    };
    (
        $name:ident, $wire:literal, $from:ty => $to:ty, $output:ty,
        $repeat:expr
    ) => {
        message!($name, $wire, $from => $to, $output);
        impl $crate::Changes for $name {
            fn repeat(&self) -> $crate::Repeat<'_> {
                let repeat: fn(&Self) -> $crate::Repeat<'_> = $repeat;
                repeat(self)
            }
        }
    };
}

/// A family of refusals: an enum tagged `type`.
macro_rules! family {
    ($(#[$doc:meta])* $name:ident { $($cases:tt)* }) => {
        $(#[$doc])*
        #[derive(
            Clone, Debug, PartialEq, Eq, ::serde::Serialize,
            ::serde::Deserialize, ::thiserror::Error,
        )]
        #[serde(tag = "type", rename_all = "snake_case")]
        pub enum $name { $($cases)* }
    };
}

/// An untagged union of families, each its own case.
macro_rules! union {
    ($(#[$doc:meta])* $name:ident { $($case:ident($family:ty)),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(
            Clone, Debug, PartialEq, Eq, ::serde::Serialize,
            ::serde::Deserialize, ::thiserror::Error,
        )]
        #[serde(untagged)]
        pub enum $name {
            $(#[error("{0}")] $case($family)),+
        }
        $(impl From<$family> for $name {
            fn from(why: $family) -> Self {
                Self::$case(why)
            }
        })+
    };
}

/// An answer: its arms, and `refused` with a `$why`.
macro_rules! answer {
    ($(#[$doc:meta])* $name:ident<$why:ty> { $($arms:tt)* }) => {
        $(#[$doc])*
        #[derive(
            Clone, Debug, PartialEq, Eq, ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #[serde(tag = "type", rename_all = "snake_case")]
        pub enum $name {
            $($arms)*
            #[serde(serialize_with = "crate::worded")]
            Refused { why: $why },
        }
        impl $crate::realm::Refusable for $name {
            type Why = $why;
            fn refused(why: $why) -> Self {
                Self::Refused { why }
            }
        }
    };
}

pub mod address;
pub mod agent;
pub mod bytes;
pub mod library;
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

/// The most bytes of a call's arguments, and of its answer, as the
/// host carries them.
pub const MOST_CALL: usize = 1 << 20;

/// The most a part holds: the bytes of its files with, for each file
/// or piece, its path and [`PIECE_FRAME`]. Base64 makes four bytes of
/// three, so a full part is under [`MOST_CALL`] once written.
pub const MOST_PART: usize = 720 * 1024;

/// What a file or a piece counts for in a part beside its path and
/// its bytes.
pub const PIECE_FRAME: usize = 64;

/// A message that changes its taker, with how the taker tells its
/// repeat.
pub trait Changes: Message {
    fn repeat(&self) -> Repeat<'_>;
}

/// How a taker tells a repeat of a [`Changes`] message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat<'a> {
    /// The id its repeat carries.
    Id(&'a str),
    /// One of several calls under one id, each kept under its number.
    Part { id: &'a str, part: u32 },
    /// It carries no id: the taker finds what it made and answers the
    /// same.
    Finds,
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
