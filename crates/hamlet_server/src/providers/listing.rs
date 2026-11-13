//! What the server reads of a model as its provider lists it. pi
//! passes each provider's listing on whole, and the server keeps it
//! so; this is the one place its fields are read, each on its own: a
//! field that is missing or does not fit is left out, and the rest
//! are still read.

use std::collections::BTreeMap;

use hamlet_protocol::client::{ImageInput, Price as ListedPrice};
use hamlet_protocol::runtime::Price;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// A model as its provider lists it.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Listing {
    #[serde(deserialize_with = "fitting")]
    pub id: String,
    /// What its provider calls it.
    #[serde(deserialize_with = "fitting")]
    pub name: String,
    /// The provider that serves it, when not the one asked.
    #[serde(deserialize_with = "fitting")]
    pub provider: Option<String>,
    #[serde(deserialize_with = "fitting")]
    pub base_url: Option<String>,
    #[serde(deserialize_with = "fitting")]
    pub context_window: Option<u64>,
    /// The context the catalogue shows, in place of the window.
    #[serde(deserialize_with = "fitting")]
    pub listed_context: Option<u64>,
    #[serde(deserialize_with = "fitting")]
    pub max_tokens: Option<u64>,
    /// The price the catalogue shows, in place of the cost.
    #[serde(deserialize_with = "fitting")]
    pub listed_price: Option<ListedPrice>,
    #[serde(deserialize_with = "fitting")]
    cost: Cost,
    /// Whether it can be asked to think.
    #[serde(deserialize_with = "fitting")]
    pub reasoning: bool,
    /// What each effort is called for it; a null effort it lacks.
    #[serde(deserialize_with = "fitting")]
    pub thinking_level_map: Option<BTreeMap<String, Value>>,
    /// The API it is called through.
    #[serde(deserialize_with = "fitting")]
    pub api: Option<String>,
    /// What it takes in: text, images.
    #[serde(deserialize_with = "fitting")]
    input: Vec<String>,
    #[serde(deserialize_with = "fitting")]
    input_limits: InputLimits,
}

/// What a call of it costs, per million tokens.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Cost {
    #[serde(deserialize_with = "fitting")]
    input: Option<f64>,
    #[serde(deserialize_with = "fitting")]
    output: Option<f64>,
    #[serde(deserialize_with = "fitting")]
    cache_read: Option<f64>,
    #[serde(deserialize_with = "fitting")]
    cache_write: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct InputLimits {
    #[serde(deserialize_with = "fitting")]
    images: ImageLimits,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ImageLimits {
    #[serde(deserialize_with = "fitting")]
    resize: Resize,
    #[serde(deserialize_with = "fitting")]
    max_per_message: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Resize {
    #[serde(deserialize_with = "fitting")]
    max_bytes: Option<u64>,
    #[serde(deserialize_with = "fitting")]
    max_height: Option<u64>,
    #[serde(deserialize_with = "fitting")]
    max_width: Option<u64>,
}

impl Listing {
    /// What the server reads of `model`; nothing when it is not an
    /// object.
    pub fn of(model: &Value) -> Self {
        Self::deserialize(model).unwrap_or_default()
    }

    /// Its price per call, when its provider lists every part of it.
    pub fn price(&self) -> Option<Price> {
        let cost = &self.cost;
        Some(Price {
            input: cost.input?,
            output: cost.output?,
            cache_read: cost.cache_read?,
            cache_write: cost.cache_write?,
        })
    }

    /// The price the catalogue shows: as listed, else its input and
    /// output cost.
    pub fn shown_price(&self) -> Option<ListedPrice> {
        self.listed_price.clone().or_else(|| {
            Some(ListedPrice {
                input: self.cost.input?,
                output: self.cost.output?,
            })
        })
    }

    /// The images it takes, none when it takes none.
    pub fn images(&self) -> Option<ImageInput> {
        let takes = self.input.iter().any(|kind| kind == "image");
        let limits = &self.input_limits.images;
        let size = |size: Option<u64>| size.filter(|size| *size > 0);
        let small = |size: Option<u64>| {
            size.filter(|size| *size > 0)
                .and_then(|size| u32::try_from(size).ok())
        };
        takes.then(|| ImageInput {
            max_bytes: size(limits.resize.max_bytes),
            max_height: small(limits.resize.max_height),
            max_width: small(limits.resize.max_width),
            per_line: small(limits.max_per_message),
        })
    }
}

/// A field as given, or its default when it does not fit.
fn fitting<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    let value = Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_default())
}
