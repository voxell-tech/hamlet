//! Images a chief attaches to a line: sent in parts, kept by an id of
//! their bytes, and fetched by it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::RequestId;
use crate::client::hash_id::hash_id;

hash_id! {
    /// An image's id: the SHA-256 of its bytes, as 64 lowercase
    /// hexadecimal digits.
    ImageId, InvalidImageId,
    "an image id is 64 lowercase hexadecimal digits"
}

/// An image a line carries, sent before it, and where in the line's
/// text it sits.
#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct AttachedImage {
    /// The byte offset into the line's text, on a character
    /// boundary and at most the text's length, where the image sits.
    /// Without one it goes after the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "u64", range(max = 9_007_199_254_740_991_u64))]
    pub at: Option<u64>,
    pub id: ImageId,
}

impl AttachedImage {
    /// An image that goes after the line's text.
    pub fn last(id: ImageId) -> Self {
        Self { at: None, id }
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

    /// The endings of the files an image is taken in (C33), in small
    /// letters.
    pub const ENDINGS: [&str; 5] =
        ["png", "jpg", "jpeg", "webp", "gif"];

    /// Whether the name of `file` says it is an image that is taken.
    pub fn is_image_file(file: &std::path::Path) -> bool {
        file.extension()
            .and_then(|ending| ending.to_str())
            .is_some_and(|ending| {
                Self::ENDINGS
                    .contains(&ending.to_ascii_lowercase().as_str())
            })
    }
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
