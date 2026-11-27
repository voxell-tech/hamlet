//! A parcel's side at its receiver: the files it is told of and the
//! parts it fetches.

use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::address::Agent;
use hamlet_messages::bytes::Base64;
use hamlet_messages::realm::LetterId;
use hamlet_messages::realm::parcels::{
    FetchPart, PARCEL_PART, ParcelFile, ParcelId, Part, PartRefusal,
    part_start,
};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::AgentNumber;
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};

use super::super::change::{self, Rows};
use super::super::{Realm, record, tree};
use super::{State, entries, hash_of, held, id, state, store};

/// The files of `letter`'s parcel, as its receiver is told of them:
/// none until they are copied, or once they are swept.
pub(in crate::realm) async fn files(
    rows: &Rows<'_>,
    letter: LetterId,
) -> Result<Vec<ParcelFile>> {
    let Some((parcel, sender)) = rows
        .row::<(u32, u32)>(
            "SELECT id, sender FROM parcel WHERE letter = ?1 AND \
             state = ?2 AND swept_at IS NULL",
            vec![change::integer(letter.0)?, state(State::Ready)?],
        )
        .await?
    else {
        return Ok(Vec::new());
    };
    let from = record::address(rows, AgentNumber(sender))
        .await?
        .replace('#', "-");
    let parcel = ParcelId(parcel);
    entries(rows, parcel)
        .await?
        .into_iter()
        .enumerate()
        .map(|(file, entry)| {
            Ok(ParcelFile {
                parcel,
                file: u32::try_from(file)?,
                path: PathBuf::from(format!("from-{from}"))
                    .join(entry.name),
                size: entry.size,
            })
        })
        .collect()
}

impl Handler<FetchPart> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        fetch: FetchPart,
    ) -> Result<Part<PartRefusal>> {
        let Agent(by) = sender(&ctx, &fetch)?;
        let rows = Rows::outside(&ctx)?;
        let Some((receiver, letter, state, swept)) = rows
            .row::<(u32, Option<u32>, String, Option<i64>)>(
                "SELECT receiver, letter, state, swept_at FROM \
                 parcel WHERE id = ?1",
                vec![id(fetch.parcel)],
            )
            .await?
        else {
            return Ok(Part::Refused {
                why: PartRefusal::NoSuchParcel,
            });
        };
        if AgentNumber(receiver) != by {
            return Ok(Part::Refused {
                why: PartRefusal::NotYours,
            });
        }
        if change::from_case_name::<State>(state)? != State::Ready
            || swept.is_some()
            || tree::left(&rows, by).await?.is_some()
            || held(&rows, letter).await?
        {
            return Ok(Part::Refused {
                why: PartRefusal::NoSuchParcel,
            });
        }
        let file = rows
            .row::<(u64, Vec<u8>)>(
                "SELECT size, sha256 FROM parcel_file WHERE parcel \
                 = ?1 AND file = ?2",
                vec![id(fetch.parcel), change::integer(fetch.file)?],
            )
            .await?;
        let Some((size, sha256)) = file else {
            return Ok(Part::Refused {
                why: PartRefusal::NoSuchPart,
            });
        };
        let Some(start) = part_start(size, fetch.part) else {
            return Ok(Part::Refused {
                why: PartRefusal::NoSuchPart,
            });
        };
        let kept = store::Store::of(&ctx)?.file(&hash_of(sha256)?);
        let mut file = tokio::fs::File::open(&kept).await?;
        file.seek(SeekFrom::Start(start)).await?;
        let mut bytes = Vec::new();
        file.take(PARCEL_PART as u64)
            .read_to_end(&mut bytes)
            .await?;
        let last = start + bytes.len() as u64 >= size;
        Ok(Part::Bytes {
            bytes: Base64(bytes),
            last,
        })
    }
}
