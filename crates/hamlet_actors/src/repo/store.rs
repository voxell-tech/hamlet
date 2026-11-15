//! The store beside a repository, as a unit reaches it from its
//! blocking thread, and the `marker` table.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::time::Duration;

use anyhow::{Result, bail, ensure};
use hamlet_actor::{
    BindParam, ColumnValue, SqliteDb, SqliteTransaction,
};
use tokio::runtime::Handle;

/// The longest the transaction of one unit stays open. It begins at
/// the unit's first row, after the git work.
const WRITE_WITHIN: Duration = Duration::from_secs(60);

/// SQL as a unit runs it, on its blocking thread.
pub(crate) trait Sql {
    fn select(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Vec<Vec<ColumnValue>>>;

    /// The number of rows it changed.
    fn execute(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<u64>;
}

/// The actor's SQL for one unit: plain reads until the first write,
/// which begins the one transaction the unit commits. Units run one
/// at a time, so the reads before it see what the transaction will
/// commit over. Dropped open, as by a panic, it rolls back.
pub(crate) struct UnitSql {
    db: SqliteDb,
    runtime: Handle,
    tx: RefCell<Option<SqliteTransaction>>,
}

impl UnitSql {
    pub fn new(db: SqliteDb, runtime: Handle) -> Self {
        Self {
            db,
            runtime,
            tx: RefCell::new(None),
        }
    }

    pub fn commit(self) -> Result<()> {
        match self.tx.borrow_mut().take() {
            Some(tx) => self.runtime.block_on(tx.commit()),
            None => Ok(()),
        }
    }

    pub fn rollback(self) {
        if let Some(tx) = self.tx.borrow_mut().take()
            && let Err(error) = self.runtime.block_on(tx.rollback())
        {
            tracing::warn!("a rollback failed: {error:#}");
        }
    }
}

impl Drop for UnitSql {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.get_mut().take() {
            self.runtime.spawn(async move {
                if let Err(error) = tx.rollback().await {
                    tracing::warn!("a rollback failed: {error:#}");
                }
            });
        }
    }
}

impl Sql for UnitSql {
    fn select(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Vec<Vec<ColumnValue>>> {
        // Once the transaction holds the gate, a plain statement
        // would wait for it, so every statement goes through it.
        let rows = match &*self.tx.borrow() {
            Some(tx) => {
                self.runtime
                    .block_on(tx.execute(sql, Some(params)))?
                    .rows
            }
            None => {
                self.runtime
                    .block_on(self.db.query(sql, Some(params)))?
                    .rows
            }
        };
        Ok(rows)
    }

    fn execute(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<u64> {
        let mut tx = self.tx.borrow_mut();
        if tx.is_none() {
            *tx = Some(self.runtime.block_on(
                self.db.begin_transaction(Some(WRITE_WITHIN)),
            )?);
        }
        let tx = tx.as_ref().expect("begun above");
        let done =
            self.runtime.block_on(tx.execute(sql, Some(params)))?;
        Ok(u64::try_from(done.changes)?)
    }
}

/// A table of the store, named by a token only the module that owns
/// the table can make.
pub(crate) trait Table {
    const NAME: &'static str;
}

/// The store, for reads: what [`super::Admits::known`] is given.
pub(crate) struct Store<'a>(pub(super) &'a dyn Sql);

impl<'a> Store<'a> {
    /// The rows of a table, for the module that owns it.
    pub fn rows<T: Table>(&self, _owner: T) -> Rows<'a, T> {
        Rows::new(self.0)
    }
}

/// A table's rows, read.
pub(crate) struct Rows<'a, T> {
    sql: &'a dyn Sql,
    table: PhantomData<T>,
}

impl<'a, T: Table> Rows<'a, T> {
    pub(super) fn new(sql: &'a dyn Sql) -> Self {
        Self {
            sql,
            table: PhantomData,
        }
    }

    pub fn select(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Vec<Vec<ColumnValue>>> {
        ensure!(
            first_words(sql, 1) == ["SELECT"],
            "not a read: {sql}"
        );
        self.sql.select(sql, params)
    }
}

/// A table's rows, read and written.
pub(crate) struct Writes<'a, T> {
    sql: &'a dyn Sql,
    table: PhantomData<T>,
}

impl<'a, T: Table> Writes<'a, T> {
    pub(super) fn new(sql: &'a dyn Sql) -> Self {
        Self {
            sql,
            table: PhantomData,
        }
    }

    pub fn rows(&self) -> Rows<'a, T> {
        Rows::new(self.sql)
    }

    /// Runs a statement that writes this table, and no other.
    pub fn execute(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<u64> {
        let words = first_words(sql, 5);
        let words: Vec<&str> =
            words.iter().map(String::as_str).collect();
        let written = match words.as_slice() {
            ["INSERT", "INTO", table, ..]
            | ["INSERT", "OR", _, "INTO", table]
            | ["UPDATE", table, ..]
            | ["DELETE", "FROM", table, ..] => *table,
            _ => bail!("not a write of one table: {sql}"),
        };
        ensure!(
            written.eq_ignore_ascii_case(T::NAME),
            "the owner of `{}` wrote `{written}`: {sql}",
            T::NAME
        );
        self.sql.execute(sql, params)
    }
}

/// The first words of a statement, in upper case.
fn first_words(sql: &str, most: usize) -> Vec<String> {
    sql.split(|c: char| c.is_whitespace() || c == '(')
        .filter(|word| !word.is_empty())
        .take(most)
        .map(str::to_ascii_uppercase)
        .collect()
}

/// The `marker` table: the answer of every kept call that carries an
/// id, under its caller, its id and its part.
pub(crate) mod marker {
    use anyhow::Result;
    use hamlet_actor::{Action, BindParam, ColumnValue};
    use hamlet_host::Caller;
    use hamlet_messages::sender::id_reused;
    use hamlet_messages::workspace::CallId;
    use hamlet_messages::{Changes, Repeat};
    use sha2::{Digest, Sha256};

    use super::Sql;

    pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS marker (
        caller TEXT NOT NULL,
        id TEXT NOT NULL,
        part INTEGER NOT NULL,
        action TEXT NOT NULL,
        digest BLOB NOT NULL,
        answer BLOB NOT NULL,
        at INTEGER NOT NULL,
        PRIMARY KEY (caller, id, part)
    ) STRICT";

    pub struct Key {
        caller: String,
        id: String,
        part: u32,
        action: &'static str,
        /// The SHA-256 of the message's CBOR, as its Rust type.
        digest: [u8; 32],
    }

    impl Key {
        /// `None` for a message that carries no id.
        pub fn of<M: Changes>(
            caller: &Caller,
            message: &M,
        ) -> Result<Option<Self>> {
            let (id, part) = match message.repeat() {
                Repeat::Id(id) => (id, 0),
                Repeat::Part { id, part } => (id, part),
                Repeat::Finds => return Ok(None),
            };
            let mut bytes = Vec::new();
            ciborium::into_writer(message, &mut bytes)?;
            Ok(Some(Self {
                caller: caller.to_string(),
                id: id.to_owned(),
                part,
                action: M::NAME,
                digest: Sha256::digest(bytes).into(),
            }))
        }

        fn bound(&self) -> Vec<BindParam> {
            vec![
                BindParam::Text(self.caller.clone()),
                BindParam::Text(self.id.clone()),
                BindParam::Integer(i64::from(self.part)),
            ]
        }
    }

    /// The answer kept under `key`, or the error of an id that a kept
    /// call of another request holds.
    pub fn find<M: Action>(
        sql: &dyn Sql,
        key: &Key,
    ) -> Result<Option<M::Output>> {
        let rows = sql.select(
            "SELECT action, digest, answer FROM marker WHERE caller \
             = ?1 AND id = ?2 AND part = ?3",
            key.bound(),
        )?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        match row.as_slice() {
            [
                ColumnValue::Text(action),
                ColumnValue::Blob(digest),
                ColumnValue::Blob(answer),
            ] if action == key.action && digest[..] == key.digest => {
                Ok(Some(ciborium::from_reader(answer.as_slice())?))
            }
            _ => Err(id_reused(key.action, &CallId(key.id.clone()))),
        }
    }

    pub fn keep<O: serde::Serialize>(
        sql: &dyn Sql,
        key: &Key,
        answer: &O,
        at: i64,
    ) -> Result<()> {
        let mut kept = Vec::new();
        ciborium::into_writer(answer, &mut kept)?;
        let mut bound = key.bound();
        bound.extend([
            BindParam::Text(key.action.to_owned()),
            BindParam::Blob(key.digest.to_vec()),
            BindParam::Blob(kept),
            BindParam::Integer(at),
        ]);
        sql.execute(
            "INSERT INTO marker (caller, id, part, action, digest, \
             answer, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            bound,
        )?;
        Ok(())
    }
}
