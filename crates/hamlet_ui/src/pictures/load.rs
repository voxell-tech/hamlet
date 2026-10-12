//! A picture read from its bytes into pixels small enough to keep.

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use bevy::math::UVec2;
use image::{DynamicImage, ImageDecoder, ImageReader};

/// The most memory a picture may ask for while it is read, in bytes.
const ALLOWED: u64 = 512 * 1024 * 1024;

/// The longest side of a picture that is read, in pixels.
const LONGEST: u32 = 32_768;

/// The pixels of a picture, four bytes to each: sRGB with alpha.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Pixels {
    pub size: UVec2,
    pub data: Vec<u8>,
}

/// The bounds a picture is read within.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Bounds {
    /// The largest file read, in bytes.
    pub bytes: u64,
    /// The most pixels kept, across and down.
    pub pixels: UVec2,
}

/// `size` made no larger than `most` either way, its proportions
/// kept. A picture is never made larger, and one with no pixels
/// stays so.
pub(crate) fn fit(size: UVec2, most: UVec2) -> UVec2 {
    if size.min_element() == 0 || most.min_element() == 0 {
        return UVec2::ZERO;
    }
    let (size, most) = (size.as_dvec2(), most.as_dvec2());
    let by = (most.x / size.x).min(most.y / size.y).min(1.0);
    (size * by).round().as_uvec2().max(UVec2::ONE)
}

/// The picture in `bytes`, of whichever kind its first bytes say,
/// made to fit in `most`. An animated one is its first frame.
pub(crate) fn decode(bytes: &[u8], most: UVec2) -> Option<Pixels> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(LONGEST);
    limits.max_image_height = Some(LONGEST);
    limits.max_alloc = Some(ALLOWED);
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let turned = decoder.orientation().ok();
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(turned) = turned {
        image.apply_orientation(turned);
    }
    let whole = UVec2::new(image.width(), image.height());
    let size = fit(whole, most);
    if size.min_element() == 0 {
        return None;
    }
    if size != whole {
        image = image.thumbnail_exact(size.x, size.y);
    }
    Some(Pixels {
        size,
        data: image.into_rgba8().into_raw(),
    })
}

/// The picture in the file `path`, within `bounds`.
pub(crate) fn read(path: &Path, bounds: Bounds) -> Option<Pixels> {
    let mut bytes = Vec::new();
    // One byte past the bound tells a file that has grown since it
    // was measured.
    File::open(path)
        .ok()?
        .take(bounds.bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > bounds.bytes {
        return None;
    }
    decode(&bytes, bounds.pixels)
}

#[cfg(test)]
pub(crate) mod tests {
    use image::{ImageFormat, RgbaImage};

    use super::*;

    /// A PNG of `size`, one colour all over.
    pub(crate) fn png(size: UVec2) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        RgbaImage::from_pixel(
            size.x,
            size.y,
            image::Rgba([200, 150, 60, 255]),
        )
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn a_picture_is_scaled_into_both_caps_with_its_proportions() {
        let most = UVec2::new(1600, 720);
        assert_eq!(
            fit(UVec2::new(800, 600), most),
            UVec2::new(800, 600)
        );
        assert_eq!(
            fit(UVec2::new(12_000, 3000), most),
            UVec2::new(1600, 400)
        );
        assert_eq!(
            fit(UVec2::new(1000, 7200), most),
            UVec2::new(100, 720)
        );
        assert_eq!(
            fit(UVec2::new(30_000, 2), most),
            UVec2::new(1600, 1)
        );
        assert_eq!(fit(UVec2::ZERO, most), UVec2::ZERO);

        let kept =
            decode(&png(UVec2::new(400, 100)), UVec2::new(100, 100))
                .unwrap();
        assert_eq!(kept.size, UVec2::new(100, 25));
        assert_eq!(kept.data.len(), 100 * 25 * 4);
    }

    #[test]
    fn only_a_picture_within_its_bounds_is_read() {
        let folder = hamlet_testing::Scratch::new("pictures-read");
        let bytes = png(UVec2::new(8, 8));
        let bounds = |bytes: usize| Bounds {
            bytes: bytes as u64,
            pixels: UVec2::splat(64),
        };
        let picture = folder.join("pear.png");
        std::fs::write(&picture, &bytes).unwrap();
        assert!(read(&picture, bounds(bytes.len())).is_some());
        assert_eq!(read(&picture, bounds(bytes.len() - 1)), None);

        // A name is not believed: the bytes say what a file is.
        let words = folder.join("words.png");
        std::fs::write(&words, "<svg></svg>").unwrap();
        assert_eq!(read(&words, bounds(1024)), None);
        let cut = folder.join("cut.png");
        std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
        assert_eq!(read(&cut, bounds(1024)), None);
        assert_eq!(
            read(&folder.join("none.png"), bounds(1024)),
            None
        );
    }
}
