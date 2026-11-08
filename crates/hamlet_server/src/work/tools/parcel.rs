//! The files an agent sends with a `tell` or `ask` (H30): found under
//! the paths it names without following links, and read as it may
//! read.

use std::path::{Path, PathBuf};

use hamlet_os::fs::{TreePath, path};
use hamlet_protocol::client::{AttachedFile, ImageId, ImageInput};
use hamlet_sandbox::{FileError, FileKind};

use super::environment::Reach;
use crate::images::Images;
use crate::inbox::Inbox;
use crate::society::refusal::Refusal;

/// The most entries, files or not, that one send visits.
const VISITED: usize = 50_000;

/// The files a send holds, and the images and links among them.
#[derive(Debug, Default)]
pub(crate) struct Sent {
    pub files: usize,
    pub bytes: u64,
    /// The images among the files, kept as images.
    pub images: Vec<ImageId>,
    /// Links found inside folders, which are left out.
    pub skipped: usize,
}

/// Where `Sent`'s files go: the folder `inbox` saves in, and the
/// folder of kept images.
pub(crate) struct To<'a> {
    pub inbox: &'a mut Inbox,
    pub images: PathBuf,
}

struct Piece {
    name: String,
    path: String,
}

fn usage(words: String) -> Refusal {
    Refusal::Usage { words }
}

fn unreadable(path: &str, error: &FileError) -> Refusal {
    usage(format!("`{path}` cannot be read: {}", error.message))
}

/// The files under `paths`, at most `most_file` bytes each, saved
/// into `to` when it is given.
pub(super) async fn run(
    env: &mut impl Reach,
    paths: &[String],
    most_file: u64,
    mut to: Option<To<'_>>,
) -> anyhow::Result<Result<Sent, Refusal>> {
    let found = match find(env, paths, most_file).await {
        Ok(found) => found,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let mut sent = Sent {
        files: found.pieces.len(),
        bytes: found.bytes,
        images: Vec::new(),
        skipped: found.skipped,
    };
    let Some(to) = to.as_mut() else {
        return Ok(Ok(sent));
    };
    let images = Images::new(to.images.clone());
    for piece in &found.pieces {
        let bytes = match env.read_file(&piece.path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(Err(unreadable(&piece.name, &error)));
            }
        };
        if let Err(refusal) =
            to.inbox.put(&piece.name, &bytes).await?
        {
            return Ok(Err(refusal));
        }
        if ImageInput::is_image_file(Path::new(&piece.name))
            && let Ok((id, _)) = images.keep(&bytes)?
        {
            sent.images.push(id);
        }
    }
    Ok(Ok(sent))
}

struct Found {
    pieces: Vec<Piece>,
    bytes: u64,
    skipped: usize,
}

async fn find(
    env: &mut impl Reach,
    paths: &[String],
    most_file: u64,
) -> Result<Found, Refusal> {
    let mut found = Found {
        pieces: Vec::new(),
        bytes: 0,
        skipped: 0,
    };
    let mut visited = 0;
    for given in paths {
        let full = env
            .footing(given)
            .await
            .map_err(|error| unreadable(given, &error))?;
        let info = env
            .file_info(&full)
            .await
            .map_err(|error| unreadable(given, &error))?;
        if info.kind == FileKind::Symlink {
            return Err(usage(format!(
                "`{given}` is a link, and links are not sent."
            )));
        }
        if info.name.is_empty() {
            return Err(usage(format!(
                "`{given}` has no name to send."
            )));
        }
        let mut stack = vec![(info.name.clone(), full, info)];
        while let Some((name, full, info)) = stack.pop() {
            visited += 1;
            if visited > VISITED {
                return Err(usage(
                    "That is too many folders and files to look \
                     through."
                        .into(),
                ));
            }
            if TreePath::parse(&name).is_err() {
                return Err(usage(format!(
                    "`{name}` is not a name of a file."
                )));
            }
            match info.kind {
                FileKind::File => {
                    if info.size > most_file {
                        return Err(usage(format!(
                            "`{name}` is more than {}.",
                            AttachedFile::size_words(most_file)
                        )));
                    }
                    found.bytes += info.size;
                    if found.bytes > AttachedFile::MOST_BYTES {
                        return Err(usage(format!(
                            "That is more than {} to send at once.",
                            AttachedFile::size_words(
                                AttachedFile::MOST_BYTES
                            )
                        )));
                    }
                    if found.pieces.len() >= AttachedFile::MOST_FILES
                    {
                        return Err(usage(format!(
                            "That is more than {} files to send at \
                             once.",
                            AttachedFile::MOST_FILES
                        )));
                    }
                    found.pieces.push(Piece { name, path: full });
                }
                FileKind::Directory => {
                    let mut entries = env
                        .list_dir(&full)
                        .await
                        .map_err(|error| unreadable(&name, &error))?;
                    // Reversed, so they come off the stack in order.
                    entries.sort_by(|a, b| b.name.cmp(&a.name));
                    for entry in entries {
                        if entry.kind == FileKind::Symlink {
                            found.skipped += 1;
                            continue;
                        }
                        stack.push((
                            format!("{name}/{}", entry.name),
                            path::join(&[full.as_str(), &entry.name]),
                            entry,
                        ));
                    }
                }
                FileKind::Symlink => found.skipped += 1,
            }
        }
    }
    if found.pieces.is_empty() {
        return Err(usage(
            "There are no files in those paths.".into(),
        ));
    }
    Ok(found)
}
