//! Files and folders a chief attaches to a line (C33): walked and
//! read on the runtime's blocking threads, which wake the window for
//! the update that collects them, and uploaded in parts, a few files
//! at a time. Nothing is read in a frame.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use base64::Engine;
use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use hamlet_protocol::client as protocol;
use protocol::AttachedFile;
use tokio::sync::oneshot;

use crate::Tokio;
use crate::connection::Wake;
use crate::messages::{Reconnected, Response};
use crate::outbox::{Outbox, RequestId};

/// The most bytes of a file sent in one part.
const PART: u64 = 512 * 1024;

/// The files of one attachment sent at once.
const AT_ONCE: usize = 4;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Packs>()
        .init_resource::<Steps>()
        .init_resource::<Reading>()
        .add_observer(|gather: On<Gather>, mut commands: Commands| {
            let Gather { key, paths } = gather.event().clone();
            commands.queue(move |world: &mut World| {
                walk_off_frame(world, key, paths);
            });
        })
        .add_observer(
            |go: On<UploadFiles>, mut commands: Commands| {
                let key = go.key;
                commands.queue(move |world: &mut World| {
                    begin(world, key);
                });
            },
        )
        .add_observer(
            |forget: On<FilesForget>, mut commands: Commands| {
                let key = forget.key;
                commands.queue(move |world: &mut World| {
                    drop_pack(world, key);
                });
            },
        )
        .add_observer(
            |response: On<Response>, mut commands: Commands| {
                let response = response.event().clone();
                commands.queue(move |world: &mut World| {
                    answered(world, response);
                });
            },
        )
        .add_observer(lost)
        .add_systems(Update, collect.run_if(reading));
}

/// A request to count the files and folders `paths`, which the
/// caller names `key`: nothing is sent.
#[derive(Event, Clone, Debug)]
pub struct Gather {
    pub key: u64,
    pub paths: Vec<PathBuf>,
}

/// The files and folders gathered for `key` hold `files` files of
/// `bytes` bytes in all. A link inside a folder, and what is not a
/// file or a folder, is left out and counted in `skipped`.
#[derive(Event, Clone, Debug)]
pub struct Gathered {
    pub key: u64,
    pub files: usize,
    pub bytes: u64,
    pub skipped: usize,
}

/// A request to upload what was gathered for `key`.
#[derive(Event, Clone, Copy, Debug)]
pub struct UploadFiles {
    pub key: u64,
}

/// A request to let go of what was gathered for `key`, and to stop
/// sending it.
#[derive(Event, Clone, Copy, Debug)]
pub struct FilesForget {
    pub key: u64,
}

/// The server kept every file gathered for `key`, as these.
#[derive(Event, Clone, Debug)]
pub struct FilesKept {
    pub key: u64,
    pub files: Vec<AttachedFile>,
}

/// The files gathered for `key` could not be read or sent, or the
/// connection was lost on the way.
#[derive(Event, Clone, Debug)]
pub struct FilesRefused {
    pub key: u64,
    pub text: String,
}

/// A file to send, and where it is read from.
#[derive(Clone, Debug)]
struct Planned {
    index: usize,
    /// The name it is sent under.
    name: String,
    /// The folder it is opened in, and its path from there.
    base: PathBuf,
    rel: String,
    size: u64,
}

enum Stage {
    /// Its folders are being walked, off the frame.
    Walking,
    /// Counted, and waiting to be sent.
    Waiting(Vec<Planned>),
    Going(Going),
}

struct Going {
    queue: VecDeque<Planned>,
    /// The files begun and not yet kept.
    flying: usize,
    kept: Vec<(usize, AttachedFile)>,
}

/// The files gathered for each key, and how far each is.
#[derive(Resource, Default)]
struct Packs(HashMap<u64, Stage>);

enum Step {
    /// The file is announced; the server has not named its upload.
    Begin { key: u64, file: Planned },
    /// The part of the upload that ends at `next` is sent.
    Part {
        key: u64,
        file: Planned,
        upload: String,
        next: u64,
    },
}

impl Step {
    fn key(&self) -> u64 {
        match self {
            Step::Begin { key, .. } | Step::Part { key, .. } => *key,
        }
    }
}

/// The step of each request the files wait on.
#[derive(Resource, Default)]
struct Steps(HashMap<RequestId, Step>);

/// Work off the frame, to be collected by the update it wakes.
#[derive(Resource, Default)]
struct Reading(Vec<Pending>);

struct Walked {
    files: Vec<Planned>,
    skipped: usize,
}

enum Pending {
    Walk {
        key: u64,
        done: oneshot::Receiver<io::Result<Walked>>,
    },
    Part {
        key: u64,
        file: Planned,
        upload: String,
        offset: u64,
        done: oneshot::Receiver<io::Result<Vec<u8>>>,
    },
}

fn reading(reading: Res<Reading>) -> bool {
    !reading.0.is_empty()
}

/// `work` run on the runtime's blocking threads, its result to be
/// collected by the update it wakes the window for.
fn off_frame<T: Send + 'static>(
    world: &World,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<oneshot::Receiver<T>> {
    let Tokio(runtime) = world.get_resource::<Tokio>()?;
    let wake = world.get_resource::<Wake>().cloned();
    let (sender, done) = oneshot::channel();
    runtime.spawn_blocking(move || {
        // A receiver dropped on reconnection no longer wants it, nor
        // the update that would collect it.
        if sender.send(work()).is_ok()
            && let Some(wake) = wake
        {
            wake.call();
        }
    });
    Some(done)
}

fn refuse(world: &mut World, key: u64, text: impl Into<String>) {
    drop_pack(world, key);
    world.trigger(FilesRefused {
        key,
        text: text.into(),
    });
}

/// Lets go of what was gathered for `key`; the answers still to
/// come for it find nothing waiting.
fn drop_pack(world: &mut World, key: u64) {
    world.resource_mut::<Packs>().0.remove(&key);
    world
        .resource_mut::<Steps>()
        .0
        .retain(|_, step| step.key() != key);
}

fn walk_off_frame(world: &mut World, key: u64, paths: Vec<PathBuf>) {
    let Some(done) = off_frame(world, move || walk(&paths)) else {
        refuse(world, key, "The window has no runtime to read with.");
        return;
    };
    world.resource_mut::<Packs>().0.insert(key, Stage::Walking);
    world
        .resource_mut::<Reading>()
        .0
        .push(Pending::Walk { key, done });
}

/// The files of `paths`: each file, and each folder's files, in
/// order, under the name each has from the folder it was attached in.
fn walk(paths: &[PathBuf]) -> io::Result<Walked> {
    let mut walked = Walked {
        files: Vec::new(),
        skipped: 0,
    };
    let mut bytes = 0_u64;
    for top in paths {
        let name = last_of(top)?;
        let real = std::fs::canonicalize(top)?;
        let metadata = std::fs::metadata(&real)?;
        if metadata.is_dir() {
            walk_folder(
                &real,
                &real,
                &name,
                "",
                &mut walked,
                &mut bytes,
            )?;
        } else if metadata.is_file() {
            let rel = last_of(&real)?;
            let base =
                real.parent().unwrap_or(Path::new("")).to_path_buf();
            let size = metadata.len();
            plan(&mut walked, &mut bytes, name, base, rel, size)?;
        } else {
            return Err(io::Error::other(format!(
                "`{name}` is neither a file nor a folder."
            )));
        }
    }
    for (index, file) in walked.files.iter_mut().enumerate() {
        file.index = index;
    }
    Ok(walked)
}

/// The last part of `path`, if it is in UTF-8.
fn last_of(path: &Path) -> io::Result<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            io::Error::other("A name to send is in UTF-8.")
        })
}

fn walk_folder(
    base: &Path,
    folder: &Path,
    name: &str,
    rel: &str,
    walked: &mut Walked,
    bytes: &mut u64,
) -> io::Result<()> {
    let mut entries = std::fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    for entry in entries {
        let Ok(last) = last_of(&entry) else {
            walked.skipped += 1;
            continue;
        };
        let (name, rel) = (
            format!("{name}/{last}"),
            match rel.is_empty() {
                true => last.clone(),
                false => format!("{rel}/{last}"),
            },
        );
        let metadata = std::fs::symlink_metadata(&entry)?;
        if metadata.is_dir() {
            walk_folder(base, &entry, &name, &rel, walked, bytes)?;
        } else if metadata.is_file() {
            plan(
                walked,
                bytes,
                name,
                base.to_path_buf(),
                rel,
                metadata.len(),
            )?;
        } else {
            walked.skipped += 1;
        }
    }
    Ok(())
}

/// Adds a file to what is sent, unless it is more than a send takes.
fn plan(
    walked: &mut Walked,
    bytes: &mut u64,
    name: String,
    base: PathBuf,
    rel: String,
    size: u64,
) -> io::Result<()> {
    if size > AttachedFile::MOST_FILE {
        return Err(io::Error::other(format!(
            "`{name}` is more than 256 MiB."
        )));
    }
    *bytes += size;
    if *bytes > AttachedFile::MOST_BYTES {
        return Err(io::Error::other(
            "That is more than 1 GiB to send at once.",
        ));
    }
    if walked.files.len() >= AttachedFile::MOST_FILES {
        return Err(io::Error::other(
            "That is more than 10000 files to send at once.",
        ));
    }
    walked.files.push(Planned {
        index: 0,
        name,
        base,
        rel,
        size,
    });
    Ok(())
}

/// The file `rel` of the folder `base`, opened without following a
/// link on the way.
fn open(base: &Path, rel: &str) -> io::Result<File> {
    #[cfg(unix)]
    {
        use rustix::fs::{Mode, OFlags, openat};

        let mut opened = File::open(base)?;
        let mut parts = rel.split('/').peekable();
        while let Some(part) = parts.next() {
            let mut flags = OFlags::RDONLY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC;
            if parts.peek().is_some() {
                flags |= OFlags::DIRECTORY;
            }
            opened =
                openat(&opened, part, flags, Mode::empty())?.into();
        }
        Ok(opened)
    }
    #[cfg(not(unix))]
    File::open(base.join(rel))
}

/// The part of `file` that starts at `offset`.
fn read_at(file: &Planned, offset: u64) -> io::Result<Vec<u8>> {
    let mut opened = open(&file.base, &file.rel)?;
    let metadata = opened.metadata()?;
    if !metadata.is_file() || metadata.len() != file.size {
        return Err(io::Error::other(format!(
            "`{}` changed while it was sent.",
            file.name
        )));
    }
    opened.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; (file.size - offset).min(PART) as usize];
    opened.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Takes in the work that finished off the frame.
fn collect(world: &mut World) {
    let pending =
        std::mem::take(&mut world.resource_mut::<Reading>().0);
    let mut waiting = Vec::new();
    for pending in pending {
        match pending {
            Pending::Walk { key, mut done } => {
                match done.try_recv() {
                    Err(oneshot::error::TryRecvError::Empty) => {
                        waiting.push(Pending::Walk { key, done });
                    }
                    Err(oneshot::error::TryRecvError::Closed) => {
                        refuse(world, key, "The reading stopped.");
                    }
                    Ok(result) => walked(world, key, result),
                }
            }
            Pending::Part {
                key,
                file,
                upload,
                offset,
                mut done,
            } => match done.try_recv() {
                Err(oneshot::error::TryRecvError::Empty) => {
                    waiting.push(Pending::Part {
                        key,
                        file,
                        upload,
                        offset,
                        done,
                    });
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    refuse(world, key, "The reading stopped.");
                }
                Ok(result) => {
                    read(world, key, file, upload, offset, result);
                }
            },
        }
    }
    world.resource_mut::<Reading>().0.extend(waiting);
}

fn walked(world: &mut World, key: u64, result: io::Result<Walked>) {
    // A cross may have let it go while it was walked.
    if !matches!(
        world.resource::<Packs>().0.get(&key),
        Some(Stage::Walking)
    ) {
        return;
    }
    match result {
        Err(error) => refuse(world, key, error.to_string()),
        Ok(Walked { files, .. }) if files.is_empty() => {
            refuse(world, key, "There is nothing to send in that.");
        }
        Ok(Walked { files, skipped }) => {
            let bytes = files.iter().map(|file| file.size).sum();
            let count = files.len();
            world
                .resource_mut::<Packs>()
                .0
                .insert(key, Stage::Waiting(files));
            world.trigger(Gathered {
                key,
                files: count,
                bytes,
                skipped,
            });
        }
    }
}

/// Sends the part that was read, if what it belongs to is still
/// being sent.
fn read(
    world: &mut World,
    key: u64,
    file: Planned,
    upload: String,
    offset: u64,
    result: io::Result<Vec<u8>>,
) {
    if !matches!(
        world.resource::<Packs>().0.get(&key),
        Some(Stage::Going(_))
    ) {
        return;
    }
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => {
            refuse(world, key, error.to_string());
            return;
        }
    };
    let next = offset + bytes.len() as u64;
    let data =
        base64::engine::general_purpose::STANDARD.encode(&bytes);
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::PutFilePart(protocol::PutFilePart {
            data,
            id,
            offset,
            upload: upload.clone(),
        })
    });
    world.resource_mut::<Steps>().0.insert(
        id,
        Step::Part {
            key,
            file,
            upload,
            next,
        },
    );
}

/// Starts sending what was gathered for `key`.
fn begin(world: &mut World, key: u64) {
    let Some(Stage::Waiting(files)) =
        world.resource_mut::<Packs>().0.remove(&key)
    else {
        return;
    };
    world.resource_mut::<Packs>().0.insert(
        key,
        Stage::Going(Going {
            queue: files.into(),
            flying: 0,
            kept: Vec::new(),
        }),
    );
    pump(world, key);
}

/// Begins files of `key` until a few are on their way, or tells that
/// all are kept.
fn pump(world: &mut World, key: u64) {
    loop {
        let mut packs = world.resource_mut::<Packs>();
        let Some(Stage::Going(going)) = packs.0.get_mut(&key) else {
            return;
        };
        if going.flying >= AT_ONCE {
            return;
        }
        let Some(file) = going.queue.pop_front() else {
            if going.flying == 0 {
                let mut kept = std::mem::take(&mut going.kept);
                kept.sort_by_key(|(index, _)| *index);
                let files =
                    kept.into_iter().map(|(_, file)| file).collect();
                packs.0.remove(&key);
                world.trigger(FilesKept { key, files });
            }
            return;
        };
        going.flying += 1;
        let size = file.size;
        let id = world.resource_mut::<Outbox>().send(|id| {
            protocol::ClientMessage::PutFile(protocol::PutFile {
                id,
                size,
            })
        });
        world
            .resource_mut::<Steps>()
            .0
            .insert(id, Step::Begin { key, file });
    }
}

/// Asks for the part of `file` at `offset` to be read off the frame.
fn read_off_frame(
    world: &mut World,
    key: u64,
    file: Planned,
    upload: String,
    offset: u64,
) {
    let source = file.clone();
    let Some(done) =
        off_frame(world, move || read_at(&source, offset))
    else {
        refuse(world, key, "The window has no runtime to read with.");
        return;
    };
    world.resource_mut::<Reading>().0.push(Pending::Part {
        key,
        file,
        upload,
        offset,
        done,
    });
}

/// The server's id of the file `file`, which is kept.
fn kept(
    world: &mut World,
    key: u64,
    file: Planned,
    id: protocol::FileId,
) {
    if let Some(Stage::Going(going)) =
        world.resource_mut::<Packs>().0.get_mut(&key)
    {
        going.flying -= 1;
        going.kept.push((
            file.index,
            AttachedFile {
                id,
                name: file.name,
                size: file.size,
            },
        ));
    }
    pump(world, key);
}

/// The words of a refusal or fault, none for what is no refusal.
fn refusal(message: &protocol::ServerMessage) -> Option<String> {
    match message {
        protocol::ServerMessage::Fault(fault) => {
            Some(fault.text.clone())
        }
        protocol::ServerMessage::Reply(reply)
            if reply.kind != protocol::ReplyKind::Notice =>
        {
            Some(reply.text.clone())
        }
        _ => None,
    }
}

fn answered(world: &mut World, response: Response) {
    let Some(step) =
        world.resource_mut::<Steps>().0.remove(&response.to)
    else {
        return;
    };
    let key = step.key();
    if let Some(text) = refusal(&response.message) {
        refuse(world, key, text);
        return;
    }
    let protocol::ServerMessage::Reply(reply) = response.message
    else {
        return;
    };
    match step {
        Step::Begin { key, file } => {
            match (reply.file, reply.upload) {
                (Some(id), _) => kept(world, key, file, id),
                (None, Some(upload)) => {
                    read_off_frame(world, key, file, upload, 0);
                }
                (None, None) => refuse(
                    world,
                    key,
                    "The server did not accept the file.",
                ),
            }
        }
        Step::Part {
            key,
            file,
            upload,
            next,
        } => match reply.file {
            Some(id) => kept(world, key, file, id),
            None if next < file.size => {
                read_off_frame(world, key, file, upload, next);
            }
            None => refuse(world, key, "The server kept no file."),
        },
    }
}

/// Ends the uploads a lost connection cut off.
fn lost(_: On<Reconnected>, mut commands: Commands) {
    commands.queue(|world: &mut World| {
        world.resource_mut::<Steps>().0.clear();
        let going = world
            .resource::<Packs>()
            .0
            .iter()
            .filter(|(_, stage)| matches!(stage, Stage::Going(_)))
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();
        for key in going {
            refuse(world, key, "The connection was lost.");
        }
    });
}
