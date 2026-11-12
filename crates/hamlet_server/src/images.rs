//! The images the realm keeps, each once under the hash of its bytes,
//! and the making of one to fit a model (S29, S30).

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use base64::Engine as _;
use diesel::dsl::exists;
use diesel::prelude::*;
use hamlet_protocol::client::{
    AttachedFile, AttachedImage, ImageBytes, ImageId, ImageInput,
    ImageSize, PutImage, PutImagePart, ReadResult, RequestId,
    ServerMessage,
};
use hamlet_protocol::runtime::Picture;
use hamlet_store::Db;
use hamlet_store::ids::AgentId;
use hamlet_store::schema::{input, item, marker};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder,
    ImageReader, Limits as DecodeLimits,
};
use sha2::{Digest, Sha256};

use crate::attached::{Attached, Caption, is_place};
use crate::clients::check::reply;
use crate::core::Core;
use crate::diagnostic::{Secrets, tell};
use crate::files::Files;
use crate::inbox::{INBOX, write_remote};
use crate::knowledge::folder::Folder;
use crate::layout::Layout;
use crate::society::refusal::Refusal;
use crate::talk::models;
use crate::workspace::lodges::LODGE;

/// The most bytes of an image the realm keeps.
pub(crate) const MOST_BYTES: usize = 20 << 20;
/// The most pixels of an image that is opened.
const MOST_PIXELS: u64 = 64_000_000;
/// The JPEG quality an image is first made at, and the least.
const QUALITY: u8 = 80;
const LEAST_QUALITY: u8 = 40;
/// The shortest side an image is made down to.
const LEAST_SIDE: u32 = 32;
/// The longest side of a thumbnail, and the most bytes it takes.
const THUMBNAIL_SIDE: u32 = 320;
const THUMBNAIL_BYTES: u64 = 256 * 1024;
/// The folder of a lodge, or of a worker's scratch folder, that holds
/// the images given to its agent (S29).
pub(crate) const SEEN: &str = "inbox/images";
/// The time an image nothing uses is kept, and the time between the
/// realm's sweeps for such images.
const GRACE: Duration = Duration::from_secs(24 * 60 * 60);
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);

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
                && be32(bytes, 8) == 13;
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

fn be32(bytes: &[u8], offset: usize) -> usize {
    bytes.get(offset..offset + 4).map_or(0, |b| {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize
    })
}

/// Why bytes are not an image the realm can keep or give a model.
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

/// An image made to fit a model.
pub(crate) struct Fitted {
    pub bytes: Vec<u8>,
    pub format: Format,
    /// Its size before it was made smaller, when it was.
    pub from: Option<(u32, u32)>,
    pub size: (u32, u32),
}

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
        let (width, height) = decoder.dimensions();
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
    /// first time it is asked for; the image itself when it is
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
    fn touch(&self, id: &ImageId, stored: &Stored) -> Result<()> {
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
    fn kept(&self) -> Vec<(ImageId, SystemTime)> {
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
    fn remove(&self, id: &ImageId) -> Result<()> {
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

/// The images `ids` as a model is given them, made to fit `limits`
/// on a thread of their own. An image that cannot be is left out.
pub(crate) async fn pictures(
    folder: PathBuf,
    ids: Vec<ImageId>,
    limits: Limits,
) -> Result<Vec<Picture>> {
    Ok(tokio::task::spawn_blocking(move || {
        let images = Images::new(folder);
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
            .collect::<Vec<_>>()
    })
    .await?)
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
fn picture(bytes: &[u8], format: Format) -> Picture {
    Picture {
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
        mime: format.mime().into(),
    }
}

fn reader(
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

/// The most images one connection sends at once.
const MOST_UPLOADS: usize = 32;

/// The images being sent, and when the realm last swept for those
/// nothing uses.
#[derive(Default)]
pub(crate) struct Uploads {
    sending: HashMap<String, Sending>,
    swept: Option<Instant>,
}

struct Sending {
    client: u64,
    size: u64,
    bytes: Vec<u8>,
}

impl Uploads {
    /// Drops what the connection `client` was sending.
    pub fn disconnect(&mut self, client: u64) {
        self.sending.retain(|_, sending| sending.client != client);
    }
}

fn usage(words: &str) -> Refusal {
    Refusal::Usage {
        words: words.into(),
    }
}

impl Core {
    /// Begins an image of `size` bytes; the reply names the upload
    /// its parts are sent under.
    pub(crate) fn put_image(
        &mut self,
        client: u64,
        put: PutImage,
    ) -> ServerMessage {
        let PutImage { id, size } = put;
        let sending = self
            .images
            .sending
            .values()
            .filter(|sending| sending.client == client)
            .count();
        let refusal = if size == 0 {
            Some(usage("An image has bytes."))
        } else if size > MOST_BYTES as u64 {
            Some(usage("An image is at most 20 MiB."))
        } else if sending >= MOST_UPLOADS {
            Some(usage("Too many images are being sent at once."))
        } else {
            None
        };
        if let Some(refusal) = refusal {
            return reply(&id, Err(refusal));
        }
        let upload = uuid::Uuid::new_v4().simple().to_string();
        self.images.sending.insert(
            upload.clone(),
            Sending {
                client,
                size,
                bytes: Vec::new(),
            },
        );
        let mut answer = reply(
            &id,
            Ok::<_, Refusal>("Ready for the image.".into()),
        );
        if let ServerMessage::Reply(answer) = &mut answer {
            answer.upload = Some(upload);
        }
        answer
    }

    /// Takes a part of an image, and keeps the image once whole.
    pub(crate) fn put_image_part(
        &mut self,
        client: u64,
        part: PutImagePart,
    ) -> Result<ServerMessage> {
        let refuse = |words: &str| reply(&part.id, Err(usage(words)));
        let Some(sending) = self
            .images
            .sending
            .get_mut(&part.upload)
            .filter(|sending| sending.client == client)
        else {
            return Ok(refuse(
                "No image of yours is being sent under that upload.",
            ));
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD
            .decode(&part.data)
        else {
            return Ok(refuse("A part's data is base64."));
        };
        let received = sending.bytes.len() as u64;
        if part.offset != received {
            return Ok(refuse(&format!(
                "The image continues at byte {received}."
            )));
        }
        if received + bytes.len() as u64 > sending.size {
            return Ok(refuse("The image is longer than declared."));
        }
        sending.bytes.extend_from_slice(&bytes);
        if (sending.bytes.len() as u64) < sending.size {
            return Ok(reply(
                &part.id,
                Ok::<_, Refusal>("Received the image part.".into()),
            ));
        }
        let whole = self
            .images
            .sending
            .remove(&part.upload)
            .expect("the upload was found above");
        let folder = Images::new(self.realm.layout().images());
        Ok(match folder.keep(&whole.bytes)? {
            Ok((image, _)) => {
                let mut answer = reply(
                    &part.id,
                    Ok::<_, Refusal>("Kept the image.".into()),
                );
                if let ServerMessage::Reply(answer) = &mut answer {
                    answer.image = Some(image);
                }
                answer
            }
            Err(unusable) => {
                refuse(&capitalised(&unusable.to_string()))
            }
        })
    }

    /// The images `images` and the files `files` of the line `text`
    /// to `agent`, each at its place in the words and in the order
    /// the model is given them, with the captions that tell where
    /// they are saved (C33). The images are the model's too, if it
    /// takes them (S30), and are saved in the agent's own folder of
    /// images; the files are saved in its inbox. The remote
    /// workspace holds both when the agent works there.
    pub(crate) async fn attach(
        &mut self,
        agent: AgentId,
        text: &str,
        images: &[AttachedImage],
        files: &[AttachedFile],
    ) -> Result<std::result::Result<Attached, Refusal>> {
        let places = images
            .iter()
            .filter_map(|image| image.at)
            .chain(files.iter().filter_map(|file| file.at));
        for at in places {
            if !is_place(text, at) {
                return Ok(Err(usage(&format!(
                    "An attachment's place, {at}, is not a byte \
                     offset on a character boundary of the line, \
                     which is {} bytes.",
                    text.len()
                ))));
            }
        }
        if let Err(refusal) = self.check_files(files) {
            return Ok(Err(refusal));
        }
        let mut images = images.to_vec();
        images.sort_by_key(|image| image.at.unwrap_or(u64::MAX));
        let mut files = files.to_vec();
        files.sort_by_key(|file| file.at.unwrap_or(u64::MAX));
        let mut attached =
            match self.attach_images(agent, &images).await? {
                Ok(attached) => attached,
                Err(refusal) => return Ok(Err(refusal)),
            };
        if files.is_empty() {
            return Ok(Ok(attached));
        }
        let kept = Files::new(self.realm.layout().files());
        let mut inbox = self.open_inbox(agent, INBOX)?;
        for file in &files {
            let Some(bytes) = kept.read(&file.id)? else {
                return Ok(Err(usage(&format!(
                    "The file `{}` was not sent first.",
                    file.name
                ))));
            };
            if let Err(refusal) =
                inbox.put_at(&file.name, &bytes, file.at).await?
            {
                return Ok(Err(refusal));
            }
        }
        attached
            .captions
            .extend(inbox.placed_captions("of this message"));
        attached.files = files;
        Ok(Ok(attached))
    }

    /// The images `images` of a line to `agent`, if its model takes
    /// them (S30), with the captions that tell where they are saved:
    /// in the agent's own folder of images.
    async fn attach_images(
        &mut self,
        agent: AgentId,
        images: &[AttachedImage],
    ) -> Result<std::result::Result<Attached, Refusal>> {
        if images.is_empty() {
            return Ok(Ok(Attached::default()));
        }
        let db = self.realm.store().connection();
        let model = models::choose(
            db,
            &self.env.settings,
            &self.env.overrides,
            agent,
        )?
        .model;
        let named = format!("{}::{}", model.provider, model.id);
        let Some(takes) = self.providers.images(db, &model)? else {
            return Ok(Err(Refusal::NoImages { model: named }));
        };
        let most = Limits::of(&takes).per_line;
        if images.len() > most as usize {
            return Ok(Err(Refusal::TooManyImages {
                model: named,
                most,
            }));
        }
        let layout = self.realm.layout().clone();
        let lodge = crate::calls::place::of(
            self.realm.store().connection(),
            &layout,
            agent,
        )?
        .place
        .lodge;
        let remote = self.remote_session(agent, false)?;
        let kept = Images::new(layout.images());
        let mut attached = Attached::default();
        for (index, image) in images.iter().enumerate() {
            let Some(stored) = kept.find(&image.id)? else {
                return Ok(Err(Refusal::NoImage {
                    image: image.id.to_string(),
                }));
            };
            let name = kept.hand_over(&lodge, &image.id, &stored)?;
            let saved = match &remote {
                None => {
                    lodge.join(SEEN).join(&name).display().to_string()
                }
                Some(session) => {
                    let path = format!("{LODGE}/{SEEN}/{name}");
                    if let Err(failure) =
                        write_remote(session, &path, &stored.bytes)
                            .await
                    {
                        return Ok(Err(usage(&format!(
                            "The workspace could not take the \
                             image: {failure}"
                        ))));
                    }
                    path
                }
            };
            attached.captions.push(Caption {
                at: image.at,
                words: format!(
                    "[Image {} of this message, saved at {saved}]",
                    index + 1
                ),
            });
            attached.images.push(image.clone());
        }
        Ok(Ok(attached))
    }

    /// Keeps the images `images` longer.
    pub(crate) fn keep_images(&mut self, images: &[AttachedImage]) {
        let kept = Images::new(self.realm.layout().images());
        for image in images {
            if let Ok(Some(stored)) = kept.find(&image.id) {
                let _ = kept.touch(&image.id, &stored);
            }
        }
    }

    /// The image `image` as `size` says, for the client that asked,
    /// made on a thread of its own.
    pub(crate) async fn read_image(
        &mut self,
        id: &RequestId,
        image: &ImageId,
        size: ImageSize,
    ) -> Result<ServerMessage> {
        let folder = self.realm.layout().images();
        let wanted = image.clone();
        let found = tokio::task::spawn_blocking(move || {
            let images = Images::new(folder);
            match size {
                ImageSize::Original => images.find(&wanted),
                ImageSize::Thumbnail => images.thumbnail(&wanted),
            }
        })
        .await??;
        Ok(match found {
            Some(stored) => ServerMessage::Read(ReadResult::Image {
                image: ImageBytes {
                    data: base64::engine::general_purpose::STANDARD
                        .encode(&stored.bytes),
                    mime: stored.format.mime().into(),
                },
                to: id.clone(),
            }),
            None => reply(
                id,
                Err(Refusal::NoImage {
                    image: image.to_string(),
                }),
            ),
        })
    }

    /// Takes away the images nothing uses any more, at most once an
    /// hour: those no kept input, transcript item or tool result
    /// names and no agent's folder holds, untouched for a day, which
    /// includes uploads never sent with a line.
    pub(crate) fn sweep_images(&mut self) -> Result<()> {
        if self
            .images
            .swept
            .is_some_and(|at| at.elapsed() < SWEEP_EVERY)
        {
            return Ok(());
        }
        self.images.swept = Some(Instant::now());
        let layout = self.realm.layout().clone();
        let images = Images::new(layout.images());
        let held = held_by_agents(&layout);
        let db = self.realm.store().connection();
        for (id, at) in images.kept() {
            let idle = at.elapsed().is_ok_and(|age| age >= GRACE);
            if idle && !held.contains(&id) && !used(db, id.as_str())?
            {
                images.remove(&id)?;
            }
        }
        Ok(())
    }
}

/// Whether a kept input, a transcript item or a tool's result names
/// the image or file `image`.
pub(crate) fn used(db: &mut Db, image: &str) -> Result<bool> {
    let named = format!("%{image}%");
    Ok(diesel::select(exists(
        input::table.filter(input::mark.like(&named)),
    ))
    .get_result::<bool>(db)?
        || diesel::select(exists(
            item::table.filter(item::body.like(&named)),
        ))
        .get_result::<bool>(db)?
        || diesel::select(exists(
            marker::table.filter(marker::outcome.like(&named)),
        ))
        .get_result::<bool>(db)?)
}

/// The images the folders of the agents' images hold.
fn held_by_agents(layout: &Layout) -> HashSet<ImageId> {
    let children = |path: PathBuf| {
        std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .collect::<Vec<_>>()
    };
    let mut lodges = vec![layout.hand()];
    lodges.extend(children(layout.workers()));
    for hamlet in children(layout.hamlets()) {
        lodges.extend(children(hamlet));
    }
    lodges
        .into_iter()
        .flat_map(|lodge| children(lodge.join(SEEN)))
        .filter_map(|file| {
            file.file_name()?
                .to_str()?
                .split_once('.')?
                .0
                .parse()
                .ok()
        })
        .collect()
}

/// `words` with its first letter in capitals.
fn capitalised(words: &str) -> String {
    let mut letters = words.chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
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

    #[test]
    fn an_image_is_kept_once_under_the_hash_of_its_bytes() {
        let dir = hamlet_testing::Scratch::new("server-images");
        let images = Images::new(dir.join("images"));
        let bytes = png(8, 8);
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
