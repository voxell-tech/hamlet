//! The Node host's link to the Rust host that started it.

use napi_derive::napi;

use crate::napi_anyhow_error;

/// Takes this process's ends of the link to the Rust host that
/// started it. From then on the process is killed when the Rust host
/// is gone or the link's input ends. Fails when no Rust host started
/// this process, or when it has taken the ends already.
#[napi]
pub fn take_link() -> napi::Result<()> {
    hamlet_host::child::take_link()
        .map_err(|error| napi_anyhow_error(error.into()))
}
