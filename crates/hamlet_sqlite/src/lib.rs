//! Runs the actor runtime's three SQL requests, `exec`, `execute` and
//! a batch, on a rusqlite connection, and reads their rows into the
//! runtime's types. `hamlet_host` serves each actor's file with it,
//! and the actor core's tests serve their in-memory database.

use anyhow::Result;
use hamlet_actor_wire::sqlite_types::{
    BindParam, ColumnValue, ExecuteResult, QueryResult,
};
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Batch, Connection, Statement};

/// Runs each statement of `sql` and returns the rows of the last one
/// that has columns.
pub fn exec(conn: &Connection, sql: &str) -> Result<QueryResult> {
    let mut result = QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
    };
    let mut batch = Batch::new(conn, sql);
    while let Some(mut stmt) = batch.next()? {
        let (columns, rows) = read_rows(&mut stmt)?;
        if !columns.is_empty() {
            result = QueryResult { columns, rows };
        }
    }
    Ok(result)
}

/// Runs one statement with its parameters. A statement that only
/// reads reports no changes and no inserted row.
pub fn execute(
    conn: &Connection,
    sql: &str,
    params: Option<&[BindParam]>,
) -> Result<ExecuteResult> {
    let mut stmt = conn.prepare(sql)?;
    for (index, param) in
        params.unwrap_or_default().iter().enumerate()
    {
        stmt.raw_bind_parameter(index + 1, value(param))?;
    }
    let readonly = stmt.readonly();
    let (columns, rows) = read_rows(&mut stmt)?;
    Ok(ExecuteResult {
        columns,
        rows,
        changes: if readonly { 0 } else { conn.changes() as i64 },
        last_insert_row_id: (!readonly)
            .then(|| conn.last_insert_rowid()),
    })
}

/// Runs the statements, each `(sql, params)`, as one transaction,
/// rolled back when one fails.
pub fn batch<'a>(
    conn: &Connection,
    statements: impl IntoIterator<
        Item = (&'a str, Option<&'a [BindParam]>),
    >,
) -> Result<Vec<ExecuteResult>> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let mut results = Vec::new();
    for (sql, params) in statements {
        match execute(conn, sql, params) {
            Ok(result) => results.push(result),
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(results)
}

/// The value rusqlite binds for a parameter.
pub fn value(param: &BindParam) -> Value {
    match param {
        BindParam::Null => Value::Null,
        BindParam::Integer(v) => Value::Integer(*v),
        BindParam::Float(v) => Value::Real(*v),
        BindParam::Text(v) => Value::Text(v.clone()),
        BindParam::Blob(v) => Value::Blob(v.clone()),
    }
}

fn read_rows(
    stmt: &mut Statement<'_>,
) -> Result<(Vec<String>, Vec<Vec<ColumnValue>>)> {
    let columns: Vec<String> = stmt
        .column_names()
        .iter()
        .map(ToString::to_string)
        .collect();
    let width = columns.len();
    let mut rows = Vec::new();
    let mut cursor = stmt.raw_query();
    while let Some(row) = cursor.next()? {
        rows.push(
            (0..width)
                .map(|index| Ok(column(row.get_ref(index)?)))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    Ok((columns, rows))
}

fn column(value: ValueRef<'_>) -> ColumnValue {
    match value {
        ValueRef::Null => ColumnValue::Null,
        ValueRef::Integer(v) => ColumnValue::Integer(v),
        ValueRef::Real(v) => ColumnValue::Float(v),
        ValueRef::Text(v) => {
            ColumnValue::Text(String::from_utf8_lossy(v).into_owned())
        }
        ValueRef::Blob(v) => ColumnValue::Blob(v.to_vec()),
    }
}
