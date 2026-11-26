//! The images the realm keeps, each once under the hash of its bytes,
//! and given to a model made to fit it (S29, S30).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use base64::Engine as _;
use diesel::dsl::exists;
use diesel::prelude::*;
pub(crate) use hamlet_actors::workspace::image::{
    Bounds, Fitted, Format, Unusable, check, fit,
};
use hamlet_protocol::client::{
    AttachedFile, AttachedImage, ImageBytes, ImageId, ImageInput,
    ImageSize, PutImage, PutImagePart, ReadResult, RequestId,
    ServerMessage,
};
use hamlet_protocol::runtime::Picture;
use hamlet_store::Db;
use hamlet_store::ids::AgentId;
use hamlet_store::schema::{input, item, marker};
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

    pub fn bounds(self) -> Bounds {
        Bounds {
            width: self.width,
            height: self.height,
            bytes: self.bytes,
        }
    }
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
        if let Err(unusable) = check(bytes, format) {
            return Ok(Err(unusable));
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
        let bounds = Bounds {
            width: THUMBNAIL_SIDE,
            height: THUMBNAIL_SIDE,
            bytes: THUMBNAIL_BYTES,
        };
        let Ok(made) = fit(&original.bytes, original.format, bounds)
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
        let fitted =
            fit(&stored.bytes, stored.format, limits.bounds())
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
        let fitted = match fit(&bytes, format, limits.bounds()) {
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
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder};

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
