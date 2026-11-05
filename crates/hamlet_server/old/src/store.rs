//! The realm database: what Hamlet owns for a home, with its schema.

#[cfg(test)]
mod tests;

use std::ops::{Deref, DerefMut};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use diesel::connection::{
    AnsiTransactionManager, Connection, SimpleConnection,
    TransactionManager,
};
use diesel::dsl::sql;
use diesel::sql_types::BigInt;
use diesel::{RunQueryDsl, SqliteConnection};

/// Stores a type as the text of its `as_str`, read back by its
/// `named`. The type derives `AsExpression` and `FromSqlRow` as
/// `Text`.
macro_rules! named_text {
    ($type:ty) => {
        impl
            ::diesel::serialize::ToSql<
                ::diesel::sql_types::Text,
                ::diesel::sqlite::Sqlite,
            > for $type
        {
            fn to_sql<'b>(
                &'b self,
                out: &mut ::diesel::serialize::Output<
                    'b,
                    '_,
                    ::diesel::sqlite::Sqlite,
                >,
            ) -> ::diesel::serialize::Result {
                out.set_value(self.as_str());
                Ok(::diesel::serialize::IsNull::No)
            }
        }

        impl
            ::diesel::deserialize::FromSql<
                ::diesel::sql_types::Text,
                ::diesel::sqlite::Sqlite,
            > for $type
        {
            fn from_sql(
                value: ::diesel::sqlite::SqliteValue<'_, '_, '_>,
            ) -> ::diesel::deserialize::Result<Self> {
                let name =
                    <String as ::diesel::deserialize::FromSql<
                        ::diesel::sql_types::Text,
                        ::diesel::sqlite::Sqlite,
                    >>::from_sql(value)?;
                Self::named(&name).ok_or_else(|| {
                    format!("not a {}: {name:?}", stringify!($type))
                        .into()
                })
            }
        }
    };
}
pub(crate) use named_text;

/// The schema's version. A store of any other version is refused.
const VERSION: i64 = 2;

/// The `application_id` that marks a realm store.
const APPLICATION: i64 = 1_212_239_180;

/// The schema, created in this order.
const SCHEMA: &[&str] = &[
    // Markers of operations' outcomes.
    "CREATE TABLE marker (
        runtime TEXT NOT NULL,
        key TEXT NOT NULL,
        kind TEXT NOT NULL,
        digest BLOB NOT NULL,
        outcome TEXT NOT NULL,
        PRIMARY KEY (runtime, key)
    ) STRICT, WITHOUT ROWID;",
    // Hamlet records, realm-wide living names and road charges.
    include_str!("record/schema.sql"),
    // The identity shared with the home's realm.id file.
    "CREATE TABLE store_identity (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        home TEXT NOT NULL
    ) STRICT;",
    // Chief documents, keyed by home or hamlet.
    "CREATE TABLE chief_document (
        kind TEXT NOT NULL,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        PRIMARY KEY (kind, key)
    ) STRICT, WITHOUT ROWID;",
    // Harness runtimes, bindings, inputs and model usage.
    include_str!("harness/schema.sql"),
    // Append-only budget resets.
    include_str!("spend.sql"),
    // Chats, kept transcripts, the gate and what waits for a chief.
    include_str!("chats/schema.sql"),
    // Society conversations, calls and documents.
    include_str!("society/schema.sql"),
];

/// A home's realm database, open in this process alone.
pub struct Store {
    db: SqliteConnection,
}

impl Store {
    /// Opens the database at `path`, creating it with the schema when
    /// missing.
    pub fn open(path: &Path) -> Result<Self> {
        let fresh = !path.try_exists()?;
        let url =
            path.to_str().context("the store's path is not UTF-8")?;
        let mut db = SqliteConnection::establish(url)
            .with_context(|| format!("opening {}", path.display()))?;
        db.batch_execute("PRAGMA busy_timeout = 5000;")?;
        if fresh {
            create(&mut db).with_context(|| {
                format!("creating {}", path.display())
            })?;
        } else {
            check(&mut db).with_context(|| {
                format!("opening {}", path.display())
            })?;
        }
        // A reply goes out after its commit, so the commit must be on
        // disk by then.
        db.batch_execute(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(Self { db })
    }

    /// The open database.
    pub fn connection(&mut self) -> &mut SqliteConnection {
        &mut self.db
    }

    /// The rows inserted, updated or deleted since the store opened,
    /// counting those of statements that rolled back. Only this
    /// connection writes the store, so it moves with every change.
    pub(crate) fn changes(&mut self) -> Result<u64> {
        let changes =
            diesel::select(sql::<BigInt>("total_changes()"))
                .get_result::<i64>(&mut self.db)?;
        Ok(u64::try_from(changes)?)
    }

    /// A transaction that takes the write lock at its first write.
    pub(crate) fn transaction(&mut self) -> Result<Tx<'_>> {
        Tx::begin(&mut self.db, "BEGIN")
    }

    /// A transaction that holds the write lock from its start.
    pub(crate) fn immediate(&mut self) -> Result<Tx<'_>> {
        Tx::begin(&mut self.db, "BEGIN IMMEDIATE")
    }

    /// Closes the database.
    pub fn close(self) -> Result<()> {
        drop(self.db);
        Ok(())
    }
}

/// A transaction on the store, rolled back when dropped uncommitted.
pub(crate) struct Tx<'a> {
    db: &'a mut SqliteConnection,
    open: bool,
}

impl<'a> Tx<'a> {
    fn begin(
        db: &'a mut SqliteConnection,
        sql: &str,
    ) -> Result<Self> {
        AnsiTransactionManager::begin_transaction_sql(db, sql)?;
        Ok(Self { db, open: true })
    }

    /// Commits the transaction.
    pub(crate) fn commit(mut self) -> Result<()> {
        self.open = false;
        AnsiTransactionManager::commit_transaction(self.db)?;
        Ok(())
    }
}

impl Deref for Tx<'_> {
    type Target = SqliteConnection;

    fn deref(&self) -> &SqliteConnection {
        self.db
    }
}

impl DerefMut for Tx<'_> {
    fn deref_mut(&mut self) -> &mut SqliteConnection {
        self.db
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if self.open {
            let _ =
                AnsiTransactionManager::rollback_transaction(self.db);
        }
    }
}

/// Creates the schema in `db`, all in one transaction.
fn create(db: &mut SqliteConnection) -> Result<()> {
    let mut tx = Tx::begin(db, "BEGIN IMMEDIATE")?;
    for part in SCHEMA {
        tx.batch_execute(part)?;
    }
    tx.batch_execute(&format!(
        "PRAGMA application_id = {APPLICATION};
         PRAGMA user_version = {VERSION};"
    ))?;
    tx.commit()
}

/// Refuses `db` unless it is a realm store of this schema's version.
fn check(db: &mut SqliteConnection) -> Result<()> {
    let mut pragma = |name: &str| {
        diesel::select(sql::<BigInt>(&format!(
            "(SELECT {name} FROM pragma_{name}())"
        )))
        .get_result::<i64>(db)
    };
    ensure!(
        pragma("application_id")? == APPLICATION,
        "the database is not a realm store"
    );
    let version = pragma("user_version")?;
    ensure!(
        version == VERSION,
        "the realm store's schema is version {version}; this server \
         reads only version {VERSION}"
    );
    Ok(())
}
