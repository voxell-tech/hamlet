//! The realm database: what Hamlet owns for a home, with its schema
//! migrations.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, TransactionBehavior};

/// The schema, one step per version. A step is never edited once
/// released; a change is a new step.
const MIGRATIONS: &[&str] = &[
    // 1. Markers of operations' outcomes.
    "CREATE TABLE marker (
        runtime TEXT NOT NULL,
        key TEXT NOT NULL,
        kind TEXT NOT NULL,
        digest BLOB NOT NULL,
        outcome TEXT NOT NULL,
        PRIMARY KEY (runtime, key)
    ) STRICT, WITHOUT ROWID;",
    // 2. Hamlet records, realm-wide living names and road charges.
    include_str!("record/schema.sql"),
    // 3. The identity shared with the home's realm.id file.
    "CREATE TABLE store_identity (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        home TEXT NOT NULL
    ) STRICT;
    PRAGMA application_id = 1212239180;",
    // 4. Chief documents, keyed by home or hamlet.
    "CREATE TABLE chief_document (
        kind TEXT NOT NULL,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        PRIMARY KEY (kind, key)
    ) STRICT, WITHOUT ROWID;",
    // 5. Harness runtimes, bindings, inputs, waits and model usage.
    include_str!("harness/schema.sql"),
    // 6. Task attribution, model time and append-only budget resets.
    include_str!("spend.sql"),
];

/// A home's realm database, open in this process alone.
pub struct Store {
    pub(crate) db: Connection,
}

impl Store {
    /// Opens the database at `path`, creating it when missing, and
    /// brings its schema up to this server's version.
    pub fn open(path: &Path) -> Result<Self> {
        let fresh = !path.try_exists()?;
        let mut db = Connection::open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        migrate(&mut db, fresh).with_context(|| {
            format!("migrating {}", path.display())
        })?;
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

    /// Closes the database and reports any closing error.
    pub fn close(self) -> Result<()> {
        self.db.close().map_err(|(_, error)| error.into())
    }
}

/// Applies the steps `db` lacks, all in one transaction.
fn migrate(db: &mut Connection, fresh: bool) -> Result<()> {
    let tx =
        db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version = tx.query_row("PRAGMA user_version", [], |row| {
        row.get::<_, i64>(0)
    })?;
    let known = i64::try_from(MIGRATIONS.len())?;
    ensure!(
        (0..=known).contains(&version),
        "the schema is version {version}, newer than this server's \
         {known}"
    );
    ensure!(
        fresh || version > 0,
        "the existing realm database has no schema; restore it from \
         a complete home backup"
    );
    let application =
        tx.query_row("PRAGMA application_id", [], |row| {
            row.get::<_, i64>(0)
        })?;
    ensure!(
        application == if version < 3 { 0 } else { 1_212_239_180 },
        "the database is not this server's realm store"
    );
    // Earlier schemas predate application_id. Check their tables
    // before upgrading them.
    if version >= 1 {
        tx.prepare(
            "SELECT runtime, key, kind, digest, outcome FROM marker \
             LIMIT 0",
        )?;
    }
    if version >= 2 {
        tx.prepare("SELECT id, document FROM record_hamlet LIMIT 0")?;
        tx.prepare(
            "SELECT name, hamlet, number FROM record_name LIMIT 0",
        )?;
        tx.prepare(
            "SELECT id, hamlet, agent, task, at, tokens, micros, ms \
             FROM record_road_charge LIMIT 0",
        )?;
    }
    for step in &MIGRATIONS[usize::try_from(version)?..] {
        tx.execute_batch(step)?;
    }
    tx.pragma_update(None, "user_version", known)?;
    tx.commit()?;
    Ok(())
}
