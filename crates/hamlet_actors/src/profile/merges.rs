//! One hire's branch merged into another's and that merge taken out
//! again, a branch's work read out in parts for a merge from another
//! profile, and a branch dropped. Also the tables of a merge: the
//! parts of one from another profile whose last part has not come,
//! and the commit of every merge.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result, bail, ensure};
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_git::{BlobId, Change, CommitId, MAIN, blob_id};
use hamlet_messages::profile::{
    BadPath, Bundle, BundleAnswer, Cursor, DropBranch, Gone,
    MergeAnswer, MergeBranch, Piece, Protected, Removed, SaveRefusal,
    Sha1, Source, TooLarge, Unmerge, UnmergeAnswer,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::{MOST_PART, Message};

use super::files::{drop_saves, items, withheld};
use super::{Profile, commit_id, git_commit, hires};
use crate::repo::git::{By, Git, Reads};
use crate::repo::parts::{self, Kept, joined};
use crate::repo::store::{self, Rows};
use crate::repo::{
    Admits, Before, Call, Ended, Look, Reply, Takes, Writer,
};

const SPLIT: &str = "Split the merge";

struct Parts(());

impl store::Table for Parts {
    const NAME: &'static str = "merge_part";
}

struct Calls(());

impl store::Table for Calls {
    const NAME: &'static str = "merge_call";
}

struct Merges(());

impl store::Table for Merges {
    const NAME: &'static str = "merge_commit";
}

struct Floors(());

impl store::Table for Floors {
    const NAME: &'static str = "merge_floor";
}

/// Keeps each branch's tip after a purge gave every commit a new id:
/// a merge's repeat looks for the commit a kill cut off no further
/// back than these.
pub(super) fn purged<M>(
    writer: &Writer<'_, Profile, M>,
    tips: impl IntoIterator<Item = CommitId>,
) -> Result<()>
where
    M: Message,
    Profile: Admits<M::From>,
{
    for tip in tips {
        writer.table(Floors(())).execute(
            "INSERT OR IGNORE INTO merge_floor (id) VALUES (?1)",
            vec![BindParam::Text(tip.to_string())],
        )?;
    }
    Ok(())
}

/// Where the file at `path` of hire `absorbed` is set aside when the
/// kept hire holds another.
fn aside(path: &str, absorbed: AgentNumber) -> String {
    let from = format!("from-hire-{}", absorbed.0);
    if let Some(rest) = path.strip_prefix("skills/")
        && let Some((skill, inner)) = rest.split_once('/')
    {
        return format!("skills/{skill}-{from}/{inner}");
    }
    format!("{path}.{from}")
}

/// What the absorbed hire brings to a merge.
enum Learned {
    /// Its branch's tip, in this repository.
    Branch(CommitId),
    /// What it changed in another profile: its files, and the paths
    /// it removed with the blob each held.
    Files {
        written: Vec<Change>,
        removed: Vec<(String, BlobId)>,
    },
}

/// A merge's files, as a commit no branch names, its second parent,
/// and the paths it set aside.
struct Staged {
    files: CommitId,
    also: CommitId,
    set_aside: Vec<String>,
}

impl Takes<MergeBranch> for Profile {
    async fn take(
        call: Call<Self, MergeBranch>,
    ) -> Result<Reply<MergeBranch>> {
        if let Source::Part { files, removed, .. } = &call.from
            && let Err(why) = fits(files, removed)
        {
            return call.refuse(why).await;
        }
        call.once(merge_branch).await
    }
}

fn fits(
    files: &[Piece],
    removed: &[Removed],
) -> Result<(), SaveRefusal> {
    let pieces = files
        .iter()
        .map(|piece| (piece.path.as_str(), piece.bytes.0.len()))
        .chain(removed.iter().map(|gone| (gone.path.as_str(), 0)));
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

/// Keeps one part of a merge from another profile as rows, or merges
/// the absorbed hire's branch, or every part once the last comes.
fn merge_branch(
    unit: Before<'_, Profile, MergeBranch>,
) -> Result<Ended<MergeBranch>> {
    let MergeBranch {
        id,
        into,
        absorbed,
        message,
        from,
    } = unit.message().clone();
    let (writer, learned) = match from {
        Source::Branch { name } => {
            let learned = match unit.git()?.tip(&name)? {
                Some(theirs) => Learned::Branch(theirs),
                // An erase may drop the branch before its merge ends.
                None => Learned::Files {
                    written: Vec::new(),
                    removed: Vec::new(),
                },
            };
            (unit.write(), learned)
        }
        Source::Part {
            part,
            last,
            files,
            removed,
        } => {
            let id = id.as_str();
            ensure!(
                part == next_part(&unit.rows(Calls(())), id)?,
                "the parts before part {part} of the merge {id} did \
                 not all come; merge again under a new id"
            );
            let writer = unit.write();
            keep_part(&writer, id, part, files, removed)?;
            if !last {
                return writer.kept(MergeAnswer::Kept);
            }
            let learned = take_parts(&writer, id)?;
            (writer, learned)
        }
    };
    merge_into(writer, &into, absorbed, &message, &learned)
}

/// Merges `learned` into `into` as one commit, or finds the commit a
/// run a kill cut off made, and tells the branch's hire it moved.
fn merge_into(
    mut writer: Writer<'_, Profile, MergeBranch>,
    into: &str,
    absorbed: AgentNumber,
    message: &str,
    learned: &Learned,
) -> Result<Ended<MergeBranch>> {
    let mut stops = ids(&writer.table(Merges(())).rows())?;
    stops.extend(ids(&writer.table(Floors(())).rows())?);
    let (commit, set_aside) = {
        let (reads, git) = (writer.git()?, writer.writes()?);
        let tip = reads
            .tip(into)?
            .with_context(|| format!("no branch {into}"))?;
        let cut_off = reads
            .first_parents(tip)?
            .into_iter()
            .take_while(|made| !stops.contains(&made.id.to_string()))
            // The history gives a commit's message trimmed.
            .find(|made| {
                made.author == By::Realm.name()
                    && made.message == message.trim()
                    && made.parents.len() == 2
            });
        match cut_off {
            Some(made) => {
                let staged = stage(
                    &git,
                    reads,
                    made.parents[0],
                    learned,
                    absorbed,
                    message,
                )?;
                (made.id, staged.set_aside)
            }
            None => {
                let staged = stage(
                    &git, reads, tip, learned, absorbed, message,
                )?;
                let commit = git.merge_onto(
                    into,
                    staged.files,
                    &withheld,
                    staged.also,
                    message,
                    By::Realm,
                )?;
                (commit, staged.set_aside)
            }
        }
    };
    writer.table(Merges(())).execute(
        "INSERT INTO merge_commit (id) VALUES (?1)",
        vec![BindParam::Text(commit.to_string())],
    )?;
    tell_hire(&mut writer, into, commit)?;
    writer.kept(MergeAnswer::Merged {
        tip: commit_id(commit)?,
        set_aside,
    })
}

/// The files `learned` brings onto `before`. Where both changed a
/// file, the kept side's lines win within one profile; from another
/// profile, the kept side's file wins and the absorbed hire's is set
/// aside.
fn stage(
    git: &Git<'_>,
    reads: &Reads,
    before: CommitId,
    learned: &Learned,
    absorbed: AgentNumber,
    message: &str,
) -> Result<Staged> {
    let mut changes = Vec::new();
    let mut set_aside = Vec::new();
    let (onto, also) = match learned {
        Learned::Branch(theirs) => (
            git.merge_files(before, *theirs, message)?.commit,
            *theirs,
        ),
        Learned::Files { written, removed } => {
            let kept = reads
                .entries(before)?
                .into_iter()
                .map(|entry| (entry.path, entry.blob))
                .collect::<BTreeMap<_, _>>();
            for change in written {
                let (Change::Write { path, bytes }
                | Change::WriteExecutable { path, bytes }) = change
                else {
                    bail!(
                        "a merge's file of another kind: {change:?}"
                    );
                };
                // As today's merge from another profile writes it, a
                // file brought in does not run.
                let brought = |path: String| Change::Write {
                    path,
                    bytes: bytes.clone(),
                };
                match kept.get(path) {
                    Some(own) if *own == blob_id(bytes) => {}
                    Some(_) => {
                        let to = aside(path, absorbed);
                        changes.push(brought(to.clone()));
                        set_aside.push(to);
                    }
                    None => changes.push(brought(path.clone())),
                }
            }
            for (path, blob) in removed {
                if kept.get(path) == Some(blob) {
                    changes
                        .push(Change::Remove { path: path.clone() });
                }
            }
            let also =
                git.stage(None, written, message, By::Realm)?;
            (before, also)
        }
    };
    set_aside.sort();
    Ok(Staged {
        files: git.stage(Some(onto), &changes, message, By::Realm)?,
        also,
        set_aside,
    })
}

fn tell_hire<M>(
    writer: &mut Writer<'_, Profile, M>,
    branch: &str,
    tip: CommitId,
) -> Result<()>
where
    M: Message,
    Profile: Admits<M::From>,
{
    let hire = hires::every(writer)?
        .into_iter()
        .find(|hire| hire.branch == branch);
    match hire {
        Some(hire) => hires::tell_moved(writer, &hire, tip),
        None => Ok(()),
    }
}

/// The commit ids `T`'s rows hold.
fn ids<T: store::Table>(
    rows: &Rows<'_, T>,
) -> Result<HashSet<String>> {
    rows.select(&format!("SELECT id FROM {}", T::NAME), Vec::new())?
        .into_iter()
        .map(|row| match row.as_slice() {
            [ColumnValue::Text(id)] => Ok(id.clone()),
            _ => bail!("a {} row of another shape: {row:?}", T::NAME),
        })
        .collect()
}

fn next_part(rows: &Rows<'_, Calls>, id: &str) -> Result<u32> {
    let found = rows.select(
        "SELECT next_part FROM merge_call WHERE id = ?1",
        vec![BindParam::Text(id.to_owned())],
    )?;
    match found.first().map(Vec::as_slice) {
        None => Ok(0),
        Some([ColumnValue::Integer(next)]) => {
            Ok(u32::try_from(*next)?)
        }
        Some(row) => {
            bail!("a merge_call row of another shape: {row:?}")
        }
    }
}

fn keep_part(
    writer: &Writer<'_, Profile, MergeBranch>,
    id: &str,
    part: u32,
    files: Vec<Piece>,
    removed: Vec<Removed>,
) -> Result<()> {
    let id = || BindParam::Text(id.to_owned());
    writer.table(Calls(())).execute(
        "INSERT OR REPLACE INTO merge_call (id, next_part) VALUES \
         (?1, ?2)",
        vec![id(), BindParam::Integer(i64::from(part) + 1)],
    )?;
    let table = writer.table(Parts(()));
    let insert = "INSERT OR REPLACE INTO merge_part (id, path, \
                  offset, bytes, last, executable, blob) VALUES \
                  (?1, ?2, ?3, ?4, ?5, ?6, ?7)";
    for piece in files {
        table.execute(
            insert,
            vec![
                id(),
                BindParam::Text(piece.path),
                BindParam::Integer(i64::from(piece.offset)),
                BindParam::Blob(piece.bytes.0),
                BindParam::Integer(i64::from(piece.last)),
                BindParam::Integer(i64::from(piece.executable)),
                BindParam::Null,
            ],
        )?;
    }
    for gone in removed {
        table.execute(
            insert,
            vec![
                id(),
                BindParam::Text(gone.path),
                BindParam::Integer(0),
                BindParam::Null,
                BindParam::Integer(1),
                BindParam::Integer(0),
                BindParam::Text(gone.blob.to_string()),
            ],
        )?;
    }
    Ok(())
}

/// Every part of the merge `id`, joined into files, and its rows
/// dropped.
fn take_parts(
    writer: &Writer<'_, Profile, MergeBranch>,
    id: &str,
) -> Result<Learned> {
    let table = writer.table(Parts(()));
    let bound = || vec![BindParam::Text(id.to_owned())];
    let pieces = table
        .rows()
        .select(
            "SELECT path, offset, bytes, last, executable FROM \
             merge_part WHERE id = ?1 AND blob IS NULL ORDER BY \
             path, offset",
            bound(),
        )?
        .iter()
        .map(|row| Kept::of(row))
        .collect::<Result<Vec<_>>>()?;
    let removed = table
        .rows()
        .select(
            "SELECT path, blob FROM merge_part WHERE id = ?1 AND \
             blob IS NOT NULL ORDER BY path",
            bound(),
        )?
        .into_iter()
        .map(|row| match row.as_slice() {
            [ColumnValue::Text(path), ColumnValue::Text(blob)] => {
                Ok((
                    path.clone(),
                    blob.parse::<BlobId>()
                        .map_err(anyhow::Error::msg)?,
                ))
            }
            _ => bail!("a merge_part row of another shape: {row:?}"),
        })
        .collect::<Result<Vec<_>>>()?;
    table.execute("DELETE FROM merge_part WHERE id = ?1", bound())?;
    writer
        .table(Calls(()))
        .execute("DELETE FROM merge_call WHERE id = ?1", bound())?;
    Ok(Learned::Files {
        written: joined(pieces)?,
        removed,
    })
}

impl Takes<Bundle> for Profile {
    async fn take(call: Call<Self, Bundle>) -> Result<Reply<Bundle>> {
        call.read(bundle).await
    }
}

fn bundle(look: &Look<'_, Profile, Bundle>) -> Result<BundleAnswer> {
    let Bundle { branch, after } = look.message();
    let git = look.git()?;
    let tip = match after {
        Some(cursor) => {
            let commit = git_commit(cursor.commit)?;
            if !(git.has(commit)? && git.holds(branch, commit)?) {
                return Ok(BundleAnswer::Refused {
                    why: Gone::CommitGone {
                        commit: cursor.commit,
                    },
                });
            }
            commit
        }
        None => match git.tip(branch)? {
            Some(tip) => tip,
            None => {
                return Ok(BundleAnswer::Part {
                    files: Vec::new(),
                    removed: Vec::new(),
                    next: None,
                });
            }
        },
    };
    let main = git.tip(MAIN)?.context("a profile has a main")?;
    let items = items(git, git.merge_base(tip, main)?, tip)?;
    let from = after.as_ref().map_or((0, 0), |cursor| {
        (cursor.file as usize, cursor.offset as usize)
    });
    let packed =
        parts::pack(&items, from, MOST_PART, &|blob| git.blob(blob))?;
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
    let removed = packed
        .removed
        .into_iter()
        .map(|(path, blob)| {
            let blob = Sha1::from_hex(&blob.to_string())
                .context("a blob's id is a SHA-1")?;
            Ok(Removed { path, blob })
        })
        .collect::<Result<_>>()?;
    Ok(BundleAnswer::Part {
        files: packed.files,
        removed,
        next,
    })
}

impl Takes<Unmerge> for Profile {
    async fn take(
        call: Call<Self, Unmerge>,
    ) -> Result<Reply<Unmerge>> {
        call.once(unmerge).await
    }
}

/// Undoes the merge on its branch as one commit, the branch's later
/// lines winning where they changed what the merge brought.
fn unmerge(
    unit: Before<'_, Profile, Unmerge>,
) -> Result<Ended<Unmerge>> {
    let Unmerge { branch, merge, .. } = unit.message().clone();
    let at = git_commit(merge)?;
    {
        let git = unit.git()?;
        if !(git.has(at)? && git.holds(&branch, at)?) {
            return unit.refuse(Gone::CommitGone { commit: merge });
        }
        ensure!(
            ids(&unit.rows(Merges(())))?.contains(&at.to_string()),
            "the commit {at} is not a merge"
        );
    }
    let mut writer = unit.write();
    let after =
        writer.writes()?.revert(&branch, at, SPLIT, By::Realm)?;
    tell_hire(&mut writer, &branch, after.commit)?;
    writer.kept(UnmergeAnswer::Unmerged {
        tip: commit_id(after.commit)?,
    })
}

impl Takes<DropBranch> for Profile {
    async fn take(
        call: Call<Self, DropBranch>,
    ) -> Result<Reply<DropBranch>> {
        call.once(drop_branch).await
    }
}

/// Removes the branch and its hire's row with the parts of its save,
/// each when it is there.
fn drop_branch(
    unit: Before<'_, Profile, DropBranch>,
) -> Result<Ended<DropBranch>> {
    let name = unit.message().name.clone();
    let has_branch = unit.git()?.tip(&name)?.is_some();
    let hire = hires::every(&unit)?
        .into_iter()
        .find(|hire| hire.branch == name);
    if !has_branch && hire.is_none() {
        return unit.kept(());
    }
    let writer = unit.write();
    if has_branch {
        writer.writes()?.delete_branch(&name)?;
    }
    if let Some(hire) = hire {
        hires::forget(&writer, hire.agent)?;
        drop_saves(&writer, Some(hire.agent))?;
    }
    writer.kept(())
}

#[cfg(test)]
mod tests {
    use hamlet_git::{Author, Repository};
    use hamlet_host::Caller;
    use hamlet_messages::address::{Party, Realm};
    use hamlet_messages::bytes::Base64;
    use hamlet_protocol::client::RequestId;
    use hamlet_testing::Scratch;

    use super::*;
    use crate::repo::testing::Hostless;

    fn repository(dir: &Scratch) -> Repository {
        Repository::open(&dir.join("scholars/1.git")).unwrap()
    }

    /// Profile 1 with branches `hires/7` and `hires/8` at `main`,
    /// each of which changed `notes.md`.
    fn two_branches(dir: &Scratch) -> Hostless {
        let profile = Hostless::profile(dir, "Careful.");
        let repository = repository(dir);
        let main = repository.tip(MAIN).unwrap().unwrap();
        let by = Author {
            name: "agent:7",
            at: 10,
        };
        for (branch, text) in
            [("hires/7", "seven"), ("hires/8", "eight")]
        {
            repository.branch(branch, main).unwrap();
            let notes = Change::Write {
                path: "notes.md".into(),
                bytes: text.into(),
            };
            repository.commit(branch, &[notes], "saved", by).unwrap();
        }
        profile
    }

    fn merge(id: &RequestId, from: Source) -> MergeBranch {
        MergeBranch {
            id: id.clone(),
            into: "hires/7".into(),
            absorbed: AgentNumber(8),
            message: "Took in what hire#8 learned: merged".into(),
            from,
        }
    }

    fn realm() -> Caller {
        Caller::Actor(Realm.address().unwrap())
    }

    /// Runs the merge's unit as far as a kill after its commit and
    /// before the store's: the branch moved, no row was kept.
    fn cut_off(profile: &mut Hostless, asked: MergeBranch) {
        profile
            .once(realm(), asked, |unit| {
                merge_branch(unit)?;
                bail!("killed")
            })
            .unwrap_err();
    }

    fn commit_count(dir: &Scratch) -> usize {
        let repository = repository(dir);
        let tip = repository.tip("hires/7").unwrap().unwrap();
        repository.history(tip, usize::MAX).unwrap().len()
    }

    #[test]
    fn a_merge_cut_off_before_its_rows_is_found_by_its_repeat() {
        let dir = Scratch::new("profile-merge-cut");
        let mut profile = two_branches(&dir);
        let asked = merge(
            &RequestId::new(),
            Source::Branch {
                name: "hires/8".into(),
            },
        );
        cut_off(&mut profile, asked.clone());
        let tip = repository(&dir).tip("hires/7").unwrap().unwrap();
        let count = commit_count(&dir);

        let (answer, _) =
            profile.once(realm(), asked, merge_branch).unwrap();
        assert_eq!(
            answer,
            MergeAnswer::Merged {
                tip: commit_id(tip).unwrap(),
                set_aside: Vec::new(),
            }
        );
        assert_eq!(commit_count(&dir), count);
    }

    #[test]
    fn a_last_part_cut_off_before_its_rows_is_found_by_its_repeat() {
        let dir = Scratch::new("profile-merge-part-cut");
        let mut profile = two_branches(&dir);
        let id = RequestId::new();
        let part = |part, last, path: &str| {
            merge(
                &id,
                Source::Part {
                    part,
                    last,
                    files: vec![Piece {
                        path: path.into(),
                        offset: 0,
                        bytes: Base64(b"theirs".to_vec()),
                        last: true,
                        executable: false,
                    }],
                    removed: Vec::new(),
                },
            )
        };
        let (kept, _) = profile
            .once(realm(), part(0, false, "notes.md"), merge_branch)
            .unwrap();
        assert_eq!(kept, MergeAnswer::Kept);
        cut_off(&mut profile, part(1, true, "new.md"));
        let tip = repository(&dir).tip("hires/7").unwrap().unwrap();
        let count = commit_count(&dir);

        let (answer, _) = profile
            .once(realm(), part(1, true, "new.md"), merge_branch)
            .unwrap();
        assert_eq!(
            answer,
            MergeAnswer::Merged {
                tip: commit_id(tip).unwrap(),
                set_aside: vec!["notes.md.from-hire-8".into()],
            }
        );
        assert_eq!(commit_count(&dir), count);
        let new = repository(&dir).read(tip, "new.md").unwrap();
        assert_eq!(new.as_deref(), Some(&b"theirs"[..]));
    }
}
