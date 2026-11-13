//! Runs the actor runtime's three SQL requests, `exec`, `execute` and
//! a batch, on a rusqlite connection, and reads their rows into the
//! runtime's types.

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
/// rolled back when one fails or the commit fails.
pub fn batch<'a>(
    conn: &Connection,
    statements: impl IntoIterator<
        Item = (&'a str, Option<&'a [BindParam]>),
    >,
) -> Result<Vec<ExecuteResult>> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let run = || {
        let results = statements
            .into_iter()
            .map(|(sql, params)| execute(conn, sql, params))
            .collect::<Result<Vec<_>>>()?;
        conn.execute_batch("COMMIT")?;
        Ok(results)
    };
    let results = run();
    if results.is_err() && !conn.is_autocommit() {
        let _ = conn.execute_batch("ROLLBACK");
    }
    results
}

fn value(param: &BindParam) -> Value {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A batch whose `COMMIT` fails, here on a deferred foreign key,
    /// leaves no transaction open, so later writes commit.
    #[test]
    fn a_failed_commit_rolls_the_batch_back() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE parent (id INTEGER PRIMARY KEY);
             CREATE TABLE child (parent INTEGER REFERENCES parent \
             (id)
                 DEFERRABLE INITIALLY DEFERRED);",
        )
        .unwrap();

        let failed =
            batch(&conn, [("INSERT INTO child VALUES (1)", None)]);
        assert!(failed.is_err());
        assert!(
            conn.is_autocommit(),
            "the batch left a transaction open"
        );

        execute(&conn, "INSERT INTO parent VALUES (1)", None)
            .unwrap();
        let rows =
            exec(&conn, "SELECT count(*) FROM parent").unwrap().rows;
        assert_eq!(rows, vec![vec![ColumnValue::Integer(1)]]);
        let children =
            exec(&conn, "SELECT count(*) FROM child").unwrap().rows;
        assert_eq!(children, vec![vec![ColumnValue::Integer(0)]]);
    }
}
