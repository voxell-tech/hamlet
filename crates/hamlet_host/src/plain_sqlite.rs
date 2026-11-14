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

/// Work of the host's own that waits for no transaction to be open.
type Idle = Box<dyn FnOnce(&Connection) -> Result<()> + Send>;

enum Command {
    Sql(Request),
    WhenIdle(Idle),
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

    /// Runs `work` on the connection as soon as no transaction is
    /// open: at once when none is, else once the open one ends, or at
    /// the close, after the rollback. A statement sent after `work`
    /// runs after it, unless it belongs to a transaction open when
    /// `work` was sent. An error of `work` is logged. Fails once the
    /// file is closed.
    pub fn when_idle(
        &self,
        work: impl FnOnce(&Connection) -> Result<()> + Send + 'static,
    ) -> Result<()> {
        self.send(Command::WhenIdle(Box::new(work)))
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
    /// has ended, so it is rolled back first. Only the work that
    /// [`Self::when_idle`] holds runs between that rollback, `last`
    /// and the close, and any statement sent after them fails with
    /// "sqlite file is closed".
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
    // The work `when_idle` holds until no transaction is open, in the
    // order it came.
    let mut idle = Vec::new();
    while let Ok(command) = rx.recv() {
        match command {
            Command::Sql(request) => request.answer(&conn),
            Command::WhenIdle(work) => idle.push(work),
            Command::Close(last) => {
                if !conn.is_autocommit()
                    && let Err(error) = conn.execute_batch("ROLLBACK")
                {
                    tracing::error!(
                        ?error,
                        "sqlite file did not roll back"
                    );
                }
                run_idle(&conn, &mut idle);
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
        if conn.is_autocommit() {
            run_idle(&conn, &mut idle);
        }
    }
}

fn run_idle(conn: &Connection, idle: &mut Vec<Idle>) {
    for work in idle.drain(..) {
        if let Err(error) = work(conn) {
            tracing::error!(
                ?error,
                "the host's write to a file failed"
            );
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
                let rows = conn.query_row(
                    "SELECT COUNT(*) FROM t WHERE who = 'actor'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                Ok(rows)
            })
            .await
            .unwrap();
        assert_eq!(rows, 0);
        assert!(file.exec("SELECT 1".to_owned()).await.is_err());

        let conn = open(&dir.join("a.sqlite"), false).unwrap();
        let who = conn
            .query_row("SELECT who FROM t", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap();
        assert_eq!(who, "host");
    }

    /// How many rows of `t` hold `who`.
    async fn rows_of(file: &PlainSqlite, who: &str) -> i64 {
        let result = file
            .exec(format!(
                "SELECT COUNT(*) FROM t WHERE who = '{who}'"
            ))
            .await
            .unwrap();
        match result.rows[0][0] {
            hamlet_actor_core::ColumnValue::Integer(rows) => rows,
            ref other => panic!("a count of {other:?}"),
        }
    }

    /// Idle work waits for the open transaction to end, and runs
    /// before the statements sent after it ends. At the close, it
    /// runs after the rollback of a transaction left open.
    #[tokio::test]
    async fn idle_work_waits_for_the_open_transaction() {
        let dir = Scratch::new("plain-sqlite-idle");
        let path = dir.join("a.sqlite");
        let conn = open(&path, true).unwrap();
        conn.execute_batch("CREATE TABLE t (who TEXT NOT NULL)")
            .unwrap();
        let file = PlainSqlite::serve(conn).unwrap();
        let insert = |who: &'static str| {
            move |conn: &Connection| {
                conn.execute("INSERT INTO t VALUES (?)", [who])?;
                Ok(())
            }
        };

        file.exec("BEGIN".to_owned()).await.unwrap();
        file.when_idle(insert("host")).unwrap();
        file.exec("INSERT INTO t VALUES ('actor')".to_owned())
            .await
            .unwrap();
        assert_eq!(rows_of(&file, "host").await, 0);
        file.exec("ROLLBACK".to_owned()).await.unwrap();
        assert_eq!(rows_of(&file, "actor").await, 0);
        assert_eq!(rows_of(&file, "host").await, 1);

        file.exec("BEGIN".to_owned()).await.unwrap();
        file.exec("INSERT INTO t VALUES ('leaked')".to_owned())
            .await
            .unwrap();
        file.when_idle(insert("closing")).unwrap();
        file.close().await;
        assert!(file.when_idle(insert("closed")).is_err());

        let conn = open(&path, false).unwrap();
        let file = PlainSqlite::serve(conn).unwrap();
        assert_eq!(rows_of(&file, "leaked").await, 0);
        assert_eq!(rows_of(&file, "closing").await, 1);
        file.close().await;
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
        let rows = conn
            .query_row("SELECT COUNT(*) FROM ballast", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert_eq!(rows, 1);
    }
}
