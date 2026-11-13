//! The calls Hamlet's actors make to each other, and their answers,
//! one module per actor that takes them.
//!
//! A call's arguments and its answer cross the host as CBOR, and may
//! cross to the Node host. `fixtures/` holds the bytes of each one as
//! the host carries it, which this crate's tests and the Node host's
//! tests both read, so the two sides agree on every shape. A change
//! to a shape rewrites its fixture and fails the test once.

pub mod workspace;
