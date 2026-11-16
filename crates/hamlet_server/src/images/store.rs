//! The realm's folder of images: each kept once under the hash of its
//! bytes, with a small copy made the first time one is requested, and
//! handed over to an agent's own folder of images.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use hamlet_protocol::client::ImageId;
use hamlet_protocol::runtime::Picture;
use sha2::{Digest, Sha256};

use super::fitting::{MOST_PIXELS, fit, picture, reader};
use super::{Format, Limits, MOST_BYTES, SEEN, Unusable};
use crate::knowledge::folder::Folder;

/// The longest side of a thumbnail, and the most bytes it takes.
const THUMBNAIL_SIDE: u32 = 320;
const THUMBNAIL_BYTES: u64 = 256 * 1024;

/// An image the realm keeps, and the format it was found in.
pub(crate) struct Stored {
    pub bytes: Vec<u8>,
    pub format: Format,
}

/// The realm's folder of images.
pub(crate) struct Images {
    folder: PathBuf,
}

impl Images {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    fn name(id: &ImageId, format: Format) -> String {
        format!("{id}.{}", format.extension())
    }

    /// Keeps `bytes` under the hash of them, once.
    pub fn keep(
        &self,
        bytes: &[u8],
    ) -> Result<std::result::Result<(ImageId, Format), Unusable>>
    {
        if bytes.len() > MOST_BYTES {
            return Ok(Err(Unusable::TooLarge));
        }
        let Some(format) = Format::detect(bytes) else {
            return Ok(Err(Unusable::NotAnImage));
        };
        let Ok(decoder) = reader(bytes, format).into_decoder() else {
            return Ok(Err(Unusable::Broken));
        };
        let (width, height) =
            image::ImageDecoder::dimensions(&decoder);
        if u64::from(width) * u64::from(height) > MOST_PIXELS {
            return Ok(Err(Unusable::TooManyPixels));
        }
        let hash = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let id = hash
            .parse::<ImageId>()
            .expect("a SHA-256 is 64 lowercase hexadecimal digits");
        Folder::make(&self.folder)?.write(
            &Self::name(&id, format),
            bytes,
            false,
        )?;
        Ok(Ok((id, format)))
    }

    /// The image kept under `id`, if there is one.
    pub fn find(&self, id: &ImageId) -> Result<Option<Stored>> {
        Self::find_in(&self.folder, id)
    }

    fn find_in(
        folder: &Path,
        id: &ImageId,
    ) -> Result<Option<Stored>> {
        let Ok(folder) = Folder::open(folder) else {
            return Ok(None);
        };
        for format in Format::ALL {
            if let Ok(bytes) = folder.read(&Self::name(id, format)) {
                return Ok(Some(Stored { bytes, format }));
            }
        }
        Ok(None)
    }

    fn thumbnails(&self) -> PathBuf {
        self.folder.join("thumbnails")
    }

    /// The thumbnail of the image kept under `id`, made and kept the
    /// first time it is requested; the image itself when it is
    /// small enough already.
    pub fn thumbnail(&self, id: &ImageId) -> Result<Option<Stored>> {
        if let Some(kept) = Self::find_in(&self.thumbnails(), id)? {
            return Ok(Some(kept));
        }
        let Some(original) = self.find(id)? else {
            return Ok(None);
        };
        let limits = Limits {
            width: THUMBNAIL_SIDE,
            height: THUMBNAIL_SIDE,
            bytes: THUMBNAIL_BYTES,
            per_line: 1,
        };
        let Ok(made) = fit(&original.bytes, original.format, limits)
        else {
            return Ok(Some(original));
        };
        if made.bytes == original.bytes {
            return Ok(Some(original));
        }
        Folder::make(&self.thumbnails())?.write(
            &Self::name(id, made.format),
            &made.bytes,
            false,
        )?;
        Ok(Some(Stored {
            bytes: made.bytes,
            format: made.format,
        }))
    }

    /// Writes `stored`, the image kept under `id`, again, so it is
    /// kept longer.
    pub fn touch(&self, id: &ImageId, stored: &Stored) -> Result<()> {
        Folder::make(&self.folder)?.write(
            &Self::name(id, stored.format),
            &stored.bytes,
            false,
        )
    }

    /// Copies `stored`, the image kept under `id`, into the folder
    /// of images in `lodge`, and gives the name it has there.
    pub fn hand_over(
        &self,
        lodge: &Path,
        id: &ImageId,
        stored: &Stored,
    ) -> Result<String> {
        self.touch(id, stored)?;
        let name = Self::name(id, stored.format);
        Folder::make(lodge)?.write(
            &format!("{SEEN}/{name}"),
            &stored.bytes,
            false,
        )?;
        Ok(name)
    }

    /// Every image kept, with when it was last written.
    pub fn kept(&self) -> Vec<(ImageId, SystemTime)> {
        let Ok(entries) = std::fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                let name = entry.file_name().into_string().ok()?;
                let id = name.split_once('.')?.0.parse().ok()?;
                Some((
                    id,
                    meta.modified()
                        .ok()
                        .filter(|_| meta.is_file())?,
                ))
            })
            .collect()
    }

    /// Takes away the image kept under `id` with its thumbnail.
    pub fn remove(&self, id: &ImageId) -> Result<()> {
        for folder in [self.folder.clone(), self.thumbnails()] {
            let Ok(folder) = Folder::open(&folder) else {
                continue;
            };
            for format in Format::ALL {
                folder.remove(&Self::name(id, format))?;
            }
        }
        Ok(())
    }

    /// `id` as a model is given it, made to fit `limits`.
    pub fn picture(
        &self,
        id: &ImageId,
        limits: Limits,
    ) -> Result<Picture> {
        let stored = self
            .find(id)?
            .with_context(|| format!("the image {id} is not kept"))?;
        let fitted = fit(&stored.bytes, stored.format, limits)
            .map_err(|unusable| anyhow::anyhow!("{unusable}"))?;
        Ok(picture(&fitted.bytes, fitted.format))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_is_kept_once_under_the_hash_of_its_bytes() {
        let dir = hamlet_testing::Scratch::new("server-images");
        let images = Images::new(dir.join("images"));
        let bytes = crate::testing::png(8, 8);
        let (id, format) = images.keep(&bytes).unwrap().unwrap();
        assert_eq!(images.keep(&bytes).unwrap().unwrap().0, id);
        let stored = images.find(&id).unwrap().unwrap();
        assert_eq!((stored.bytes, stored.format), (bytes, format));
        assert_eq!(
            images.keep(b"not an image").unwrap(),
            Err(Unusable::NotAnImage)
        );
    }
}
