//! The images the realm keeps, each once under the hash of its bytes
//! ([`store`]), and the making of one to fit a model ([`fitting`])
//! (S29, S30): sent by a client ([`uploads`]), carried by a chief's
//! line to an agent ([`attaching`]), and swept once nothing uses them
//! ([`garbage_collection`]).

pub(crate) mod attaching;
pub(crate) mod fitting;
pub(crate) mod garbage_collection;
pub(crate) mod store;
pub(crate) mod uploads;

use std::time::Duration;

use hamlet_protocol::client::ImageInput;

use crate::timers::AddTimer;

/// The most bytes of an image the realm keeps.
pub(crate) const MOST_BYTES: usize = 20 << 20;
/// The folder of a lodge, or of a worker's scratch folder, that holds
/// the images given to its agent (S29).
pub(crate) const SEEN: &str = "inbox/images";
/// The time between the realm's sweeps for images nothing uses.
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_every(SWEEP_EVERY, garbage_collection::sweep);
}

/// An image's format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
    Webp,
    Gif,
}

impl Format {
    const ALL: [Self; 4] =
        [Self::Png, Self::Jpeg, Self::Webp, Self::Gif];

    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Gif => "gif",
        }
    }

    fn decoding(self) -> image::ImageFormat {
        match self {
            Self::Png => image::ImageFormat::Png,
            Self::Jpeg => image::ImageFormat::Jpeg,
            Self::Webp => image::ImageFormat::WebP,
            Self::Gif => image::ImageFormat::Gif,
        }
    }

    /// The format `bytes` begin as, if it is one an image is kept in.
    pub fn detect(bytes: &[u8]) -> Option<Self> {
        const PNG: [u8; 8] =
            [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        let at = |offset: usize, text: &[u8]| {
            bytes.get(offset..offset + text.len()) == Some(text)
        };
        if at(0, &[0xff, 0xd8, 0xff]) {
            return (bytes.get(3) != Some(&0xf7))
                .then_some(Self::Jpeg);
        }
        if at(0, &PNG) {
            let header = bytes.len() >= 16
                && at(12, b"IHDR")
                && big_endian_u32(bytes, 8) == 13;
            return header.then_some(Self::Png);
        }
        if at(0, b"GIF87a") || at(0, b"GIF89a") {
            return Some(Self::Gif);
        }
        if at(0, b"RIFF") && at(8, b"WEBP") {
            return Some(Self::Webp);
        }
        None
    }
}

fn big_endian_u32(bytes: &[u8], offset: usize) -> usize {
    bytes.get(offset..offset + 4).map_or(0, |b| {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize
    })
}

/// The reason bytes are not an image the realm can keep or give a
/// model.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Unusable {
    #[error("an image is a PNG, JPEG, WebP or GIF")]
    NotAnImage,
    #[error("an image is at most 20 MiB")]
    TooLarge,
    #[error("that image has too many pixels to open")]
    TooManyPixels,
    #[error("that image could not be read")]
    Broken,
    #[error("that image cannot be made small enough for the model")]
    Unfit,
}

/// The limits of the images a model takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    /// The most images of one line.
    pub per_line: u32,
}

impl Limits {
    /// The limits the listing `input` gives, and the defaults for
    /// those it leaves out.
    pub fn of(input: &ImageInput) -> Self {
        Self {
            width: input.max_width.unwrap_or(ImageInput::WIDTH),
            height: input.max_height.unwrap_or(ImageInput::HEIGHT),
            bytes: input.max_bytes.unwrap_or(ImageInput::BYTES),
            per_line: input
                .per_line
                .unwrap_or(ImageInput::PER_LINE)
                .min(ImageInput::PER_LINE),
        }
    }
}

/// `words` with its first letter in capitals.
fn capitalised(words: &str) -> String {
    let mut letters = words.chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}
