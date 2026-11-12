//! The files the realm keeps for the lines chiefs send, each once
//! under the hash of its bytes (S29), and how they are taken in.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use base64::Engine as _;
use hamlet_protocol::client::{
    AttachedFile, FileId, PutFile, PutFilePart, ServerMessage,
};
use sha2::{Digest, Sha256};

use crate::clients::check::reply;
use crate::core::Core;
use crate::society::refusal::Refusal;

/// The time a file nothing uses is kept, and the time between the
/// realm's sweeps for such files.
const GRACE: Duration = Duration::from_secs(24 * 60 * 60);
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);
/// The most files one connection sends at once.
const MOST_UPLOADS: usize = 32;
/// The name of a file being taken in, until it is whole.
const INCOMING: &str = ".incoming-";

/// The realm's folder of files.
pub(crate) struct Files {
    folder: PathBuf,
}

impl Files {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    fn path(&self, id: &FileId) -> PathBuf {
        self.folder.join(id.as_str())
    }

    /// The bytes kept under `id`, if there are any.
    pub fn read(&self, id: &FileId) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(id)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// The size of the file kept under `id`, if there is one.
    pub fn size(&self, id: &FileId) -> Option<u64> {
        let meta = std::fs::metadata(self.path(id)).ok()?;
        meta.is_file().then_some(meta.len())
    }

    /// Keeps the file kept under `id` longer.
    pub fn touch(&self, id: &FileId) {
        let _ = File::options()
            .append(true)
            .open(self.path(id))
            .and_then(|file| file.set_modified(SystemTime::now()));
    }

    /// Every file kept, with when it was last written.
    fn kept(&self) -> Vec<(FileId, SystemTime)> {
        let Ok(entries) = std::fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                let id = entry
                    .file_name()
                    .into_string()
                    .ok()?
                    .parse()
                    .ok()?;
                Some((
                    id,
                    meta.modified()
                        .ok()
                        .filter(|_| meta.is_file())?,
                ))
            })
            .collect()
    }

    /// Takes away what was begun and never finished, and was last
    /// written a day ago.
    fn clear_incoming(&self) {
        let Ok(entries) = std::fs::read_dir(&self.folder) else {
            return;
        };
        for entry in entries.flatten() {
            let begun = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(INCOMING));
            let idle = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|at| {
                    at.elapsed().is_ok_and(|age| age >= GRACE)
                });
            if begun && idle {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// The files being sent, and when the realm last swept for those
/// nothing uses.
#[derive(Default)]
pub(crate) struct FileUploads {
    sending: HashMap<String, Sending>,
    swept: Option<Instant>,
}

struct Sending {
    client: u64,
    size: u64,
    received: u64,
    hasher: Sha256,
    file: File,
    path: PathBuf,
}

impl FileUploads {
    /// Drops what the connection `client` was sending.
    pub fn disconnect(&mut self, client: u64) {
        self.sending.retain(|_, sending| {
            let keep = sending.client != client;
            if !keep {
                let _ = std::fs::remove_file(&sending.path);
            }
            keep
        });
    }
}

fn usage(words: &str) -> Refusal {
    Refusal::Usage {
        words: words.into(),
    }
}

fn hex(hash: &[u8]) -> FileId {
    hash.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .parse()
        .expect("a SHA-256 is 64 lowercase hexadecimal digits")
}

impl Core {
    /// Begins a file of `size` bytes; the reply names the upload its
    /// parts are sent under. A file with no bytes is kept at once.
    pub(crate) fn put_file(
        &mut self,
        client: u64,
        put: PutFile,
    ) -> Result<ServerMessage> {
        let PutFile { id, size } = put;
        let sending = self
            .files
            .sending
            .values()
            .filter(|sending| sending.client == client)
            .count();
        let refusal = if size > AttachedFile::MOST_FILE {
            Some(usage("A file is at most 256 MiB."))
        } else if sending >= MOST_UPLOADS {
            Some(usage("Too many files are being sent at once."))
        } else {
            None
        };
        if let Some(refusal) = refusal {
            return Ok(reply(&id, Err(refusal)));
        }
        let folder = self.realm.layout().files();
        std::fs::create_dir_all(&folder).with_context(|| {
            format!("making {}", folder.display())
        })?;
        let upload = uuid::Uuid::new_v4().simple().to_string();
        let path = folder.join(format!("{INCOMING}{upload}"));
        let file = File::create(&path)
            .with_context(|| format!("making {}", path.display()))?;
        let sending = Sending {
            client,
            size,
            received: 0,
            hasher: Sha256::new(),
            file,
            path,
        };
        if size == 0 {
            let kept = self.finish_file(sending)?;
            let mut answer =
                reply(&id, Ok::<_, Refusal>("Kept the file.".into()));
            if let ServerMessage::Reply(answer) = &mut answer {
                answer.file = Some(kept);
            }
            return Ok(answer);
        }
        self.files.sending.insert(upload.clone(), sending);
        let mut answer = reply(
            &id,
            Ok::<_, Refusal>("Ready for the file.".into()),
        );
        if let ServerMessage::Reply(answer) = &mut answer {
            answer.upload = Some(upload);
        }
        Ok(answer)
    }

    /// Takes a part of a file, and keeps the file once whole.
    pub(crate) fn put_file_part(
        &mut self,
        client: u64,
        part: PutFilePart,
    ) -> Result<ServerMessage> {
        let refuse = |words: &str| reply(&part.id, Err(usage(words)));
        let Some(sending) = self
            .files
            .sending
            .get_mut(&part.upload)
            .filter(|sending| sending.client == client)
        else {
            return Ok(refuse(
                "No file of yours is being sent under that upload.",
            ));
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD
            .decode(&part.data)
        else {
            return Ok(refuse("A part's data is base64."));
        };
        if part.offset != sending.received {
            return Ok(refuse(&format!(
                "The file continues at byte {}.",
                sending.received
            )));
        }
        if sending.received + bytes.len() as u64 > sending.size {
            return Ok(refuse("The file is longer than declared."));
        }
        sending.file.write_all(&bytes)?;
        sending.hasher.update(&bytes);
        sending.received += bytes.len() as u64;
        if sending.received < sending.size {
            return Ok(reply(
                &part.id,
                Ok::<_, Refusal>("Received the file part.".into()),
            ));
        }
        let whole = self
            .files
            .sending
            .remove(&part.upload)
            .expect("the upload was found above");
        let kept = self.finish_file(whole)?;
        let mut answer = reply(
            &part.id,
            Ok::<_, Refusal>("Kept the file.".into()),
        );
        if let ServerMessage::Reply(answer) = &mut answer {
            answer.file = Some(kept);
        }
        Ok(answer)
    }

    /// Puts a whole file in its place under the hash of its bytes.
    fn finish_file(&self, sending: Sending) -> Result<FileId> {
        let Sending {
            hasher, file, path, ..
        } = sending;
        file.sync_all()?;
        drop(file);
        let id = hex(&hasher.finalize());
        let files = Files::new(self.realm.layout().files());
        if files.size(&id).is_some() {
            std::fs::remove_file(&path)?;
            files.touch(&id);
        } else {
            std::fs::rename(&path, files.path(&id))?;
        }
        Ok(id)
    }

    /// The files `files` of a line, each kept and of the size it is
    /// said to be, in no more than [`AttachedFile::MOST_FILES`] and
    /// [`AttachedFile::MOST_BYTES`], read back one at a time by
    /// the caller.
    pub(crate) fn check_files(
        &self,
        files: &[AttachedFile],
    ) -> std::result::Result<(), Refusal> {
        if files.len() > AttachedFile::MOST_FILES {
            return Err(usage("A line takes at most 10000 files."));
        }
        let kept = Files::new(self.realm.layout().files());
        let mut total = 0_u64;
        for file in files {
            let Some(size) = kept.size(&file.id) else {
                return Err(usage(&format!(
                    "The file `{}` was not sent first.",
                    file.name
                )));
            };
            if size != file.size {
                return Err(usage(&format!(
                    "The file `{}` is not the size it was sent as.",
                    file.name
                )));
            }
            total += size;
        }
        if total > AttachedFile::MOST_BYTES {
            return Err(usage(
                "A line takes at most 1 GiB of files.",
            ));
        }
        Ok(())
    }

    /// Keeps the files `files` longer.
    pub(crate) fn keep_files(&self, files: &[AttachedFile]) {
        let kept = Files::new(self.realm.layout().files());
        for file in files {
            kept.touch(&file.id);
        }
    }

    /// Takes away the files nothing uses any more, at most once an
    /// hour: those no kept input or transcript item names, untouched
    /// for a day, which includes uploads never sent with a line.
    pub(crate) fn sweep_files(&mut self) -> Result<()> {
        if self
            .files
            .swept
            .is_some_and(|at| at.elapsed() < SWEEP_EVERY)
        {
            return Ok(());
        }
        self.files.swept = Some(Instant::now());
        let files = Files::new(self.realm.layout().files());
        files.clear_incoming();
        let db = self.realm.store().connection();
        for (id, at) in files.kept() {
            let idle = at.elapsed().is_ok_and(|age| age >= GRACE);
            if idle && !crate::images::used(db, id.as_str())? {
                let _ = std::fs::remove_file(files.path(&id));
            }
        }
        Ok(())
    }
}
