//! The store the realm keeps parcels' files in.
//!
//! The store's rule: a file is named by its hash and written whole
//! under a temporary name before its rename, so a kill leaves at most
//! a temporary file; a parcel's steps driven again at the realm's
//! start copy what is missing; at the start and after each parcel's
//! steps, the realm removes the files no unswept parcel names and the
//! temporary files no copy is writing. No file changes after its
//! rename.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_host::HostRef;
use hamlet_messages::address::{self, Party as _, Workspace};
use hamlet_messages::bytes::Sha256;
use hamlet_messages::realm::LetterId;
use hamlet_messages::realm::parcels::{ParcelId, Part};
use hamlet_messages::workspace::{
    AgentNumber, ParcelEntry, ParcelFault, ParcelPart, ParcelRefusal,
};
use sha2::Digest as _;
use tokio::io::AsyncWriteExt as _;

use super::super::Realm;
use super::super::change::{self, Rows};
use super::super::oversight::asking;
use super::super::peer::When;
use super::super::steps::Cx;
use super::{
    Change, KEPT_MS, Sent, State, either_left, hash_of, state,
};

/// Copies each file the store lacks from the sender's workspace,
/// part by part, checking its hash before its rename.
pub(super) async fn copy(
    cx: &Cx<'_>,
    sent: &Sent,
) -> Result<Vec<change::Change>> {
    let store = Store::of(cx.ctx)?;
    tokio::fs::create_dir_all(&store.0).await?;
    for (file, entry) in sent.files.iter().enumerate() {
        let kept = store.file(&entry.sha256);
        if tokio::fs::try_exists(&kept).await? {
            continue;
        }
        let temporary = store.temporary(sent.parcel, file);
        let copied =
            copy_file(cx, sent.sender, entry, &temporary).await;
        if !matches!(copied, Ok(None)) {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        if let Some(why) = copied? {
            return sent.refused(cx, &why).await;
        }
        tokio::fs::rename(&temporary, &kept).await?;
    }
    Ok(Vec::new())
}

/// Writes `entry` whole to `temporary`, or says why it cannot.
async fn copy_file(
    cx: &Cx<'_>,
    sender: AgentNumber,
    entry: &ParcelEntry,
    temporary: &std::path::Path,
) -> Result<Option<ParcelRefusal>> {
    let mut out = tokio::fs::File::create(temporary).await?;
    let mut hash = sha2::Sha256::new();
    let mut written = 0u64;
    let mut part = 0;
    loop {
        let asked = ParcelPart {
            agent: sender,
            path: entry.path.clone(),
            part,
        };
        let (bytes, last) = match cx
            .peers()
            .call(&Workspace, &asked, When::AfterAnswer)
            .await?
        {
            Part::Bytes { bytes, last } => (bytes.0, last),
            Part::Refused {
                why:
                    ParcelRefusal::Parcel(ParcelFault::NoSuchPart {
                        ..
                    }),
            } => return Ok(Some(changed(entry))),
            Part::Refused { why } => return Ok(Some(why)),
        };
        written += bytes.len() as u64;
        if written > entry.size {
            break;
        }
        hash = hash.chain_update(&bytes);
        out.write_all(&bytes).await?;
        if last {
            break;
        }
        part += 1;
    }
    out.sync_all().await?;
    if written != entry.size
        || Sha256(hash.finalize().into()) != entry.sha256
    {
        return Ok(Some(changed(entry)));
    }
    Ok(None)
}

/// Why `entry` is not sent when it grew or shrank, or its bytes
/// changed, after its measure.
fn changed(entry: &ParcelEntry) -> ParcelRefusal {
    ParcelFault::Unreadable {
        path: entry.name.clone(),
        words: "it changed while it was sent.".to_owned(),
    }
    .into()
}

/// Removes the realm's files no unswept parcel names, and the
/// temporary files no copy is writing. First it ends the parcels done
/// with, as [`done`] says.
pub(in crate::realm) async fn sweep(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
) -> Result<()> {
    let changes = done(&Rows::outside(ctx)?).await?;
    if !changes.is_empty() {
        change::commit(realm, ctx, changes).await?;
    }
    let store = Store::of(ctx)?;
    let listed = match tokio::fs::read_dir(&store.0).await {
        Ok(listed) => listed,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let names = names_in(listed).await?;
    let tx = change::Transaction::begin(ctx).await?;
    let rows = tx.rows();
    let named = rows
        .values::<Vec<u8>>(
            "SELECT DISTINCT parcel_file.sha256 FROM parcel_file \
             JOIN parcel ON parcel.id = parcel_file.parcel WHERE \
             parcel.swept_at IS NULL",
            Vec::new(),
        )
        .await?
        .into_iter()
        .map(|sha256| Ok(hash_of(sha256)?.to_string()))
        .collect::<Result<HashSet<_>>>()?;
    let copying = rows
        .values::<u32>(
            "SELECT id FROM parcel WHERE state = ?1",
            vec![state(State::Copying)?],
        )
        .await?
        .into_iter()
        .collect::<HashSet<_>>();
    for name in names {
        let stays = match Store::kept_as(&name) {
            Kept::File => named.contains(&name),
            Kept::Temporary(parcel) => copying.contains(&parcel),
            Kept::Other => true,
        };
        if !stays {
            match tokio::fs::remove_file(store.0.join(&name)).await {
                Err(error)
                    if error.kind()
                        != std::io::ErrorKind::NotFound =>
                {
                    return Err(error.into());
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The changes that end the parcels done with: one waiting for a
/// chief while its sender or receiver has ended is refused, and one
/// refused, or delivered a day ago and not held, is swept.
async fn done(rows: &Rows<'_>) -> Result<Vec<change::Change>> {
    let unswept = rows
        .rows::<(u32, u32, u32, u32, String, i64)>(
            "SELECT id, letter, sender, receiver, state, at FROM \
             parcel WHERE swept_at IS NULL AND state IN (?1, ?2, ?3)",
            vec![
                state(State::Waiting)?,
                state(State::Ready)?,
                state(State::Refused)?,
            ],
        )
        .await?;
    let old = rows.at().0 - KEPT_MS;
    let mut changes = Vec::new();
    let mut swept = Vec::new();
    for (parcel, letter, sender, receiver, was, at) in unswept {
        let parcel = ParcelId(parcel);
        let sweeps = match change::from_case_name(was)? {
            State::Refused => true,
            State::Ready => {
                at < old
                    && !asking::still_held(rows, LetterId(letter))
                        .await?
            }
            State::Waiting => {
                let left = either_left(
                    rows,
                    AgentNumber(sender),
                    AgentNumber(receiver),
                )
                .await?;
                if left {
                    changes.push(Change::Refused { parcel }.into());
                }
                left
            }
            State::Measuring | State::Copying => false,
        };
        if sweeps {
            swept.push(parcel);
        }
    }
    if !swept.is_empty() {
        changes.push(Change::Swept { parcels: swept }.into());
    }
    Ok(changes)
}

async fn names_in(
    mut listed: tokio::fs::ReadDir,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    while let Some(entry) = listed.next_entry().await? {
        if let Ok(name) = entry.file_name().into_string() {
            names.push(name);
        }
    }
    Ok(names)
}

/// The realm's folder of parcels' files, `local.files` beside its
/// database.
pub(super) struct Store(PathBuf);

enum Kept {
    /// A file named by its hash.
    File,
    /// A file a copy of this parcel is writing.
    Temporary(u32),
    Other,
}

impl Store {
    pub(super) fn of(ctx: &Ctx<Realm>) -> Result<Self> {
        let realm = address::Realm.address()?;
        Ok(Self(
            HostRef::of(ctx.inner())?
                .folder()
                .join("actors")
                .join(realm.kind())
                .join(format!("{}.files", realm.key())),
        ))
    }

    pub(super) fn file(&self, sha256: &Sha256) -> PathBuf {
        self.0.join(sha256.to_string())
    }

    fn temporary(&self, parcel: ParcelId, file: usize) -> PathBuf {
        self.0.join(format!("{}.{file}.part", parcel.0))
    }

    fn kept_as(name: &str) -> Kept {
        if name.len() == 64
            && name.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Kept::File;
        }
        name.strip_suffix(".part")
            .and_then(|rest| rest.split_once('.'))
            .and_then(|(parcel, _)| parcel.parse().ok())
            .map_or(Kept::Other, Kept::Temporary)
    }
}
