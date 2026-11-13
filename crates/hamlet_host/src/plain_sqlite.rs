//! One plain SQLite file per actor, served on a thread of its own.
//!
//! The file is in write-ahead mode with normal sync, and in SQLite's
//! exclusive locking mode, so no other connection can open it while
//! the host runs the actor.

use std::path::Path;
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result, anyhow, ensure};
use hamlet_actor_core::{
    BindParam, ExecuteResult, QueryResult, SqliteBatchStatement,
};
use hamlet_sqlite::Request;
use rusqlite::{Connection, OpenFlags};
use tokio::sync::oneshot;

/// The last work on a file before it closes. What it returns answers
/// the caller once the file is closed, so the caller finds the file
/// unlocked.
type Last = Box<dyn FnOnce(&Connection) -> Answer + Send>;

type Answer = Box<dyn FnOnce() + Send>;

enum Command {
    Sql(Request),
    Close(Last),
}

#[derive(Clone)]
pub struct PlainSqlite {
    tx: mpsc::Sender<Command>,
}

/// Opens an actor's file, and makes it if `create` and it is
/// missing. The exclusive lock is taken at the first access, so the
/// caller writes before it serves the file.
pub fn open(path: &Path, create: bool) -> Result<Connection> {
    let mut flags = OpenFlags::default();
    if !create {
        flags.remove(OpenFlags::SQLITE_OPEN_CREATE);
    }
    let conn = Connection::open_with_flags(path, flags)
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
        let (tx, rx) = mpsc::channel::<Command>();
        thread::Builder::new()
            .name("plain-sqlite".into())
            .spawn(move || serve(conn, &rx))?;
        Ok(Self { tx })
    }

    pub async fn exec(&self, sql: String) -> Result<QueryResult> {
        self.request(|reply| Request::Exec { sql, reply }).await
    }

    pub async fn execute(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> Result<ExecuteResult> {
        self.request(|reply| Request::Execute { sql, params, reply })
            .await
    }

    pub async fn execute_batch(
        &self,
        statements: Vec<SqliteBatchStatement>,
    ) -> Result<Vec<ExecuteResult>> {
        let statements = statements
            .into_iter()
            .map(|statement| (statement.sql, statement.params))
            .collect();
        self.request(|reply| Request::Batch { statements, reply })
            .await
    }

    /// Closes the connection, which releases the file's lock, and
    /// returns once it is closed.
    pub async fn close(&self) {
        let _ = self.close_with(|_| Ok(())).await;
    }

    /// Runs `last` on the connection, then closes it and ends the
    /// file's thread, whatever `last` returned, and returns what
    /// `last` returned once the file is closed. It runs as soon as
    /// the statements queued before it have, without waiting for an
    /// open transaction: one still open belongs to a generation that
    /// has ended, so it is rolled back first. Nothing runs between
    /// that rollback, `last` and the close, and any statement sent
    /// after them fails with "sqlite file is closed".
    pub async fn close_with<T: Send + 'static>(
        &self,
        last: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Close(Box::new(move |conn| {
            let result = last(conn);
            Box::new(move || {
                let _ = reply.send(result);
            })
        })))?;
        received(rx).await
    }

    async fn request<T>(
        &self,
        request: impl FnOnce(oneshot::Sender<Result<T>>) -> Request,
    ) -> Result<T> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Sql(request(reply)))?;
        received(rx).await
    }

    fn send(&self, command: Command) -> Result<()> {
        self.tx
            .send(command)
            .map_err(|_| anyhow!("sqlite file is closed"))
    }
}

async fn received<T>(rx: oneshot::Receiver<Result<T>>) -> Result<T> {
    rx.await.map_err(|_| anyhow!("sqlite thread stopped"))?
}

fn serve(conn: Connection, rx: &mpsc::Receiver<Command>) {
    while let Ok(command) = rx.recv() {
        match command {
            Command::Sql(request) => request.answer(&conn),
            Command::Close(last) => {
                if !conn.is_autocommit()
                    && let Err(error) = conn.execute_batch("ROLLBACK")
                {
                    tracing::error!(
                        ?error,
                        "sqlite file did not roll back"
                    );
                }
                let answer = last(&conn);
                if let Err((_, error)) = conn.close() {
                    tracing::error!(
                        ?error,
                        "sqlite file did not close"
                    );
                }
                answer();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[tokio::test]
    async fn closing_rolls_back_an_open_transaction_first() {
        let dir = Scratch::new("plain-sqlite-close");
        let conn = open(&dir.join("a.sqlite"), true).unwrap();
        conn.execute_batch("CREATE TABLE t (who TEXT NOT NULL)")
            .unwrap();
        let file = PlainSqlite::serve(conn).unwrap();

        file.exec("BEGIN".to_owned()).await.unwrap();
        file.exec("INSERT INTO t VALUES ('actor')".to_owned())
            .await
            .unwrap();
        let rows = file
            .close_with(|conn| {
                conn.execute("INSERT INTO t VALUES ('host')", [])?;
                let rows: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM t WHERE who = 'actor'",
                    [],
                    |row| row.get(0),
                )?;
                Ok(rows)
            })
            .await
            .unwrap();
        assert_eq!(rows, 0);
        assert!(file.exec("SELECT 1".to_owned()).await.is_err());

        let conn = open(&dir.join("a.sqlite"), false).unwrap();
        let who: String = conn
            .query_row("SELECT who FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(who, "host");
    }

    /// The close checkpoints the write-ahead log, which takes a while
    /// for a large one. The file is unlocked once `close_with` has
    /// answered.
    #[tokio::test]
    async fn closing_answers_once_the_file_is_unlocked() {
        let dir = Scratch::new("plain-sqlite-unlocked");
        let path = dir.join("a.sqlite");
        let conn = open(&path, true).unwrap();
        conn.execute_batch(
            "PRAGMA wal_autocheckpoint = 0; CREATE TABLE ballast (b \
             BLOB NOT NULL); INSERT INTO ballast VALUES \
             (randomblob(32000000))",
        )
        .unwrap();
        let file = PlainSqlite::serve(conn).unwrap();
        file.close_with(|_| Ok(())).await.unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.busy_timeout(std::time::Duration::ZERO).unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM ballast", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 1);
    }
}
