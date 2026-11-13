//! One plain SQLite file per actor, served on a thread of its own.
//!
//! The file is in write-ahead mode with normal sync, and in SQLite's
//! exclusive locking mode, so no other connection can open it while
//! the host runs the actor.

use std::path::Path;
use std::sync::mpsc as std_mpsc;
use std::thread;

use anyhow::{Context, Result, anyhow, ensure};
use hamlet_actor_core::{
    BindParam, ExecuteResult, QueryResult, SqliteBatchStatement,
};
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};
use tokio::sync::oneshot;

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
    /// A write of the host's own, held while the actor has a
    /// transaction open so it never joins the actor's transaction.
    HostWrite(HostWrite),
    Close(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct PlainSqlite {
    tx: std_mpsc::Sender<Command>,
}

/// Opens the actor's file in exclusive locking mode and write-ahead
/// mode with normal sync. The exclusive lock is taken at the first
/// access, so the caller writes before it serves the file.
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("open {}", path.display()))?;
    // The locking mode comes first, so the write-ahead log keeps its
    // index in this process's memory instead of a shared file.
    conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
    let journal: String = conn.pragma_update_and_check(
        None,
        "journal_mode",
        "WAL",
        |row| row.get(0),
    )?;
    ensure!(
        journal.eq_ignore_ascii_case("wal"),
        "{} stays in journal mode {journal}",
        path.display()
    );
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

impl PlainSqlite {
    /// Serves `conn` on a thread of its own until [`Self::close`].
    pub fn serve(conn: Connection) -> Result<Self> {
        let (tx, rx) = std_mpsc::channel::<Command>();
        thread::Builder::new()
            .name("plain-sqlite".into())
            .spawn(move || serve(conn, &rx))?;
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

    /// Queues a write of the host's own, which runs once the actor
    /// has no transaction open. Writes run in the order they are
    /// queued, each after the fault point `point`. The receiver gets
    /// the write's result.
    pub fn host_write(
        &self,
        point: &'static str,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<HostWriteResult> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::HostWrite(HostWrite {
            point,
            sql: sql.to_owned(),
            params,
            reply,
        }))?;
        Ok(HostWriteResult(rx))
    }

    /// Closes the connection, which releases the file's lock, and
    /// returns once it is closed.
    pub async fn close(&self) {
        let (reply, rx) = oneshot::channel();
        if self.send(Command::Close(reply)).is_ok() {
            let _ = rx.await;
        }
    }

    fn send(&self, command: Command) -> Result<()> {
        self.tx
            .send(command)
            .map_err(|_| anyhow!("sqlite file is closed"))
    }
}

fn serve(conn: Connection, rx: &std_mpsc::Receiver<Command>) {
    let mut held = Vec::new();
    while let Ok(command) = rx.recv() {
        match command {
            Command::Exec(sql, reply) => {
                let _ = reply.send(hamlet_sqlite::exec(&conn, &sql));
            }
            Command::Execute(sql, params, reply) => {
                let _ = reply.send(hamlet_sqlite::execute(
                    &conn,
                    &sql,
                    params.as_deref(),
                ));
            }
            Command::Batch(statements, reply) => {
                let _ = reply.send(hamlet_sqlite::batch(
                    &conn,
                    statements.iter().map(|s| {
                        (s.sql.as_str(), s.params.as_deref())
                    }),
                ));
            }
            Command::HostWrite(write) => held.push(write),
            Command::Close(reply) => {
                run_held(&conn, &mut held);
                if let Err((_, error)) = conn.close() {
                    tracing::error!(
                        ?error,
                        "sqlite file did not close"
                    );
                }
                let _ = reply.send(());
                return;
            }
        }
        run_held(&conn, &mut held);
    }
}

struct HostWrite {
    point: &'static str,
    sql: String,
    params: Vec<Value>,
    reply: oneshot::Sender<Result<()>>,
}

/// The result of a queued host write.
pub struct HostWriteResult(oneshot::Receiver<Result<()>>);

impl HostWriteResult {
    pub async fn wait(self) -> Result<()> {
        self.0.await.map_err(|_| anyhow!("sqlite thread stopped"))?
    }
}

/// Runs the host's held writes when no transaction is open.
fn run_held(conn: &Connection, held: &mut Vec<HostWrite>) {
    if !conn.is_autocommit() {
        return;
    }
    for HostWrite {
        point,
        sql,
        params,
        reply,
    } in held.drain(..)
    {
        crate::fault::point(point);
        let result = conn
            .execute(&sql, params_from_iter(params))
            .map(drop)
            .map_err(Into::into);
        let _ = reply.send(result);
    }
}

#[cfg(test)]
mod tests {
    use hamlet_actor_core::ColumnValue;
    use hamlet_testing::Scratch;

    use super::*;

    #[tokio::test]
    async fn a_host_write_waits_out_an_open_transaction() {
        let dir = Scratch::new("plain-sqlite-held");
        let conn = open(&dir.join("a.sqlite")).unwrap();
        conn.execute_batch("CREATE TABLE t (who TEXT NOT NULL)")
            .unwrap();
        let file = PlainSqlite::serve(conn).unwrap();

        file.exec("BEGIN".to_owned()).await.unwrap();
        file.exec("INSERT INTO t VALUES ('actor')".to_owned())
            .await
            .unwrap();
        let written = file
            .host_write(
                "test-write",
                "INSERT INTO t VALUES (?)",
                vec![Value::Text("host".to_owned())],
            )
            .unwrap();
        file.exec("ROLLBACK".to_owned()).await.unwrap();
        written.wait().await.unwrap();

        let rows = file
            .exec("SELECT who FROM t".to_owned())
            .await
            .unwrap()
            .rows;
        assert_eq!(
            rows,
            vec![vec![ColumnValue::Text("host".into())]]
        );
        file.close().await;
    }
}
