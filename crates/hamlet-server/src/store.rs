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
];

/// A home's realm database, open in this process alone.
pub struct Store {
    pub(crate) db: Connection,
}

impl Store {
    /// Opens the database at `path`, creating it when missing, and
    /// brings its schema up to this server's version.
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        // A reply goes out after its commit, so the commit must be on
        // disk by then.
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;",
        )?;
        migrate(&mut db).with_context(|| {
            format!("migrating {}", path.display())
        })?;
        Ok(Self { db })
    }

    /// The open database.
    pub fn connection(&self) -> &Connection {
        &self.db
    }

    pub fn close(self) -> Result<()> {
        self.db.close().map_err(|(_, error)| error.into())
    }
}

/// Applies the steps `db` lacks, all in one transaction.
fn migrate(db: &mut Connection) -> Result<()> {
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
    for step in &MIGRATIONS[usize::try_from(version)?..] {
        tx.execute_batch(step)?;
    }
    tx.pragma_update(None, "user_version", known)?;
    tx.commit()?;
    Ok(())
}
