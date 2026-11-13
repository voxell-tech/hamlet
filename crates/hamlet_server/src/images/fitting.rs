//! An image made to fit a model (S30): turned upright, made smaller
//! and encoded again until it fits what the model takes.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::Result;
use base64::Engine as _;
use hamlet_protocol::client::ImageId;
use hamlet_protocol::runtime::Picture;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder,
    ImageReader, Limits as DecodeLimits,
};

use super::kept::Images;
use super::{Format, Limits, Unusable};
use crate::diagnostic::{Secrets, tell};

/// The most pixels of an image that is opened.
pub(super) const MOST_PIXELS: u64 = 64_000_000;
/// The JPEG quality an image is first made at, and the least.
const QUALITY: u8 = 80;
const LEAST_QUALITY: u8 = 40;
/// The shortest side an image is made down to.
const LEAST_SIDE: u32 = 32;

/// An image made to fit a model.
pub(crate) struct Fitted {
    pub bytes: Vec<u8>,
    pub format: Format,
    /// Its size before it was made smaller, when it was.
    pub from: Option<(u32, u32)>,
    pub size: (u32, u32),
}

/// The images `ids` in `folder` as a model is given them, made to
/// fit `limits`. An image that cannot be is left out.
pub(crate) fn pictures(
    folder: &Path,
    ids: &[ImageId],
    limits: Limits,
) -> Vec<Picture> {
    let images = Images::new(folder.to_owned());
    ids.iter()
        .filter_map(|id| match images.picture(id, limits) {
            Ok(picture) => Some(picture),
            Err(error) => {
                tell(
                    Secrets::Fixed,
                    format!("an image was left out: {error:#}"),
                );
                None
            }
        })
        .collect()
}

/// `bytes` made to fit `limits` and kept in `folder`, on a thread of
/// their own.
pub(crate) async fn keep_fitted(
    folder: PathBuf,
    bytes: Vec<u8>,
    format: Format,
    limits: Limits,
) -> Result<std::result::Result<(ImageId, Fitted), Unusable>> {
    tokio::task::spawn_blocking(move || {
        let fitted = match fit(&bytes, format, limits) {
            Ok(fitted) => fitted,
            Err(unusable) => return Ok(Err(unusable)),
        };
        Ok(Images::new(folder)
            .keep(&fitted.bytes)?
            .map(|(id, _)| (id, fitted)))
    })
    .await?
}

/// The images `ids` as they are kept, for a tool's result; one that
/// is not kept is left out.
pub(crate) fn kept_pictures(
    folder: PathBuf,
    ids: &[ImageId],
) -> Vec<Picture> {
    let images = Images::new(folder);
    ids.iter()
        .filter_map(|id| images.find(id).ok().flatten())
        .map(|stored| picture(&stored.bytes, stored.format))
        .collect()
}

/// `bytes` of an image of `format` as a model is given it.
pub(super) fn picture(bytes: &[u8], format: Format) -> Picture {
    Picture {
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
        mime: format.mime().into(),
    }
}

pub(super) fn reader(
    bytes: &[u8],
    format: Format,
) -> ImageReader<Cursor<&[u8]>> {
    ImageReader::with_format(Cursor::new(bytes), format.decoding())
}

/// `bytes` made to fit `limits`: as they are when they do, else
/// turned upright, made smaller and encoded again until they do.
pub(crate) fn fit(
    bytes: &[u8],
    format: Format,
    limits: Limits,
) -> std::result::Result<Fitted, Unusable> {
    let mut decoder = reader(bytes, format)
        .into_decoder()
        .map_err(|_| Unusable::Broken)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MOST_PIXELS {
        return Err(Unusable::TooManyPixels);
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let fits = width <= limits.width
        && height <= limits.height
        && bytes.len() as u64 <= limits.bytes;
    if fits
        && orientation == image::metadata::Orientation::NoTransforms
    {
        return Ok(Fitted {
            bytes: bytes.to_vec(),
            format,
            from: None,
            size: (width, height),
        });
    }
    decoder
        .set_limits(DecodeLimits::default())
        .map_err(|_| Unusable::Broken)?;
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|_| Unusable::Broken)?;
    image.apply_orientation(orientation);
    let (upright_width, upright_height) =
        (image.width(), image.height());
    let scale = f64::min(
        f64::from(limits.width) / f64::from(upright_width),
        f64::from(limits.height) / f64::from(upright_height),
    )
    .min(1.0);
    let mut scale = scale;
    let mut quality = QUALITY;
    loop {
        let side = |side: u32| {
            ((f64::from(side) * scale).round() as u32).max(1)
        };
        let size = (side(upright_width), side(upright_height));
        let made = if size == (upright_width, upright_height) {
            image.clone()
        } else {
            image.resize_exact(size.0, size.1, FilterType::Lanczos3)
        };
        let (encoded, format) = encode(&made, quality)?;
        if encoded.len() as u64 <= limits.bytes {
            return Ok(Fitted {
                bytes: encoded,
                format,
                from: (size != (width, height))
                    .then_some((width, height)),
                size,
            });
        }
        if format == Format::Jpeg && quality > LEAST_QUALITY {
            quality -= 10;
        } else {
            scale *= 0.75;
            if size.0.min(size.1) <= LEAST_SIDE {
                return Err(Unusable::Unfit);
            }
        }
    }
}

/// `image` as a PNG when it has transparency, else a JPEG of
/// `quality`.
fn encode(
    image: &DynamicImage,
    quality: u8,
) -> std::result::Result<(Vec<u8>, Format), Unusable> {
    let mut out = Vec::new();
    if image.color().has_alpha() {
        let rgba = image.to_rgba8();
        PngEncoder::new(&mut out)
            .write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                ExtendedColorType::Rgba8,
            )
            .map_err(|_| Unusable::Broken)?;
        Ok((out, Format::Png))
    } else {
        let rgb = image.to_rgb8();
        JpegEncoder::new_with_quality(&mut out, quality)
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                ExtendedColorType::Rgb8,
            )
            .map_err(|_| Unusable::Broken)?;
        Ok((out, Format::Jpeg))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(in crate::images) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let pixels =
            image::RgbImage::from_fn(width, height, |x, y| {
                image::Rgb([
                    (x * 7 % 256) as u8,
                    (y * 13 % 256) as u8,
                    90,
                ])
            });
        PngEncoder::new(&mut out)
            .write_image(
                pixels.as_raw(),
                width,
                height,
                ExtendedColorType::Rgb8,
            )
            .unwrap();
        out
    }

    const LIMITS: Limits = Limits {
        width: 100,
        height: 80,
        bytes: 1 << 20,
        per_line: 20,
    };

    #[test]
    fn a_larger_image_is_made_smaller_to_keep_its_shape() {
        let fitted =
            fit(&png(400, 200), Format::Png, LIMITS).unwrap();
        assert_eq!(fitted.from, Some((400, 200)));
        assert_eq!(fitted.size, (100, 50));
        let again = Format::detect(&fitted.bytes).unwrap();
        assert_eq!(again, fitted.format);
    }

    /// `png` with an animation control chunk after its header, which
    /// makes it an animated PNG of one frame.
    fn animated(png: &[u8]) -> Vec<u8> {
        let crc = |bytes: &[u8]| {
            let mut crc = u32::MAX;
            for byte in bytes {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xedb8_8320 * (crc & 1));
                }
            }
            !crc
        };
        let mut chunk = Vec::from(*b"acTL");
        chunk.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0]);
        let mut out = png[..33].to_vec();
        out.extend_from_slice(&8_u32.to_be_bytes());
        out.extend_from_slice(&chunk);
        out.extend_from_slice(&crc(&chunk).to_be_bytes());
        out.extend_from_slice(&png[33..]);
        out
    }

    #[test]
    fn an_animated_png_is_kept_and_made_smaller_by_its_first_frame() {
        let bytes = animated(&png(400, 200));
        assert_eq!(Format::detect(&bytes), Some(Format::Png));
        let big = Limits {
            width: 1000,
            height: 1000,
            ..LIMITS
        };
        let kept = fit(&bytes, Format::Png, big).unwrap();
        assert_eq!(kept.bytes, bytes);
        assert_eq!(
            fit(&bytes, Format::Png, LIMITS).unwrap().size,
            (100, 50)
        );
    }
}
