//! The calls Hamlet's actors make to each other, and their answers,
//! one module per actor that takes them.
//!
//! A call's arguments and its answer cross the host as CBOR of the
//! JSON value model. An enum whose cases hold data is a map with its
//! case's name, in snake_case, under `type`, and an enum of bare
//! names is the name as text. A field that is not set is absent,
//! never `null`. Bytes are base64 text and a hash is hex text.
//!
//! `fixtures/` holds the bytes of each one as the host carries it,
//! so a host in another language can check its shapes against them.
//! A change to a shape rewrites its fixture and fails the test once.

pub mod agent;
pub mod bytes;
pub mod realm;
pub mod workspace;
