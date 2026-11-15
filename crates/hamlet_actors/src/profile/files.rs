//! The calls a hire's agent makes on its own branch: its files in
//! parts, and a save of what its filesystem changed, in parts. Also
//! the `save` table, the parts of a save whose last part has not
//! come.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_git::{BlobId, Entry};
use hamlet_messages::bytes::Base64;
use hamlet_messages::profile::{
    BadPath, Cursor, FileChange, Files, FilesAnswer, Gone, Path,
    Piece, Protected, Save, SaveAnswer, SaveRefusal, TooLarge,
};
use hamlet_messages::workspace::{AgentNumber, CallId};
use hamlet_messages::{MOST_PART, PIECE_FRAME};

use super::hires::{self, Hire};
use super::{CHARACTER, Profile, commit_id, git_commit};
use crate::repo::git::{By, Reads};
use crate::repo::parts::{self, Kept, joined};
use crate::repo::store::{self, Rows};
use crate::repo::{Before, Call, Ended, Look, Reply, Takes};

struct Table(());

impl store::Table for Table {
    const NAME: &'static str = "save";
}

struct Calls(());

impl store::Table for Calls {
    const NAME: &'static str = "save_call";
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

/// A removal or a file a fill sends, in the order a cursor counts it.
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
                let cost = parts::cost(path, 0);
                if cost > room {
                    packed.next = Some((at, 0));
                    return Ok(packed);
                }
                room -= cost;
                packed.removed.push(Path { path: path.clone() });
            }
            Item::Write(entry) => {
                let head = parts::cost(&entry.path, 0);
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

impl Takes<Save> for Profile {
    async fn take(call: Call<Self, Save>) -> Result<Reply<Save>> {
        if let Err(why) = fits(&call.changes) {
            return call.refuse(why).await;
        }
        call.once(save).await
    }
}

fn fits(changes: &[FileChange]) -> Result<(), SaveRefusal> {
    let pieces = changes.iter().map(|change| match change {
        FileChange::Write(piece) => {
            (piece.path.as_str(), piece.bytes.0.len())
        }
        FileChange::Remove { path } => (path.as_str(), 0),
    });
    let size = parts::sized(pieces.clone()).map_err(|path| {
        BadPath::NotAPath {
            path: path.to_owned(),
        }
    })?;
    if let Some((path, _)) =
        pieces.clone().find(|(path, _)| withheld(path))
    {
        let path = path.to_owned();
        return Err(Protected::Withheld { path }.into());
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
    let git = unit.git()?;
    if !git.has(at)? || !git.holds(&branch, at)? {
        return unit.refuse(Gone::CommitGone { commit: base }.into());
    }
    if part != next_part(&unit.rows(Calls(())), agent, &call)? {
        bail!(
            "the parts before part {part} of the save {} did not \
             all come; save again under a new call",
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
    let mut row = of_call(agent, &call);
    row.push(BindParam::Integer(i64::from(part) + 1));
    writer.table(Calls(())).execute(
        "INSERT OR REPLACE INTO save_call (agent, call, next_part) \
         VALUES (?1, ?2, ?3)",
        row,
    )?;
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
    writer.table(Calls(())).execute(
        "DELETE FROM save_call WHERE agent = ?1",
        vec![BindParam::Integer(i64::from(agent.0))],
    )?;
    writer.kept(SaveAnswer::Saved {
        tip: commit_id(tip)?,
    })
}

fn of_call(agent: AgentNumber, call: &CallId) -> Vec<BindParam> {
    vec![
        BindParam::Integer(i64::from(agent.0)),
        BindParam::Text(call.0.clone()),
    ]
}

fn next_part(
    rows: &Rows<'_, Calls>,
    agent: AgentNumber,
    call: &CallId,
) -> Result<u32> {
    let found = rows.select(
        "SELECT next_part FROM save_call WHERE agent = ?1 AND call \
         = ?2",
        of_call(agent, call),
    )?;
    match found.first().map(Vec::as_slice) {
        None => Ok(0),
        Some([ColumnValue::Integer(next)]) => {
            Ok(u32::try_from(*next)?)
        }
        Some(row) => {
            bail!("a save_call row of another shape: {row:?}")
        }
    }
}

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
    .iter()
    .map(|row| Kept::of(row))
    .collect()
}
