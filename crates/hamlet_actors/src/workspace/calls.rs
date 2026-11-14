//! The record of every call that changes something, by the agent and
//! the id it gave the call, which makes a repeat of the call safe: a
//! command's and a file change's alike.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hamlet_actor_core::{BindParam, ColumnValue, SqliteTransaction};
use hamlet_messages::workspace::{AgentNumber, CallId};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// One row per call. A command's id is its row's. A change records
/// the path of the new file it is about to make in `temp` until it
/// ends. A call that waits for a chief is `waiting`, its `answer` the
/// CBOR list of what it needs, and its request is in `waits`; one
/// that a chief refused, or whose wait lapsed, is `refused`, its
/// `answer` the CBOR refusal.
pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS calls (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	agent INTEGER NOT NULL,
	call TEXT NOT NULL,
	op TEXT NOT NULL,
	request_hash BLOB NOT NULL,
	state TEXT NOT NULL,
	answer BLOB,
	temp TEXT,
	opened_at INTEGER NOT NULL,
	ended_at INTEGER,
	UNIQUE (agent, call)) STRICT";

/// A call's row as it is stored.
pub(super) struct Call {
    pub id: i64,
    pub op: String,
    pub request_hash: Vec<u8>,
    pub state: String,
    pub answer: Option<Vec<u8>>,
    pub temp: Option<String>,
}

impl Call {
    /// Whether this row is of a call of `op` with the request hashed
    /// to `request_hash`: else its id was given to another call.
    pub fn is(&self, op: &str, request_hash: &[u8]) -> bool {
        self.op == op && self.request_hash == request_hash
    }
}

/// The hash of `request` that a repeat of its call must match.
pub(super) fn request_hash(
    request: &impl Serialize,
) -> Result<Vec<u8>> {
    Ok(Sha256::digest(cbor(request)?).to_vec())
}

/// `value` in CBOR, as a row keeps it.
pub(super) fn cbor(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes)?;
    Ok(bytes)
}

const COLUMNS: &str = "id, op, request_hash, state, answer, temp";

/// The row of `agent`'s call `call`, in `tx`.
pub(super) async fn find(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    call: &CallId,
) -> Result<Option<Call>> {
    let read = tx
        .execute(
            &format!(
                "SELECT {COLUMNS} FROM calls WHERE agent = ?1 AND \
                 call = ?2"
            ),
            Some(vec![
                BindParam::Integer(i64::try_from(agent.0)?),
                BindParam::Text(call.0.clone()),
            ]),
        )
        .await?;
    read.rows.first().map(|row| read_row(row)).transpose()
}

/// The row `id` of `agent`'s calls of `op`, in `tx`.
pub(super) async fn by_id(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    id: i64,
    op: &str,
) -> Result<Option<Call>> {
    let read = tx
        .execute(
            &format!(
                "SELECT {COLUMNS} FROM calls WHERE agent = ?1 AND \
                 id = ?2 AND op = ?3"
            ),
            Some(vec![
                BindParam::Integer(i64::try_from(agent.0)?),
                BindParam::Integer(id),
                BindParam::Text(op.to_owned()),
            ]),
        )
        .await?;
    read.rows.first().map(|row| read_row(row)).transpose()
}

fn read_row(row: &[ColumnValue]) -> Result<Call> {
    let [
        ColumnValue::Integer(id),
        ColumnValue::Text(op),
        ColumnValue::Blob(request_hash),
        ColumnValue::Text(state),
        answer,
        temp,
    ] = row
    else {
        bail!("a call's row has another shape: {row:?}");
    };
    Ok(Call {
        id: *id,
        op: op.clone(),
        request_hash: request_hash.clone(),
        state: state.clone(),
        answer: match answer {
            ColumnValue::Blob(answer) => Some(answer.clone()),
            _ => None,
        },
        temp: match temp {
            ColumnValue::Text(temp) => Some(temp.clone()),
            _ => None,
        },
    })
}

/// Records `agent`'s call `call` of `op`, in `state`, in `tx`.
/// Returns its row's id.
#[expect(clippy::too_many_arguments, reason = "a row's columns")]
pub(super) async fn insert(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    call: &CallId,
    op: &str,
    request_hash: Vec<u8>,
    state: &str,
    answer: Option<Vec<u8>>,
    temp: Option<&str>,
) -> Result<i64> {
    let inserted = tx
        .execute(
            "INSERT INTO calls (agent, call, op, request_hash, \
             state, answer, temp, opened_at) VALUES (?1, ?2, ?3, \
             ?4, ?5, ?6, ?7, ?8)",
            Some(vec![
                BindParam::Integer(i64::try_from(agent.0)?),
                BindParam::Text(call.0.clone()),
                BindParam::Text(op.to_owned()),
                BindParam::Blob(request_hash),
                BindParam::Text(state.to_owned()),
                answer.map_or(BindParam::Null, BindParam::Blob),
                temp.map_or(BindParam::Null, |temp| {
                    BindParam::Text(temp.to_owned())
                }),
                BindParam::Integer(now_ms()?),
            ]),
        )
        .await?;
    inserted
        .last_insert_row_id
        .context("the insert of a call gave no id")
}

pub(super) fn now_ms() -> Result<i64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(i64::try_from(now.as_millis())?)
}
