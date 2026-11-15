//! An actor's address and a call's options, as TypeScript hands them
//! to the native module.

use std::time::Duration;

use hamlet_host::Address;
use napi_derive::napi;

use crate::{NapiInvalidArgument, napi_anyhow_error};

/// A [`hamlet_host::Address`] as TypeScript writes it.
#[napi(object)]
pub struct JsAddress {
    pub kind: String,
    pub key: String,
}

/// The options of one call.
#[napi(object)]
pub struct JsCallOptions {
    /// Milliseconds, whole, from 1 to 2^32 - 1.
    pub deadline_ms: f64,
}

impl JsAddress {
    /// The address, or `napi.invalid_argument` for `to` with
    /// [`hamlet_host::InvalidName`]'s text.
    pub(crate) fn checked(self) -> napi::Result<Address> {
        // `InvalidName` has no code, so the bridge would mask it as
        // `core.internal_error`.
        Address::new(self.kind, self.key).map_err(|error| {
            invalid_argument("to", error.to_string())
        })
    }
}

impl JsCallOptions {
    /// The deadline, or `napi.invalid_argument` for
    /// `options.deadlineMs`.
    pub(crate) fn deadline(&self) -> napi::Result<Duration> {
        // Read as `f64` so that `-1` and `1.5` are refused: napi
        // reads them as a `u32` without a word. A deadline of 0 would
        // start the action with no time left.
        let ms = self.deadline_ms;
        if ms.fract() != 0.0
            || !(1.0..=f64::from(u32::MAX)).contains(&ms)
        {
            return Err(invalid_argument(
                "options.deadlineMs",
                format!(
                    "{ms} is not a whole number of milliseconds \
                     from 1 to {}",
                    u32::MAX
                ),
            ));
        }
        Ok(Duration::from_millis(ms as u64))
    }
}

fn invalid_argument(argument: &str, reason: String) -> napi::Error {
    napi_anyhow_error(
        NapiInvalidArgument {
            argument: argument.to_owned(),
            reason,
        }
        .build(),
    )
}
