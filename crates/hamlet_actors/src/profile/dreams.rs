//! The `dream` table: one row per dream of this profile or of one of
//! its hires.

use anyhow::{Result, bail};
use hamlet_actor::ColumnValue;
use hamlet_messages::Message;
use hamlet_messages::profile::DreamId;

use super::Profile;
use crate::repo::{Admits, Look, store};

/// The token of the `dream` table.
struct Table(());

impl store::Table for Table {
    const NAME: &'static str = "dream";
}

/// The dream that has not ended, when there is one.
pub(super) fn open<M>(
    look: &Look<'_, Profile, M>,
) -> Result<Option<DreamId>>
where
    M: Message,
    Profile: Admits<M::From>,
{
    let rows = look.rows(Table(())).select(
        "SELECT id FROM dream WHERE outcome IS NULL ORDER BY id \
         LIMIT 1",
        Vec::new(),
    )?;
    match rows.first().map(Vec::as_slice) {
        None => Ok(None),
        Some([ColumnValue::Integer(id)]) => {
            Ok(Some(DreamId(u32::try_from(*id)?)))
        }
        Some(row) => bail!("a dream row of another shape: {row:?}"),
    }
}
