//! The wire format: what crosses between the engine and a plugin, as
//! postcard bytes.

use core::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Bytes that are not a value of the type asked for.
#[derive(Debug)]
pub struct WireError(postcard::Error);

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not readable: {}", self.0)
    }
}

impl std::error::Error for WireError {}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, WireError> {
    postcard::to_allocvec(value).map_err(WireError)
}

/// The value in `bytes`, which must hold nothing after it.
pub fn decode<T: DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, WireError> {
    let (value, rest) =
        postcard::take_from_bytes::<T>(bytes).map_err(WireError)?;
    if rest.is_empty() {
        Ok(value)
    } else {
        Err(WireError(postcard::Error::DeserializeBadEncoding))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        API_VERSION, Ask, AssetTypes, Capability, Events, Manifest,
        PluginId, Services,
    };

    #[test]
    fn a_manifest_crosses_the_wire_unchanged() {
        let mut manifest = Manifest::new(
            PluginId::try_from("hamlet.weather").unwrap(),
            "Weather",
            "0.1.0",
        );
        manifest.asks.push(Ask {
            capability: Capability::NetFetch,
            target: "forecast.example.org".into(),
            reason: "Fetches the forecast".into(),
        });
        let bytes = encode(&manifest).unwrap();
        assert_eq!(decode::<Manifest>(&bytes).unwrap(), manifest);
        // A byte more is refused, as is an id that is not one.
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode::<Manifest>(&longer).is_err());
        let bad = encode(&(
            "Hamlet.Weather",
            "W",
            "0.1.0",
            API_VERSION,
            Vec::<Ask>::new(),
            AssetTypes::default(),
            Services::default(),
            Events::default(),
        ))
        .unwrap();
        assert!(decode::<Manifest>(&bad).is_err());
    }
}
