//! The making of an image to fit a model's limits (S29, S30).

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder,
    ImageReader, Limits as DecodeLimits,
};

/// The most pixels of an image that is opened.
const MOST_PIXELS: u64 = 64_000_000;
/// The JPEG quality an image is first made at, and the least.
const QUALITY: u8 = 80;
const LEAST_QUALITY: u8 = 40;
/// The shortest side an image is made down to.
const LEAST_SIDE: u32 = 32;

/// An image's format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Webp,
    Gif,
}

impl Format {
    pub const ALL: [Self; 4] =
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

    /// The format `bytes` begin as, if it is one a model takes.
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
                && bytes.get(8..12) == Some(&13u32.to_be_bytes());
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

/// Why bytes are not an image the realm can keep or give a model.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unusable {
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

/// The most pixels of an image's width and height, and its most
/// bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

/// An image made to fit a model.
pub struct Fitted {
    pub bytes: Vec<u8>,
    pub format: Format,
    /// Its size before it was made smaller, when it was.
    pub from: Option<(u32, u32)>,
    pub size: (u32, u32),
}

fn opened(
    bytes: &[u8],
    format: Format,
) -> Result<impl ImageDecoder + '_, Unusable> {
    let decoder = ImageReader::with_format(
        Cursor::new(bytes),
        format.decoding(),
    )
    .into_decoder()
    .map_err(|_| Unusable::Broken)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MOST_PIXELS {
        return Err(Unusable::TooManyPixels);
    }
    Ok(decoder)
}

/// Whether `bytes` open as an image of `format`.
pub fn check(bytes: &[u8], format: Format) -> Result<(), Unusable> {
    opened(bytes, format).map(drop)
}

/// `bytes` made to fit `bounds`: as they are when they do, else
/// turned upright, made smaller and encoded again until they do.
pub fn fit(
    bytes: &[u8],
    format: Format,
    bounds: Bounds,
) -> Result<Fitted, Unusable> {
    let mut decoder = opened(bytes, format)?;
    let (width, height) = decoder.dimensions();
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let fits = width <= bounds.width
        && height <= bounds.height
        && bytes.len() as u64 <= bounds.bytes;
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
    let mut scale = f64::min(
        f64::from(bounds.width) / f64::from(upright_width),
        f64::from(bounds.height) / f64::from(upright_height),
    )
    .min(1.0);
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
        if encoded.len() as u64 <= bounds.bytes {
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

fn encode(
    image: &DynamicImage,
    quality: u8,
) -> Result<(Vec<u8>, Format), Unusable> {
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
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
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

    const BOUNDS: Bounds = Bounds {
        width: 100,
        height: 80,
        bytes: 1 << 20,
    };

    #[test]
    fn a_larger_image_is_made_smaller_to_keep_its_shape() {
        let fitted =
            fit(&png(400, 200), Format::Png, BOUNDS).unwrap();
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
        let big = Bounds {
            width: 1000,
            height: 1000,
            ..BOUNDS
        };
        let kept = fit(&bytes, Format::Png, big).unwrap();
        assert_eq!(kept.bytes, bytes);
        assert_eq!(
            fit(&bytes, Format::Png, BOUNDS).unwrap().size,
            (100, 50)
        );
    }

    #[test]
    fn an_image_is_told_by_its_first_bytes() {
        let mut png =
            vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend(13u32.to_be_bytes());
        png.extend(b"IHDR");
        let mut no_header = png.clone();
        no_header[12..16].copy_from_slice(b"IDAT");
        let mut webp = b"RIFF".to_vec();
        webp.extend([0; 4]);
        webp.extend(b"WEBP");

        let detected = |bytes: &[u8]| Format::detect(bytes);
        assert_eq!(detected(&png), Some(Format::Png));
        assert_eq!(detected(&no_header), None);
        assert_eq!(detected(&png[..12]), None);
        assert_eq!(
            detected(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(Format::Jpeg)
        );
        assert_eq!(detected(&[0xff, 0xd8, 0xff, 0xf7]), None);
        assert_eq!(detected(b"GIF89a..."), Some(Format::Gif));
        assert_eq!(detected(b"GIF87a"), Some(Format::Gif));
        assert_eq!(detected(&webp), Some(Format::Webp));
        assert_eq!(detected(b"RIFF"), None);
        assert_eq!(detected(b"plain text"), None);
    }
}
