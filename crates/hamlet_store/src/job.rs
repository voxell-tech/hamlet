//! Jobs: effects kept with the change that needs them, run after the
//! commit and again after a crash until done.

use anyhow::Result;
use diesel::prelude::*;
use serde::Serialize;

use crate::Db;
use crate::ids::JobId;
use crate::schema::job;

/// A job waiting to run.
#[derive(Clone, Debug, PartialEq, Queryable)]
pub struct Job {
    pub id: JobId,
    pub kind: String,
    pub payload: String,
    pub tries: i64,
}

/// Keeps a job of `kind`. With a `key`, a job of that key already
/// waiting is kept instead and its id returned.
pub fn add(
    db: &mut Db,
    kind: &str,
    key: Option<&str>,
    payload: &impl Serialize,
    at: i64,
) -> Result<JobId> {
    if let Some(key) = key
        && let Some(id) = job::table
            .filter(job::key.eq(key))
            .filter(job::done.is_null())
            .select(job::id)
            .first::<JobId>(db)
            .optional()?
    {
        return Ok(id);
    }
    Ok(diesel::insert_into(job::table)
        .values((
            job::kind.eq(kind),
            job::key.eq(key),
            job::payload.eq(serde_json::to_string(payload)?),
            job::made.eq(at),
        ))
        .returning(job::id)
        .get_result(db)?)
}

/// The jobs not done whose time has come at `now`, oldest first.
pub fn due(db: &mut Db, now: i64) -> Result<Vec<Job>> {
    Ok(job::table
        .filter(job::done.is_null())
        .filter(job::after.is_null().or(job::after.le(now)))
        .order(job::id)
        .select((job::id, job::kind, job::payload, job::tries))
        .load(db)?)
}

/// The earliest time a job waits for, if any waits.
pub fn next(db: &mut Db) -> Result<Option<i64>> {
    Ok(job::table
        .filter(job::done.is_null())
        .select(diesel::dsl::min(job::after))
        .first::<Option<i64>>(db)?)
}

pub fn finish(db: &mut Db, id: JobId, at: i64) -> Result<()> {
    diesel::update(job::table.find(id))
        .set(job::done.eq(at))
        .execute(db)?;
    Ok(())
}

/// Keeps why a run of the job failed, to try again from `after`.
pub fn retry(
    db: &mut Db,
    id: JobId,
    failure: &str,
    after: i64,
) -> Result<()> {
    diesel::update(job::table.find(id))
        .set((
            job::tries.eq(job::tries + 1),
            job::failure.eq(failure),
            job::after.eq(after),
        ))
        .execute(db)?;
    Ok(())
}
