//! A chief's uploads: the images and files a line carries, kept in
//! the realm's store of parcels under the SHA-256 of their bytes.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use anyhow::{Context as _, Result};
use base64::Engine as _;
use hamlet_actors::realm::ParcelStore;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::chat::ImagesTaken;
use hamlet_messages::bytes::Sha256;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, AttachedFile, RequestId, ServerMessage,
};
use sha2::Digest as _;

use super::call::Calls;
use super::route::Upload;
use super::view::{self, Own};
use crate::clients::check::reply;
use crate::images::{
    Format, MOST_BYTES, Unusable, capitalised, check,
};
use crate::society::refusal::Refusal;

/// The most uploads of one kind one connection sends at once.
const MOST_UPLOADS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Image,
    File,
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::File => "file",
        }
    }
}

/// An upload whose parts are coming.
struct Sending {
    client: u64,
    kind: Kind,
    size: u64,
    received: u64,
    hasher: sha2::Sha256,
    file: File,
    /// Its file in the store until it is whole, under a name of the
    /// store's form that no bytes hash to: the store's sweep takes
    /// it away a day after its last part if it is never finished.
    path: PathBuf,
}

/// The uploads the connections are sending, into the realm's store
/// of parcels.
pub(crate) struct Uploads {
    store: ParcelStore,
    sending: Mutex<HashMap<String, Sending>>,
}

fn usage(words: &str) -> Refusal {
    Refusal::Usage {
        words: words.into(),
    }
}

fn notice(
    id: &RequestId,
    words: &str,
    fill: impl FnOnce(&mut protocol::Reply),
) -> ServerMessage {
    let mut answer = reply(id, Ok(words.to_owned()));
    if let ServerMessage::Reply(answer) = &mut answer {
        fill(answer);
    }
    answer
}

impl Uploads {
    /// The uploads of the realm whose host keeps its actors under
    /// `root`.
    pub(crate) fn at(root: &Path) -> Result<Self> {
        Ok(Self {
            store: ParcelStore::at(root)?,
            sending: Mutex::default(),
        })
    }

    pub(crate) fn store(&self) -> &ParcelStore {
        &self.store
    }

    fn sending(&self) -> MutexGuard<'_, HashMap<String, Sending>> {
        self.sending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn disconnect(&self, client: u64) {
        self.sending().retain(|_, sending| {
            let theirs = sending.client == client;
            if theirs {
                let _ = std::fs::remove_file(&sending.path);
            }
            !theirs
        });
    }

    pub(crate) fn take(
        &self,
        client: u64,
        upload: Upload,
    ) -> Result<ServerMessage> {
        match upload {
            Upload::Image(protocol::PutImage { id, size }) => {
                let refusal = if size == 0 {
                    Some("An image has bytes.")
                } else if size > MOST_BYTES as u64 {
                    Some("An image is at most 20 MiB.")
                } else {
                    None
                };
                self.begin(client, Kind::Image, &id, size, refusal)
            }
            Upload::File(protocol::PutFile { id, size }) => {
                let refusal = (size > AttachedFile::MOST_FILE)
                    .then_some("A file is at most 256 MiB.");
                self.begin(client, Kind::File, &id, size, refusal)
            }
            Upload::ImagePart(part) => self.part(
                client,
                Kind::Image,
                &part.id,
                &part.upload,
                part.offset,
                &part.data,
            ),
            Upload::FilePart(part) => self.part(
                client,
                Kind::File,
                &part.id,
                &part.upload,
                part.offset,
                &part.data,
            ),
        }
    }

    /// Begins an upload of `size` bytes, unless `refusal` refuses it;
    /// the reply names the upload its parts are sent under. A file
    /// with no bytes is kept at once.
    fn begin(
        &self,
        client: u64,
        kind: Kind,
        id: &RequestId,
        size: u64,
        refusal: Option<&str>,
    ) -> Result<ServerMessage> {
        let mut sending = self.sending();
        let busy = sending
            .values()
            .filter(|open| open.client == client && open.kind == kind)
            .count()
            >= MOST_UPLOADS;
        let many = format!(
            "Too many {}s are being sent at once.",
            kind.noun()
        );
        if let Some(words) =
            refusal.or_else(|| busy.then_some(many.as_str()))
        {
            return Ok(reply(id, Err(usage(words))));
        }
        let folder = self.store.folder();
        std::fs::create_dir_all(folder).with_context(|| {
            format!("making {}", folder.display())
        })?;
        let path = folder.join(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        let file = File::create(&path)
            .with_context(|| format!("making {}", path.display()))?;
        let open = Sending {
            client,
            kind,
            size,
            received: 0,
            hasher: sha2::Sha256::new(),
            file,
            path,
        };
        if size == 0 {
            return self.finish(id, open);
        }
        let upload = uuid::Uuid::new_v4().simple().to_string();
        sending.insert(upload.clone(), open);
        let ready = format!("Ready for the {}.", kind.noun());
        Ok(notice(id, &ready, |answer| answer.upload = Some(upload)))
    }

    /// Takes a part of an upload, and keeps the upload once whole.
    fn part(
        &self,
        client: u64,
        kind: Kind,
        id: &RequestId,
        upload: &str,
        offset: u64,
        data: &str,
    ) -> Result<ServerMessage> {
        let noun = kind.noun();
        let refuse = |words: &str| Ok(reply(id, Err(usage(words))));
        let mut sending = self.sending();
        let Some(open) = sending.get_mut(upload).filter(|open| {
            open.client == client && open.kind == kind
        }) else {
            return refuse(&format!(
                "No {noun} of yours is being sent under that upload."
            ));
        };
        let Ok(bytes) =
            base64::engine::general_purpose::STANDARD.decode(data)
        else {
            return refuse("A part's data is base64.");
        };
        if offset != open.received {
            return refuse(&format!(
                "The {noun} continues at byte {}.",
                open.received
            ));
        }
        if open.received + bytes.len() as u64 > open.size {
            return refuse(&format!(
                "The {noun} is longer than declared."
            ));
        }
        open.file.write_all(&bytes)?;
        open.hasher.update(&bytes);
        open.received += bytes.len() as u64;
        if open.received < open.size {
            return Ok(reply(
                id,
                Ok(format!("Received the {noun} part.")),
            ));
        }
        let whole =
            sending.remove(upload).context("the upload was found")?;
        drop(sending);
        self.finish(id, whole)
    }

    /// Puts a whole upload in its place under the hash of its bytes,
    /// once an image is found to be one.
    fn finish(
        &self,
        id: &RequestId,
        whole: Sending,
    ) -> Result<ServerMessage> {
        let Sending {
            kind,
            hasher,
            file,
            path,
            ..
        } = whole;
        file.sync_all()?;
        drop(file);
        if kind == Kind::Image {
            let bytes = std::fs::read(&path)?;
            let usable = Format::detect(&bytes)
                .ok_or(Unusable::NotAnImage)
                .and_then(|format| check(&bytes, format));
            if let Err(unusable) = usable {
                std::fs::remove_file(&path)?;
                return Ok(reply(
                    id,
                    Err(usage(&capitalised(&unusable.to_string()))),
                ));
            }
        }
        let sha256 = Sha256(hasher.finalize().into());
        let kept = self.store.file(&sha256);
        if kept.is_file() {
            std::fs::remove_file(&path)?;
            File::options()
                .append(true)
                .open(&kept)?
                .set_modified(SystemTime::now())?;
        } else {
            std::fs::rename(&path, &kept)?;
        }
        let hex = sha256.to_string();
        let words = format!("Kept the {}.", kind.noun());
        Ok(notice(id, &words, |answer| match kind {
            Kind::Image => answer.image = hex.parse().ok(),
            Kind::File => answer.file = hex.parse().ok(),
        }))
    }
}

/// Why the line `say` to `agent` is refused before the realm is
/// given it: images to a model that takes none, or more than it takes
/// in one line.
pub(crate) async fn refusal(
    calls: &Calls,
    own: &Arc<Own>,
    say: &protocol::Say,
    agent: AgentNumber,
) -> Result<Option<Refusal>> {
    if say.images.is_empty() {
        return Ok(None);
    }
    let taken = calls.call(&Agent(agent), &ImagesTaken).await?;
    let Some(refused) = too_many(say.images.len(), taken.per_line)
    else {
        return Ok(None);
    };
    let view = view::read(calls, own).await?;
    let model = view
        .agent(agent)
        .and_then(|seen| seen.runs.clone())
        .unwrap_or_default();
    Ok(Some(match refused {
        None => Refusal::NoImages { model },
        Some(most) => Refusal::TooManyImages { model, most },
    }))
}

/// The most a model takes in a line when `images` are more than it
/// takes: none when it takes no images at all.
fn too_many(
    images: usize,
    per_line: Option<u32>,
) -> Option<Option<u32>> {
    match per_line {
        Some(most) if images <= most as usize => None,
        taken => Some(taken),
    }
}

#[cfg(test)]
mod tests {
    use super::too_many;

    #[test]
    fn a_line_is_refused_past_what_its_model_takes() {
        assert_eq!(too_many(20, Some(20)), None);
        assert_eq!(too_many(21, Some(20)), Some(Some(20)));
        assert_eq!(too_many(1, None), Some(None));
    }
}
