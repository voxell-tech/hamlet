//! The realm database: what Hamlet owns for a home, with its schema.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, Transaction, TransactionBehavior};

/// The schema's version. A store of any other version is refused.
const VERSION: i64 = 1;

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
    db: Connection,
}

impl Store {
    /// Opens the database at `path`, creating it with the schema when
    /// missing.
    pub fn open(path: &Path) -> Result<Self> {
        let fresh = !path.try_exists()?;
        let mut db = Connection::open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        if fresh {
            create(&mut db).with_context(|| {
                format!("creating {}", path.display())
            })?;
        } else {
            check(&db).with_context(|| {
                format!("opening {}", path.display())
            })?;
        }
        // A reply goes out after its commit, so the commit must be on
        // disk by then.
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(Self { db })
    }

    /// The open database.
    pub fn connection(&self) -> &Connection {
        &self.db
    }

    /// A transaction that takes the write lock at its first write.
    pub(crate) fn transaction(&mut self) -> Result<Transaction<'_>> {
        Ok(self.db.transaction()?)
    }

    /// A transaction that holds the write lock from its start.
    pub(crate) fn immediate(&mut self) -> Result<Transaction<'_>> {
        Ok(self.db.transaction_with_behavior(
            TransactionBehavior::Immediate,
        )?)
    }

    /// Closes the database and reports any closing error.
    pub fn close(self) -> Result<()> {
        self.db.close().map_err(|(_, error)| error.into())
    }
}

/// Creates the schema in `db`, all in one transaction.
fn create(db: &mut Connection) -> Result<()> {
    let tx =
        db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for part in SCHEMA {
        tx.execute_batch(part)?;
    }
    tx.pragma_update(None, "application_id", APPLICATION)?;
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()?;
    Ok(())
}

/// Refuses `db` unless it is a realm store of this schema's version.
fn check(db: &Connection) -> Result<()> {
    let pragma = |name: &str| {
        db.query_row(&format!("PRAGMA {name}"), [], |row| {
            row.get::<_, i64>(0)
        })
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
