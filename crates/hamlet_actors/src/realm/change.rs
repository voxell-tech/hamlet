//! The changes to the realm's file as values, and the one
//! transaction that makes them. Nothing else writes the file.

#![expect(
    dead_code,
    reason = "the scaffold: the units' bodies use these"
)]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use hamlet_actor::{BindParam, ColumnValue, Ctx, SqliteTransaction};
use hamlet_messages::workspace::AgentNumber;
use serde::Serialize;

use super::calls::Push;
use super::{
    Realm, budget, chiefs, laid, letters, marker, record, steps,
    tasks, tree, versions, waits, world,
};

/// One change, of the module that owns the table it is made in.
/// Each module's `Change` is its whole vocabulary for writing.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Chiefs(chiefs::Change),
    Tree(tree::Change),
    Tasks(tasks::Change),
    Letters(letters::Change),
    Budget(budget::Change),
    Waits(waits::Change),
    Versions(versions::Change),
    Steps(steps::Change),
    Laid(laid::Change),
}

macro_rules! change_from {
    ($($module:ident => $variant:ident),* $(,)?) => {$(
        impl From<$module::Change> for Change {
            fn from(change: $module::Change) -> Self {
                Self::$variant(change)
            }
        }
    )*};
}

change_from!(
    chiefs => Chiefs,
    tree => Tree,
    tasks => Tasks,
    letters => Letters,
    budget => Budget,
    waits => Waits,
    versions => Versions,
    steps => Steps,
    laid => Laid,
);

/// What a module's `apply` reports of the change it made.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Affected {
    /// The agents whose place it moved: their lead, their hires,
    /// their roads or their tools turned off.
    pub places: BTreeSet<AgentNumber>,
    /// The agents whose reach it moved.
    pub reaches: BTreeSet<AgentNumber>,
    /// The step rows it began.
    pub begun: Vec<steps::StepId>,
    /// The parts of the world a client reads that it changed. Any
    /// part raises `revision` in this transaction, and the copy in
    /// memory rebuilds those parts after the commit.
    pub seen: BTreeSet<world::Part>,
}

impl Affected {
    fn merge(&mut self, other: Affected) {
        self.places.extend(other.places);
        self.reaches.extend(other.reaches);
        self.begun.extend(other.begun);
        self.seen.extend(other.seen);
    }
}

/// The time of one transaction, in ms since the Unix epoch. Every
/// row a transaction writes carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct At(pub i64);

impl At {
    fn now() -> Result<Self> {
        Ok(Self(i64::try_from(
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        )?))
    }
}

/// Reads rows: inside a transaction, or as ordinary statements after
/// a commit.
pub(super) struct Rows<'a> {
    source: Source<'a>,
    at: At,
}

enum Source<'a> {
    Transaction(&'a SqliteTransaction),
    Statements(&'a Ctx<Realm>),
}

impl<'a> Rows<'a> {
    /// Rows read as ordinary statements, each of which waits for a
    /// transaction that is open.
    pub fn outside(ctx: &'a Ctx<Realm>) -> Result<Self> {
        Ok(Self {
            source: Source::Statements(ctx),
            at: At::now()?,
        })
    }

    /// Runs a query. Any statement but a `SELECT` (or a `WITH` that
    /// ends in one) fails, in every build: rows read here are never
    /// written, and `tests/realm/writers.rs` names the writer of each
    /// table.
    pub async fn select(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Vec<Vec<ColumnValue>>> {
        let head = sql
            .trim_start()
            .get(..6)
            .unwrap_or("")
            .to_ascii_uppercase();
        anyhow::ensure!(
            head == "SELECT" || head.starts_with("WITH"),
            "rows are only read: {sql}"
        );
        Ok(match self.source {
            Source::Transaction(tx) => {
                tx.execute(sql, Some(params)).await?.rows
            }
            Source::Statements(ctx) => {
                ctx.sql().query(sql, Some(params)).await?.rows
            }
        })
    }

    /// The time this read counts as now.
    pub fn at(&self) -> At {
        self.at
    }
}

/// Writes rows in a transaction. Only [`Transaction::commit`] makes
/// one, and only a module's `apply` is given it.
pub(super) struct Writer<'a> {
    tx: &'a SqliteTransaction,
    at: At,
}

impl Writer<'_> {
    /// Runs a statement; the number of rows it changed.
    pub async fn execute(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<u64> {
        Ok(u64::try_from(
            self.tx.execute(sql, Some(params)).await?.changes,
        )?)
    }

    pub fn rows(&self) -> Rows<'_> {
        Rows {
            source: Source::Transaction(self.tx),
            at: self.at,
        }
    }

    pub fn at(&self) -> At {
        self.at
    }
}

/// A transaction of the realm. It holds the file's gate from `begin`
/// to `commit` or `rollback`, so no call to another actor is made
/// while one is open. The host gives a transaction no `Drop`: each
/// path out of one rolls it back or commits it.
pub(super) struct Transaction {
    tx: SqliteTransaction,
    at: At,
}

/// What one transaction writes.
pub(super) struct Write<'a, D> {
    pub changes: Vec<Change>,
    pub entries: Vec<record::Entry>,
    /// The id the changes are recorded under, with what they made.
    pub marker: Option<(&'a marker::Key, &'a D)>,
}

impl Transaction {
    pub async fn begin(ctx: &Ctx<Realm>) -> Result<Self> {
        Ok(Self {
            tx: ctx.sql().begin_transaction(None).await?,
            at: At::now()?,
        })
    }

    pub fn rows(&self) -> Rows<'_> {
        Rows {
            source: Source::Transaction(&self.tx),
            at: self.at,
        }
    }

    pub async fn rollback(self) -> Result<()> {
        self.tx.rollback().await
    }

    /// Makes `write` and commits. Returns the pushes the changes set
    /// off, for the caller to make after it. An error before the
    /// commit rolls the transaction back.
    pub async fn commit<D: Serialize>(
        self,
        realm: &Arc<Realm>,
        ctx: &Ctx<Realm>,
        write: Write<'_, D>,
    ) -> Result<Vec<Push>> {
        let writer = Writer {
            tx: &self.tx,
            at: self.at,
        };
        let written = async {
            let mut affected = Affected::default();
            for change in write.changes {
                affected.merge(match change {
                    Change::Chiefs(change) => {
                        chiefs::apply(&writer, change).await?
                    }
                    Change::Tree(change) => {
                        tree::apply(&writer, change).await?
                    }
                    Change::Tasks(change) => {
                        tasks::apply(&writer, change).await?
                    }
                    Change::Letters(change) => {
                        letters::apply(&writer, change).await?
                    }
                    Change::Budget(change) => {
                        budget::apply(&writer, change).await?
                    }
                    Change::Waits(change) => {
                        waits::apply(&writer, change).await?
                    }
                    Change::Versions(change) => {
                        versions::apply(&writer, change).await?
                    }
                    Change::Steps(change) => {
                        steps::apply(&writer, change).await?
                    }
                    Change::Laid(change) => {
                        laid::apply(&writer, change).await?
                    }
                });
            }
            // Places and reaches.
            versions::raise(&writer, &affected).await?;
            record::write(&writer, write.entries).await?;
            let revision = if affected.seen.is_empty() {
                None
            } else {
                Some(versions::advance_revision(&writer).await?)
            };
            // Drives, reaches, places.
            let pushes = Push::of(&affected);
            if let Some((key, done)) = write.marker {
                marker::record(&writer, key, done, &pushes).await?;
            }
            anyhow::Ok((affected, revision, pushes))
        }
        .await;
        let (affected, revision, pushes) = match written {
            Ok(written) => written,
            Err(error) => {
                self.tx.rollback().await?;
                return Err(error);
            }
        };
        // Before the commit: a row that is begun is never seen
        // without its drive. On an error of the commit, `reserved`
        // drops.
        let reserved = realm.drives.reserve(&affected.begun);
        self.tx.commit().await?;
        reserved.start(realm, ctx);
        // After the commit, so it adds nothing to the commit's time.
        if let Some(revision) = revision {
            realm.world.refresh(ctx, revision, &affected).await;
        }
        Ok(pushes)
    }
}

/// Makes `changes` in a transaction of their own: for a step, for
/// the answer of a reach, and for the realm's first start.
pub(super) async fn commit(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    changes: Vec<Change>,
) -> Result<Vec<Push>> {
    Transaction::begin(ctx)
        .await?
        .commit(
            realm,
            ctx,
            Write::<()> {
                changes,
                entries: Vec::new(),
                marker: None,
            },
        )
        .await
}
