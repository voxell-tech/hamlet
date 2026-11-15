//! What a hire's agent asks of its own branch: its files in parts,
//! and a save of what its filesystem changed, in parts. Also the
//! `save` table, the parts of a save whose last part has not come.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_git::{BlobId, Change, Entry};
use hamlet_messages::bytes::Base64;
use hamlet_messages::profile::{
    BadPath, Cursor, FileChange, Files, FilesAnswer, Gone, Path,
    Piece, Save, SaveAnswer, SaveRefusal, TooLarge,
};
use hamlet_messages::workspace::{AgentNumber, CallId};
use hamlet_messages::{MOST_PART, PIECE_FRAME};
use hamlet_os::fs::TreePath;

use super::hires::{self, Hire};
use super::{CHARACTER, Profile, commit_id, git_commit};
use crate::repo::git::{By, Reads};
use crate::repo::store::{self, Rows};
use crate::repo::{Before, Call, Ended, Look, Reply, Takes};

/// Removes the parts of every save a stop cut off, at the profile's
/// start. Their agents save again under a new call.
pub(super) const CUT_OFF: &str = "DELETE FROM save";

/// The token of the `save` table.
struct Table(());

impl store::Table for Table {
    const NAME: &'static str = "save";
}

/// Whether `path` is one an agent's filesystem is never given and
/// never gives: the character, and its own inbox.
fn withheld(path: &str) -> bool {
    path == CHARACTER || path == "inbox" || path.starts_with("inbox/")
}

impl Takes<Files> for Profile {
    async fn take(call: Call<Self, Files>) -> Result<Reply<Files>> {
        call.read(files).await
    }
}

fn files(look: &Look<'_, Profile, Files>) -> Result<FilesAnswer> {
    let hire = look.from();
    let Files { since, after } = look.message();
    let git = look.git()?;
    let tip = match after {
        Some(cursor) => {
            let commit = git_commit(cursor.commit)?;
            if !(git.has(commit)?
                && git.holds(&hire.branch, commit)?)
            {
                return Ok(FilesAnswer::Refused {
                    why: Gone::CommitGone {
                        commit: cursor.commit,
                    },
                });
            }
            commit
        }
        None => {
            git.tip(&hire.branch)?.context("a hire has a branch")?
        }
    };
    let since = match since {
        Some(since) => {
            let since = git_commit(*since)?;
            git.has(since)?.then_some(since)
        }
        None => None,
    };
    let character = match after {
        None => Some(hires::character(git, hire, tip)?),
        Some(_) => None,
    };
    let room = MOST_PART.saturating_sub(
        character.as_ref().map_or(0, |c| c.text.len() + PIECE_FRAME),
    );
    let from = after.as_ref().map_or((0, 0), |cursor| {
        (cursor.file as usize, cursor.offset as usize)
    });
    let items = items(git, since, tip)?;
    let packed = pack(&items, from, room, &|blob| git.blob(blob))?;
    let next = packed
        .next
        .map(|(file, offset)| {
            Ok::<_, anyhow::Error>(Cursor {
                commit: commit_id(tip)?,
                file: u32::try_from(file)?,
                offset: u32::try_from(offset)?,
            })
        })
        .transpose()?;
    Ok(FilesAnswer::Part {
        tip: commit_id(tip)?,
        files: packed.files,
        removed: packed.removed,
        next,
        character,
    })
}

/// What a fill sends, in the order a cursor counts it.
enum Item {
    Remove(String),
    Write(Entry),
}

/// The paths `since` held that `tip` does not, then the files of
/// `tip` that differ from `since`'s in bytes or in whether they run,
/// in path order; every file of `tip` with no `since`. Never a
/// withheld path.
fn items(
    git: &Reads,
    since: Option<hamlet_git::CommitId>,
    tip: hamlet_git::CommitId,
) -> Result<Vec<Item>> {
    let now = git
        .entries(tip)?
        .into_iter()
        .filter(|entry| !withheld(&entry.path));
    let Some(since) = since else {
        return Ok(now.map(Item::Write).collect());
    };
    let mut was = git
        .entries(since)?
        .into_iter()
        .filter(|entry| !withheld(&entry.path))
        .map(|entry| (entry.path, (entry.blob, entry.executable)))
        .collect::<BTreeMap<_, _>>();
    let written = now
        .filter(|entry| {
            was.remove(&entry.path)
                != Some((entry.blob, entry.executable))
        })
        .collect::<Vec<_>>();
    Ok(was
        .into_keys()
        .map(Item::Remove)
        .chain(written.into_iter().map(Item::Write))
        .collect())
}

/// One part of a fill.
struct Packed {
    files: Vec<Piece>,
    removed: Vec<Path>,
    /// The item and the byte within it that the next part starts at;
    /// `None` after the last.
    next: Option<(usize, usize)>,
}

/// The items from `from` on that fit in `room`, each counting its
/// path, its bytes and [`PIECE_FRAME`]. A file that fits a part of
/// its own waits for the next part when this one is full; a larger
/// one is cut by byte range to fill each part it spans.
fn pack(
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
            Item::Remove(path) => {
                let cost = path.len() + PIECE_FRAME;
                if cost > room {
                    packed.next = Some((at, 0));
                    return Ok(packed);
                }
                room -= cost;
                packed.removed.push(Path { path: path.clone() });
            }
            Item::Write(entry) => {
                let head = entry.path.len() + PIECE_FRAME;
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
                            .then_some(room - head)
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

impl Takes<Save> for Profile {
    async fn take(call: Call<Self, Save>) -> Result<Reply<Save>> {
        if let Err(why) = fits(&call.changes) {
            return call.refuse(why).await;
        }
        call.once(save).await
    }
}

/// Refuses a part that names a path outside the folder or withheld,
/// or holds more than [`MOST_PART`].
fn fits(changes: &[FileChange]) -> Result<(), SaveRefusal> {
    let mut size = 0;
    for change in changes {
        let (path, bytes) = match change {
            FileChange::Write(piece) => {
                (&piece.path, piece.bytes.0.len())
            }
            FileChange::Remove { path } => (path, 0),
        };
        if TreePath::parse(path).is_err() {
            let path = path.clone();
            return Err(BadPath::NotAPath { path }.into());
        }
        if withheld(path) {
            let path = path.clone();
            return Err(BadPath::Withheld { path }.into());
        }
        size += path.len() + bytes + PIECE_FRAME;
    }
    if size > MOST_PART {
        let most =
            u32::try_from(MOST_PART).expect("a part fits a u32");
        return Err(TooLarge::PartTooLarge { most }.into());
    }
    Ok(())
}

/// Keeps one part of a save as rows. The last one commits every part
/// of the call onto `base` as one commit and joins it to the hire's
/// branch, the save's side winning.
fn save(unit: Before<'_, Profile, Save>) -> Result<Ended<Save>> {
    let Save {
        call,
        base,
        part,
        last,
        changes,
    } = unit.message().clone();
    let Hire { agent, branch, .. } = unit.from();
    let (agent, branch) = (*agent, branch.clone());
    let at = git_commit(base)?;
    if !unit.git()?.has(at)? {
        return unit.refuse(Gone::CommitGone { commit: base }.into());
    }
    // A part's earlier parts are kept answers, but their rows went
    // with a stop: committing the rest would lose them.
    if part > 0 && !begun(&unit.rows(Table(())), agent, &call)? {
        bail!(
            "the parts before part {part} of the save {} are gone; \
             save again under a new call",
            call.0
        );
    }
    let writer = unit.write();
    let table = writer.table(Table(()));
    if part == 0 {
        table.execute(
            "DELETE FROM save WHERE agent = ?1 AND call != ?2",
            of_call(agent, &call),
        )?;
    }
    for change in changes {
        let (path, offset, bytes, last, executable) = match change {
            FileChange::Write(piece) => (
                piece.path,
                piece.offset,
                BindParam::Blob(piece.bytes.0),
                piece.last,
                piece.executable,
            ),
            FileChange::Remove { path } => {
                (path, 0, BindParam::Null, true, false)
            }
        };
        let mut row = of_call(agent, &call);
        row.extend([
            BindParam::Text(path),
            BindParam::Integer(i64::from(offset)),
            bytes,
            BindParam::Integer(i64::from(last)),
            BindParam::Integer(i64::from(executable)),
        ]);
        table.execute(
            "INSERT OR REPLACE INTO save (agent, call, path, \
             offset, bytes, last, executable) VALUES (?1, ?2, ?3, \
             ?4, ?5, ?6, ?7)",
            row,
        )?;
    }
    if !last {
        return writer.kept(SaveAnswer::Kept);
    }
    let changes = joined(pieces(&table.rows(), agent, &call)?)?;
    let tip = writer.writes()?.save_onto(
        &branch,
        at,
        &changes,
        By::Agent(agent),
    )?;
    table.execute(
        "DELETE FROM save WHERE agent = ?1 AND call = ?2",
        of_call(agent, &call),
    )?;
    writer.kept(SaveAnswer::Saved {
        tip: commit_id(tip)?,
    })
}

/// The agent and the call that name a save's rows.
fn of_call(agent: AgentNumber, call: &CallId) -> Vec<BindParam> {
    vec![
        BindParam::Integer(i64::from(agent.0)),
        BindParam::Text(call.0.clone()),
    ]
}

/// A row of the `save` table: a piece of a file, or a removal.
struct Kept {
    path: String,
    offset: usize,
    /// `None` for a removal.
    bytes: Option<Vec<u8>>,
    last: bool,
    executable: bool,
}

/// Whether the save holds any row.
fn begun(
    rows: &Rows<'_, Table>,
    agent: AgentNumber,
    call: &CallId,
) -> Result<bool> {
    let found = rows.select(
        "SELECT 1 FROM save WHERE agent = ?1 AND call = ?2 LIMIT 1",
        of_call(agent, call),
    )?;
    Ok(!found.is_empty())
}

/// The rows of one save, by path and offset.
fn pieces(
    rows: &Rows<'_, Table>,
    agent: AgentNumber,
    call: &CallId,
) -> Result<Vec<Kept>> {
    rows.select(
        "SELECT path, offset, bytes, last, executable FROM save \
         WHERE agent = ?1 AND call = ?2 ORDER BY path, offset",
        of_call(agent, call),
    )?
    .into_iter()
    .map(|row| match row.as_slice() {
        [
            ColumnValue::Text(path),
            ColumnValue::Integer(offset),
            bytes,
            ColumnValue::Integer(last),
            ColumnValue::Integer(executable),
        ] => Ok(Kept {
            path: path.clone(),
            offset: usize::try_from(*offset)?,
            bytes: match bytes {
                ColumnValue::Blob(bytes) => Some(bytes.clone()),
                ColumnValue::Null => None,
                other => bail!(
                    "a piece's bytes of another kind: {other:?}"
                ),
            },
            last: *last != 0,
            executable: *executable != 0,
        }),
        _ => bail!("a save row of another shape: {row:?}"),
    })
    .collect()
}

/// The changes a save's rows make: each file's pieces joined in
/// order, each removal as it is.
fn joined(pieces: Vec<Kept>) -> Result<Vec<Change>> {
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
