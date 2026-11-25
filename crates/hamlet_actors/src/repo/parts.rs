//! Files sent in parts: what a piece counts for, the paths a part may
//! name, and the pieces a store keeps joined back into files.

use anyhow::{Context, Result, bail, ensure};
use hamlet_actor::ColumnValue;
use hamlet_git::{BlobId, Change, Entry};
use hamlet_messages::bytes::Base64;
use hamlet_messages::profile::Piece;
use hamlet_messages::{MOST_PART, PIECE_FRAME};
use hamlet_os::fs::TreePath;

/// The longest name of one file or folder a path may hold, in bytes.
const MOST_NAME: usize = 255;

/// The prefix of the name of a checkout's file being written; a
/// checkout's listing never shows one.
pub(crate) const INCOMING: &str = ".incoming-";

/// The size a piece of `bytes` bytes at `path` counts for in a part.
pub(crate) fn cost(path: &str, bytes: usize) -> usize {
    path.len() + bytes + PIECE_FRAME
}

/// Whether `path` names a file inside a folder that every system
/// Hamlet runs on can write, and that a checkout lists.
pub(crate) fn is_file_path(path: &str) -> bool {
    TreePath::parse(path).is_ok()
        && path.split('/').all(|name| {
            name.len() <= MOST_NAME && !name.starts_with(INCOMING)
        })
}

/// The size `pieces`, each a path and its length in bytes, count for
/// together, or the first path that is not a file path.
pub(crate) fn sized<'a>(
    pieces: impl IntoIterator<Item = (&'a str, usize)>,
) -> Result<usize, &'a str> {
    let mut size = 0;
    for (path, bytes) in pieces {
        if !is_file_path(path) {
            return Err(path);
        }
        size += cost(path, bytes);
    }
    Ok(size)
}

/// A piece a store keeps: part of a file, or a removal.
pub(crate) struct Kept {
    pub path: String,
    pub offset: usize,
    /// `None` for a removal.
    pub bytes: Option<Vec<u8>>,
    pub last: bool,
    pub executable: bool,
}

impl Kept {
    /// Reads a row of `path, offset, bytes, last, executable`.
    pub fn of(row: &[ColumnValue]) -> Result<Self> {
        let [
            ColumnValue::Text(path),
            ColumnValue::Integer(offset),
            bytes,
            ColumnValue::Integer(last),
            ColumnValue::Integer(executable),
        ] = row
        else {
            bail!("a piece row of another shape: {row:?}");
        };
        Ok(Self {
            path: path.clone(),
            offset: usize::try_from(*offset)?,
            bytes: match bytes {
                ColumnValue::Blob(bytes) => Some(bytes.clone()),
                ColumnValue::Null => None,
                other => {
                    bail!(
                        "a piece's bytes of another kind: {other:?}"
                    )
                }
            },
            last: *last != 0,
            executable: *executable != 0,
        })
    }
}

impl TryFrom<Piece> for Kept {
    type Error = anyhow::Error;

    fn try_from(piece: Piece) -> Result<Self> {
        Ok(Self {
            path: piece.path,
            offset: usize::try_from(piece.offset)?,
            bytes: Some(piece.bytes.0),
            last: piece.last,
            executable: piece.executable,
        })
    }
}

/// The changes `pieces` make, in path and offset order: each file's
/// pieces joined end to end, each removal as it is. A file runs when
/// its last piece says so.
pub(crate) fn joined(pieces: Vec<Kept>) -> Result<Vec<Change>> {
    let mut changes = Vec::new();
    let mut open: Option<(String, Vec<u8>)> = None;
    for piece in pieces {
        let Some(bytes) = piece.bytes else {
            ensure!(open.is_none(), "{} is cut short", piece.path);
            changes.push(Change::Remove { path: piece.path });
            continue;
        };
        let mut file = match open.take() {
            None if piece.offset == 0 => Vec::new(),
            Some((path, file))
                if path == piece.path
                    && file.len() == piece.offset =>
            {
                file
            }
            _ => bail!("the pieces of {} do not join", piece.path),
        };
        file.extend(bytes);
        let path = piece.path;
        if !piece.last {
            open = Some((path, file));
            continue;
        }
        changes.push(if piece.executable {
            Change::WriteExecutable { path, bytes: file }
        } else {
            Change::Write { path, bytes: file }
        });
    }
    if let Some((path, _)) = open {
        bail!("the last piece of {path} never came");
    }
    Ok(changes)
}

/// A removal or a file a part sends, in the order a cursor counts it.
pub(crate) enum Item {
    /// A path, with the blob it held.
    Remove {
        path: String,
        blob: BlobId,
    },
    Write(Entry),
}

/// The items one part holds.
pub(crate) struct Packed {
    pub files: Vec<Piece>,
    pub removed: Vec<(String, BlobId)>,
    /// The item and the byte within it that the next part starts at;
    /// `None` after the last.
    pub next: Option<(usize, usize)>,
}

/// The items from `from` on that fit in `room`, each counting its
/// path, its bytes and [`PIECE_FRAME`]. A file that fits a part of
/// its own waits for the next part when this one is full; a larger
/// one is cut by byte range to fill each part it spans.
pub(crate) fn pack(
    items: &[Item],
    (mut at, mut offset): (usize, usize),
    mut room: usize,
    bytes: &dyn Fn(BlobId) -> Result<Vec<u8>>,
) -> Result<Packed> {
    let mut packed = Packed {
        files: Vec::new(),
        removed: Vec::new(),
        next: None,
    };
    while let Some(item) = items.get(at) {
        match item {
            Item::Remove { path, blob } => {
                let cost = cost(path, 0);
                if cost > room {
                    packed.next = Some((at, 0));
                    return Ok(packed);
                }
                room -= cost;
                packed.removed.push((path.clone(), *blob));
            }
            Item::Write(entry) => {
                let head = cost(&entry.path, 0);
                ensure!(
                    head < MOST_PART,
                    "the path {} is too long to send",
                    entry.path
                );
                let all = bytes(entry.blob)?;
                let rest = all
                    .get(offset..)
                    .context("a cursor past the end of its file")?;
                let sent = (head + rest.len() <= room)
                    .then_some(rest.len())
                    .or_else(|| {
                        (head + rest.len() > MOST_PART && head < room)
                            .then(|| room - head)
                    });
                let Some(sent) = sent else {
                    packed.next = Some((at, offset));
                    return Ok(packed);
                };
                packed.files.push(Piece {
                    path: entry.path.clone(),
                    offset: u32::try_from(offset)?,
                    bytes: Base64(rest[..sent].to_vec()),
                    last: sent == rest.len(),
                    executable: entry.executable,
                });
                room -= head + sent;
                if sent < rest.len() {
                    packed.next = Some((at, offset + sent));
                    return Ok(packed);
                }
            }
        }
        at += 1;
        offset = 0;
    }
    Ok(packed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(
        path: &str,
        offset: usize,
        bytes: &[u8],
        last: bool,
    ) -> Kept {
        Kept {
            path: path.into(),
            offset,
            bytes: Some(bytes.to_vec()),
            last,
            executable: false,
        }
    }

    #[test]
    fn pieces_join_into_whole_files() {
        let changes = joined(vec![
            piece("a.md", 0, b"he", false),
            piece("a.md", 2, b"y", true),
            Kept {
                bytes: None,
                ..piece("b.md", 0, b"", true)
            },
        ])
        .unwrap();
        assert_eq!(
            changes,
            [
                Change::Write {
                    path: "a.md".into(),
                    bytes: b"hey".to_vec()
                },
                Change::Remove {
                    path: "b.md".into()
                },
            ]
        );
    }

    #[test]
    fn pieces_that_do_not_join_fail() {
        assert!(joined(vec![piece("a.md", 1, b"x", true)]).is_err());
        assert!(joined(vec![piece("a.md", 0, b"x", false)]).is_err());
        assert!(
            joined(vec![
                piece("a.md", 0, b"x", false),
                piece("a.md", 2, b"y", true),
            ])
            .is_err()
        );
    }

    #[test]
    fn a_file_path_is_one_every_system_writes() {
        for bad in [
            "",
            "/abs.md",
            "a//b.md",
            "a\\b.md",
            "../a.md",
            "a/./b.md",
            &"n".repeat(MOST_NAME + 1),
            ".incoming-a.md",
            "memory/.incoming-plan.md",
            ".incoming-x/a.md",
        ] {
            assert!(!is_file_path(bad), "{bad:?}");
        }
        for good in ["a.md", "scripts/run.py", &"n".repeat(MOST_NAME)]
        {
            assert!(is_file_path(good), "{good:?}");
        }
    }
}
