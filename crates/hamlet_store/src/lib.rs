//! The realm's database: its schema, opening it, transactions, the
//! markers of requests done once, and the jobs kept with changes.

pub mod functions;
pub mod ids;
pub mod job;
pub mod marker;
pub mod schema;
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
use diesel::prelude::*;
use diesel::sql_types::BigInt;
use hamlet_protocol::Version;

/// The connection every query of the realm goes through.
pub type Db = SqliteConnection;

/// The schema's version, in semver. A database of a version it is not
/// compatible with is refused.
const VERSION: Version = Version::new(0, 1, 0);

/// The `application_id` that marks a realm's database.
const APPLICATION: i64 = 1_212_239_181;

const SCHEMA: &str = include_str!("schema.sql");

/// Stores a type as the text of its `as_str`, read back by its
/// `named`. The type derives `AsExpression` and `FromSqlRow` as
/// `Text`.
#[macro_export]
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

/// A realm's database, open in this process alone.
pub struct Store {
    db: Db,
}

impl Store {
    /// Makes the database of a new realm at `path`, which must not
    /// exist, holding the realm's identity `realm`.
    pub fn create(path: &Path, realm: &str, at: i64) -> Result<Self> {
        ensure!(
            !path.try_exists()?,
            "{} already exists",
            path.display()
        );
        let mut db = connect(path)?;
        let mut tx = Tx::begin(&mut db, "BEGIN IMMEDIATE")?;
        tx.batch_execute(SCHEMA)?;
        diesel::insert_into(schema::realm::table)
            .values((
                schema::realm::id.eq(1),
                schema::realm::identity.eq(realm),
                schema::realm::version.eq(VERSION.to_string()),
                schema::realm::made.eq(at),
            ))
            .execute(&mut *tx)?;
        tx.batch_execute(&format!(
            "PRAGMA application_id = {APPLICATION};"
        ))?;
        tx.commit().with_context(|| {
            format!("creating {}", path.display())
        })?;
        Self::ready(db)
    }

    /// Opens the database at `path`, refusing one that is missing or
    /// not a realm's of this schema's version.
    pub fn open(path: &Path) -> Result<Self> {
        ensure!(
            path.try_exists()?,
            "the realm's database {} is missing",
            path.display()
        );
        let mut db = connect(path)?;
        check(&mut db)
            .with_context(|| format!("opening {}", path.display()))?;
        Self::ready(db)
    }

    fn ready(mut db: Db) -> Result<Self> {
        // A reply goes out after its commit, so the commit must be on
        // disk by then.
        db.batch_execute(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(Self { db })
    }

    /// The realm's identity, as its folder's realm.id holds it too.
    pub fn realm(&mut self) -> Result<String> {
        Ok(schema::realm::table
            .select(schema::realm::identity)
            .get_result(&mut self.db)?)
    }

    /// The open database, for reads outside a transaction.
    pub fn connection(&mut self) -> &mut Db {
        &mut self.db
    }

    /// A transaction that takes the write lock at its first write.
    pub fn transaction(&mut self) -> Result<Tx<'_>> {
        Tx::begin(&mut self.db, "BEGIN")
    }

    /// A transaction that holds the write lock from its start.
    pub fn immediate(&mut self) -> Result<Tx<'_>> {
        Tx::begin(&mut self.db, "BEGIN IMMEDIATE")
    }
}

/// A transaction on the store, rolled back when dropped uncommitted.
pub struct Tx<'a> {
    db: &'a mut Db,
    open: bool,
}

impl<'a> Tx<'a> {
    fn begin(db: &'a mut Db, sql: &str) -> Result<Self> {
        AnsiTransactionManager::begin_transaction_sql(db, sql)?;
        Ok(Self { db, open: true })
    }

    pub fn commit(mut self) -> Result<()> {
        self.open = false;
        AnsiTransactionManager::commit_transaction(self.db)?;
        Ok(())
    }
}

impl Deref for Tx<'_> {
    type Target = Db;

    fn deref(&self) -> &Db {
        self.db
    }
}

impl DerefMut for Tx<'_> {
    fn deref_mut(&mut self) -> &mut Db {
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

fn connect(path: &Path) -> Result<Db> {
    let url =
        path.to_str().context("the database's path is not UTF-8")?;
    let mut db = Db::establish(url)
        .with_context(|| format!("opening {}", path.display()))?;
    db.batch_execute("PRAGMA busy_timeout = 5000;")?;
    Ok(db)
}

/// Refuses `db` unless it is a realm's database of this schema's
/// version.
fn check(db: &mut Db) -> Result<()> {
    let mut pragma = |name: &str| {
        diesel::select(sql::<BigInt>(&format!(
            "(SELECT {name} FROM pragma_{name}())"
        )))
        .get_result::<i64>(db)
    };
    ensure!(
        pragma("application_id")? == APPLICATION,
        "the database is not a realm's"
    );
    let kept = schema::realm::table
        .select(schema::realm::version)
        .get_result::<String>(db)?;
    let version = kept.parse::<Version>()?;
    ensure!(
        VERSION.is_compatible(&version),
        "the realm's database is version {version}; this server \
         reads only versions compatible with {VERSION}"
    );
    Ok(())
}
