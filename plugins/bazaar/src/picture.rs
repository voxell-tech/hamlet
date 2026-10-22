//! A picture handed to the engine to be shown, checked first from its
//! header alone: one the engine would refuse fails the call and stops
//! the plugin, so what cannot be read here is not shown.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The most bytes of a cover, as the bazaar's server takes them: one
/// fetch's answer holds it (`fetch_bytes`, 512 KiB), and a call has
/// room for two (`bytes_per_call`, 1 MiB).
pub(crate) const MOST_BYTES: usize = 400 * 1024;

/// The most pixels a side, as the bazaar's server takes them: 16 MiB
/// of the 64 the engine holds of a plugin's pictures
/// (`picture_memory`), at four bytes a pixel.
pub(crate) const MOST_SIDE: u32 = 2048;

/// The kinds of picture the engine shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Format {
    Png,
    Jpeg,
    WebP,
}

impl Format {
    /// Its media type, as it is uploaded.
    pub fn media(self) -> &'static str {
        match self {
            Format::Png => "image/png",
            Format::Jpeg => "image/jpeg",
            Format::WebP => "image/webp",
        }
    }
}

/// What the header of a picture says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Header {
    pub format: Format,
    pub width: u32,
    pub height: u32,
}

impl Header {
    /// What the engine holds of it shown, in bytes.
    pub fn memory(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

/// A picture that may be shown.
#[derive(Clone, Debug)]
pub(crate) struct Picture {
    /// Its own number, never 0, so that a view can tell it from
    /// another.
    pub serial: u64,
    pub bytes: Arc<[u8]>,
    pub header: Header,
}

impl Picture {
    pub fn new(bytes: Vec<u8>, header: Header) -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(1);
        Picture {
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
            bytes: bytes.into(),
            header,
        }
    }
}

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        self.serial == other.serial
    }
}

/// The header of a PNG, JPEG or WebP, or `None` when `bytes` are none
/// of those or are cut short of it.
pub(crate) fn header(bytes: &[u8]) -> Option<Header> {
    png(bytes).or_else(|| jpeg(bytes)).or_else(|| webp(bytes))
}

/// The header of `bytes` when they may be shown, or why not.
pub(crate) fn check(bytes: &[u8]) -> Result<Header, String> {
    if bytes.len() > MOST_BYTES {
        return Err(format!(
            "It is {} KB, past the {} KB a cover may be.",
            bytes.len().div_ceil(1024),
            MOST_BYTES / 1024
        ));
    }
    let header = header(bytes)
        .ok_or("It is not a PNG, JPEG or WebP that can be read.")?;
    if header.width.max(header.height) > MOST_SIDE {
        return Err(format!(
            "It is {} by {} pixels, past the {MOST_SIDE} a side a \
             cover may be.",
            header.width, header.height
        ));
    }
    Ok(header)
}

fn sized(format: Format, width: u32, height: u32) -> Option<Header> {
    (width > 0 && height > 0).then_some(Header {
        format,
        width,
        height,
    })
}

fn at<const N: usize>(bytes: &[u8], from: usize) -> Option<[u8; N]> {
    bytes.get(from..from + N)?.try_into().ok()
}

fn be16(bytes: &[u8], from: usize) -> Option<u32> {
    at::<2>(bytes, from).map(|b| u16::from_be_bytes(b).into())
}

fn le16(bytes: &[u8], from: usize) -> Option<u32> {
    at::<2>(bytes, from).map(|b| u16::from_le_bytes(b).into())
}

fn le24(bytes: &[u8], from: usize) -> Option<u32> {
    at::<3>(bytes, from)
        .map(|[a, b, c]| u32::from_le_bytes([a, b, c, 0]))
}

fn png(bytes: &[u8]) -> Option<Header> {
    const SIGNATURE: [u8; 8] = *b"\x89PNG\r\n\x1a\n";
    if at::<8>(bytes, 0)? != SIGNATURE
        || at::<4>(bytes, 12)? != *b"IHDR"
    {
        return None;
    }
    let width = u32::from_be_bytes(at::<4>(bytes, 16)?);
    let height = u32::from_be_bytes(at::<4>(bytes, 20)?);
    sized(Format::Png, width, height)
}

/// The size in the first frame's header, past the segments before it.
fn jpeg(bytes: &[u8]) -> Option<Header> {
    if at::<2>(bytes, 0)? != [0xff, 0xd8] {
        return None;
    }
    let mut from = 2;
    loop {
        if *bytes.get(from)? != 0xff {
            return None;
        }
        while *bytes.get(from)? == 0xff {
            from += 1;
        }
        let marker = bytes[from];
        from += 1;
        match marker {
            // The image's data or its end came before any frame.
            0xd9 | 0xda => return None,
            // Markers with no length after them.
            0x01 | 0xd0..=0xd8 => continue,
            _ => {}
        }
        let length = be16(bytes, from)? as usize;
        if length < 2 {
            return None;
        }
        if matches!(marker, 0xc0..=0xcf)
            && !matches!(marker, 0xc4 | 0xc8 | 0xcc)
        {
            let height = be16(bytes, from + 3)?;
            let width = be16(bytes, from + 5)?;
            return sized(Format::Jpeg, width, height);
        }
        from += length;
    }
}

fn webp(bytes: &[u8]) -> Option<Header> {
    if at::<4>(bytes, 0)? != *b"RIFF"
        || at::<4>(bytes, 8)? != *b"WEBP"
    {
        return None;
    }
    match &at::<4>(bytes, 12)? {
        b"VP8 " => {
            let key_frame = bytes.get(20)? & 1 == 0;
            if !key_frame || at::<3>(bytes, 23)? != [0x9d, 0x01, 0x2a]
            {
                return None;
            }
            let width = le16(bytes, 26)? & 0x3fff;
            let height = le16(bytes, 28)? & 0x3fff;
            sized(Format::WebP, width, height)
        }
        b"VP8L" => {
            if *bytes.get(20)? != 0x2f {
                return None;
            }
            let bits = u32::from_le_bytes(at::<4>(bytes, 21)?);
            let width = (bits & 0x3fff) + 1;
            let height = ((bits >> 14) & 0x3fff) + 1;
            sized(Format::WebP, width, height)
        }
        b"VP8X" => {
            let width = le24(bytes, 24)? + 1;
            let height = le24(bytes, 27)? + 1;
            sized(Format::WebP, width, height)
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The header of a PNG of `width` by `height`.
    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    /// The start of a JPEG of `width` by `height`, an APP0 segment
    /// before its frame.
    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16];
        bytes.extend(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        bytes.extend([0xff, 0xff, 0xc2, 0, 17, 8]);
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes.extend([3; 10]);
        bytes
    }

    fn webp(chunk: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
        bytes.extend(chunk);
        bytes.extend([0; 4]);
        bytes.extend(data);
        bytes.resize(bytes.len().max(30), 0);
        bytes
    }

    #[test]
    fn a_header_gives_each_format_and_its_size() {
        let read = |bytes: &[u8]| {
            header(bytes).map(|h| (h.format, h.width, h.height))
        };
        assert_eq!(
            read(&png(640, 360)),
            Some((Format::Png, 640, 360))
        );
        assert_eq!(
            read(&jpeg(1200, 630)),
            Some((Format::Jpeg, 1200, 630))
        );
        let mut lossy = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
        lossy.extend(320u16.to_le_bytes());
        lossy.extend((240u16 | 0x4000).to_le_bytes());
        assert_eq!(
            read(&webp(b"VP8 ", &lossy)),
            Some((Format::WebP, 320, 240))
        );
        let mut lossless = vec![0x2f];
        lossless.extend((99u32 | (49 << 14)).to_le_bytes());
        assert_eq!(
            read(&webp(b"VP8L", &lossless)),
            Some((Format::WebP, 100, 50))
        );
        let extended = [0, 0, 0, 0, 0xff, 0x0f, 0, 0x37, 0, 0];
        assert_eq!(
            read(&webp(b"VP8X", &extended)),
            Some((Format::WebP, 4096, 56))
        );
    }

    #[test]
    fn what_is_cut_short_or_not_a_picture_is_not_read() {
        assert_eq!(header(&png(640, 360)[..20]), None);
        assert_eq!(header(&jpeg(64, 64)[..26]), None);
        assert_eq!(header(&png(0, 360)), None);
        assert_eq!(header(b"GIF89a\x01\0\x01\0"), None);
        // A frame never comes before the image's data.
        assert_eq!(
            header(&[0xff, 0xd8, 0xff, 0xda, 0, 2, 0xff, 0xc0]),
            None
        );
        assert_eq!(header(&[]), None);
    }

    #[test]
    fn a_cover_too_large_is_refused() {
        assert!(check(&png(2048, 1152)).is_ok());
        assert!(check(&png(2049, 16)).is_err());
        assert!(check(&jpeg(16, 4096)).is_err());
        let mut heavy = png(64, 64);
        heavy.resize(MOST_BYTES + 1, 0);
        assert!(check(&heavy).is_err());
        assert!(check(b"not a picture").is_err());
    }
}
