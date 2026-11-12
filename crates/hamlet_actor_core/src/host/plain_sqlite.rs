//! One plain SQLite file per actor, served on a thread of its own.

use std::path::Path;
use std::sync::mpsc as std_mpsc;
use std::thread;

use anyhow::{Context, Result, anyhow};
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Batch, Connection};
use sqlite_types::{
    BindParam, ColumnValue, ExecuteResult, QueryResult,
};
use tokio::sync::oneshot;

use crate::sqlite::SqliteBatchStatement;

enum Command {
    Exec(String, oneshot::Sender<Result<QueryResult>>),
    Execute(
        String,
        Option<Vec<BindParam>>,
        oneshot::Sender<Result<ExecuteResult>>,
    ),
    Batch(
        Vec<SqliteBatchStatement>,
        oneshot::Sender<Result<Vec<ExecuteResult>>>,
    ),
}

#[derive(Clone)]
pub struct PlainSqlite {
    tx: std_mpsc::Sender<Command>,
}

impl PlainSqlite {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("open {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;",
        )?;
        Self::serve_connection(conn)
    }

    /// Serves an already open connection, such as an in-memory one in
    /// tests.
    pub fn serve_connection(conn: Connection) -> Result<Self> {
        let (tx, rx) = std_mpsc::channel::<Command>();
        thread::Builder::new()
            .name("plain-sqlite".into())
            .spawn(move || serve(conn, rx))?;
        Ok(Self { tx })
    }

    pub async fn exec(&self, sql: String) -> Result<QueryResult> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Exec(sql, reply))?;
        rx.await.map_err(|_| anyhow!("sqlite thread stopped"))?
    }

    pub async fn execute(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> Result<ExecuteResult> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Execute(sql, params, reply))?;
        rx.await.map_err(|_| anyhow!("sqlite thread stopped"))?
    }

    pub async fn execute_batch(
        &self,
        statements: Vec<SqliteBatchStatement>,
    ) -> Result<Vec<ExecuteResult>> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Batch(statements, reply))?;
        rx.await.map_err(|_| anyhow!("sqlite thread stopped"))?
    }

    fn send(&self, command: Command) -> Result<()> {
        self.tx
            .send(command)
            .map_err(|_| anyhow!("sqlite file is closed"))
    }
}

fn serve(conn: Connection, rx: std_mpsc::Receiver<Command>) {
    while let Ok(command) = rx.recv() {
        match command {
            Command::Exec(sql, reply) => {
                let _ = reply.send(exec(&conn, &sql));
            }
            Command::Execute(sql, params, reply) => {
                let _ = reply.send(execute(
                    &conn,
                    &sql,
                    params.as_deref(),
                ));
            }
            Command::Batch(statements, reply) => {
                let _ = reply.send(batch(&conn, statements));
            }
        }
    }
}

fn exec(conn: &Connection, sql: &str) -> Result<QueryResult> {
    let mut result = QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
    };
    let mut batch = Batch::new(conn, sql);
    while let Some(mut stmt) = batch.next()? {
        let columns: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|c| c.to_string())
            .collect();
        let width = columns.len();
        let mut rows = Vec::new();
        let mut cursor = stmt.raw_query();
        while let Some(row) = cursor.next()? {
            rows.push(read_row(row, width)?);
        }
        if width > 0 || !rows.is_empty() {
            result = QueryResult { columns, rows };
        }
    }
    Ok(result)
}

fn execute(
    conn: &Connection,
    sql: &str,
    params: Option<&[BindParam]>,
) -> Result<ExecuteResult> {
    let mut stmt = conn.prepare(sql)?;
    for (index, param) in
        params.unwrap_or_default().iter().enumerate()
    {
        stmt.raw_bind_parameter(index + 1, to_value(param))?;
    }
    let columns: Vec<String> =
        stmt.column_names().iter().map(|c| c.to_string()).collect();
    let width = columns.len();
    let readonly = stmt.readonly();
    let mut rows = Vec::new();
    let mut cursor = stmt.raw_query();
    while let Some(row) = cursor.next()? {
        rows.push(read_row(row, width)?);
    }
    drop(cursor);
    Ok(ExecuteResult {
        columns,
        rows,
        changes: if readonly { 0 } else { conn.changes() as i64 },
        last_insert_row_id: (!readonly)
            .then(|| conn.last_insert_rowid()),
    })
}

/// Runs the statements as one transaction.
fn batch(
    conn: &Connection,
    statements: Vec<SqliteBatchStatement>,
) -> Result<Vec<ExecuteResult>> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let mut results = Vec::with_capacity(statements.len());
    for statement in statements {
        match execute(
            conn,
            &statement.sql,
            statement.params.as_deref(),
        ) {
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

fn read_row(
    row: &rusqlite::Row<'_>,
    width: usize,
) -> Result<Vec<ColumnValue>> {
    (0..width)
        .map(|index| {
            Ok(match row.get_ref(index)? {
                ValueRef::Null => ColumnValue::Null,
                ValueRef::Integer(v) => ColumnValue::Integer(v),
                ValueRef::Real(v) => ColumnValue::Float(v),
                ValueRef::Text(v) => ColumnValue::Text(
                    String::from_utf8_lossy(v).into_owned(),
                ),
                ValueRef::Blob(v) => ColumnValue::Blob(v.to_vec()),
            })
        })
        .collect()
}

fn to_value(param: &BindParam) -> Value {
    match param {
        BindParam::Null => Value::Null,
        BindParam::Integer(v) => Value::Integer(*v),
        BindParam::Float(v) => Value::Real(*v),
        BindParam::Text(v) => Value::Text(v.clone()),
        BindParam::Blob(v) => Value::Blob(v.clone()),
    }
}
