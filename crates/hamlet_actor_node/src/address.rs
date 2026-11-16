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

#[napi(object)]
pub struct JsCallOptions {
    /// Milliseconds, whole, from 1 to 2^32 - 1.
    pub deadline_ms: f64,
}

impl JsAddress {
    pub(crate) fn checked(self) -> napi::Result<Address> {
        Address::new(self.kind, self.key).map_err(|error| {
            invalid_argument("to", error.to_string())
        })
    }
}

impl JsCallOptions {
    pub(crate) fn deadline(&self) -> napi::Result<Duration> {
        // Read as `f64` so that `-1` and `1.5` are refused: napi
        // reads them as a `u32` without a word.
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
