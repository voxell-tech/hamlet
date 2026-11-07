//! Images a chief attaches to a line: sent in parts, kept by an id of
//! their bytes, and fetched by it.

use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::client::RequestId;

/// An image's id: the SHA-256 of its bytes, as 64 lowercase
/// hexadecimal digits.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageId(String);

/// Text that is not an [`ImageId`].
#[derive(Debug, PartialEq, thiserror::Error)]
#[error("an image id is 64 lowercase hexadecimal digits")]
pub struct InvalidImageId;

impl ImageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ImageId {
    type Error = InvalidImageId;

    fn try_from(text: String) -> Result<Self, InvalidImageId> {
        let digit =
            |byte: u8| matches!(byte, b'0'..=b'9' | b'a'..=b'f');
        if text.len() == 64 && text.bytes().all(digit) {
            Ok(Self(text))
        } else {
            Err(InvalidImageId)
        }
    }
}

impl FromStr for ImageId {
    type Err = InvalidImageId;

    fn from_str(text: &str) -> Result<Self, InvalidImageId> {
        Self::try_from(text.to_owned())
    }
}

impl Deref for ImageId {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ImageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ImageId {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ImageId {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ImageId {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "ImageId".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "pattern": "^[0-9a-f]{64}$"})
    }
}

/// Begins sending one image of `size` bytes; the reply names the
/// upload its parts are sent under.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct PutImage {
    pub id: RequestId,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub size: u64,
}

/// Sends base64 bytes of an image at a byte offset. The part that
/// completes the image is answered with the image's id.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct PutImagePart {
    pub data: String,
    pub id: RequestId,
    pub offset: u64,
    pub upload: String,
}

/// What a model takes of images, as its listing says. A field left
/// out is not limited.
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ImageInput {
    /// The most bytes of one image; larger ones are made smaller.
    #[serde(
        rename = "maxBytes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "u64")]
    pub max_bytes: Option<u64>,
    /// The most pixels of an image's height; larger ones are made
    /// smaller.
    #[serde(
        rename = "maxHeight",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "u32")]
    pub max_height: Option<u32>,
    /// The most pixels of an image's width; larger ones are made
    /// smaller.
    #[serde(
        rename = "maxWidth",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "u32")]
    pub max_width: Option<u32>,
    /// The most images one line may carry.
    #[serde(
        rename = "perLine",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "u32")]
    pub per_line: Option<u32>,
}

impl ImageInput {
    /// The pixels of width, of height, the bytes and the images of a
    /// line taken where a limit is left out. A line takes no more
    /// images than [`Self::PER_LINE`], whatever the limit says.
    pub const WIDTH: u32 = 2000;
    pub const HEIGHT: u32 = 2000;
    pub const BYTES: u64 = 4_718_592;
    pub const PER_LINE: u32 = 20;
}

/// How much of an image a read gives.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum ImageSize {
    /// The image as it was sent.
    #[default]
    #[serde(rename = "original")]
    Original,
    /// A small copy, enough to show the image among words.
    #[serde(rename = "thumbnail")]
    Thumbnail,
}

/// An image's bytes, in base64.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ImageBytes {
    pub data: String,
    /// Its media type, such as `image/png`.
    pub mime: String,
}
