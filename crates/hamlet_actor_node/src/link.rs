//! The Node host's link to the Rust host that started it.

use napi_derive::napi;

use crate::napi_anyhow_error;

/// Takes this process's standard input and output for the link to
/// the Rust host that started it. From then on standard output
/// writes to standard error and standard input reads nothing, here
/// and in the processes it starts, and the process kills itself when
/// the Rust host is gone. Call it first, before anything reads or
/// writes either. Fails when they are taken already, or when
/// `HAMLET_LINK_PARENT` names no process id.
#[napi]
pub fn take_stdio() -> napi::Result<()> {
    hamlet_host::child::take_stdio()
        .map_err(|error| napi_anyhow_error(error.into()))
}
