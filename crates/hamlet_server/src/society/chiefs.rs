//! The humans the realm answers to.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::ChiefId;
use hamlet_store::schema::chief;

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = chief)]
pub(crate) struct Chief {
    pub id: ChiefId,
    pub name: String,
    pub since: i64,
}

/// The chief named `name`, made the first time it is asked for.
pub(crate) fn ensure(
    db: &mut Db,
    name: &str,
    at: i64,
) -> Result<ChiefId> {
    if let Some(id) = chief::table
        .filter(chief::name.eq(name))
        .select(chief::id)
        .first::<ChiefId>(db)
        .optional()?
    {
        return Ok(id);
    }
    Ok(diesel::insert_into(chief::table)
        .values((chief::name.eq(name), chief::since.eq(at)))
        .returning(chief::id)
        .get_result(db)?)
}

pub(crate) fn find(
    db: &mut Db,
    id: ChiefId,
) -> Result<Option<Chief>> {
    Ok(chief::table
        .find(id)
        .select(Chief::as_select())
        .first(db)
        .optional()?)
}

/// The chief who came first, who keeps what no other chief does.
pub(crate) fn first(db: &mut Db) -> Result<Option<ChiefId>> {
    Ok(chief::table
        .order(chief::id)
        .select(chief::id)
        .first(db)
        .optional()?)
}
