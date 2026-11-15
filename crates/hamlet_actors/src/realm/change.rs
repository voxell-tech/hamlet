//! The changes to the realm's file as values, and the one
//! transaction that makes them.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use hamlet_actor::{BindParam, ColumnValue, Ctx};
use hamlet_messages::workspace::AgentNumber;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::calls::Push;
use super::{
    Realm, budget, chiefs, laid, letters, marker, record, steps,
    tasks, tree, versions, waits, world,
};

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

#[derive(Debug, Default, PartialEq)]
pub(super) struct Affected {
    /// The agents whose place it moved.
    pub places: BTreeSet<AgentNumber>,
    /// The agents whose reach it moved.
    pub reaches: BTreeSet<AgentNumber>,
    pub begun: Vec<steps::StepId>,
    /// The parts of the world a client reads that it changed.
    pub seen: BTreeSet<world::Part>,
}

impl Affected {
    pub fn seen(
        parts: impl IntoIterator<Item = world::Part>,
    ) -> Self {
        Self {
            seen: parts.into_iter().collect(),
            ..Self::default()
        }
    }

    fn merge(&mut self, other: Affected) {
        self.places.extend(other.places);
        self.reaches.extend(other.reaches);
        self.begun.extend(other.begun);
        self.seen.extend(other.seen);
    }
}

/// The time of one transaction, in ms since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct At(pub i64);

impl At {
    fn now() -> Result<Self> {
        Ok(Self(i64::try_from(
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        )?))
    }
}

pub(super) struct Rows<'a> {
    source: Source<'a>,
    at: At,
}

enum Source<'a> {
    Transaction(&'a handle::Statements),
    Outside(&'a Ctx<Realm>),
}

impl<'a> Rows<'a> {
    pub fn outside(ctx: &'a Ctx<Realm>) -> Result<Self> {
        Ok(Self {
            source: Source::Outside(ctx),
            at: At::now()?,
        })
    }

    pub async fn select(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Vec<Vec<ColumnValue>>> {
        read_only(sql)?;
        Ok(match self.source {
            Source::Transaction(statements) => {
                statements.execute(sql, params).await?.rows
            }
            Source::Outside(ctx) => {
                ctx.sql().query(sql, Some(params)).await?.rows
            }
        })
    }

    pub async fn rows<T: Columns>(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Vec<T>> {
        self.select(sql, params)
            .await?
            .iter()
            .map(|row| T::of(row))
            .collect()
    }

    pub async fn row<T: Columns>(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Option<T>> {
        Ok(self.rows(sql, params).await?.into_iter().next())
    }

    pub async fn values<T: Column>(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Vec<T>> {
        let rows = self.rows::<(T,)>(sql, params).await?;
        Ok(rows.into_iter().map(|(value,)| value).collect())
    }

    pub async fn value<T: Column>(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<Option<T>> {
        Ok(self.row::<(T,)>(sql, params).await?.map(|(value,)| value))
    }

    pub async fn next_id(&self, table: &str) -> Result<u64> {
        let highest = self
            .value::<Option<u64>>(
                &format!("SELECT MAX(id) FROM {table}"),
                Vec::new(),
            )
            .await?
            .flatten();
        Ok(highest.unwrap_or(0) + 1)
    }

    pub fn at(&self) -> At {
        self.at
    }
}

pub(super) trait Column: Sized {
    fn of(value: &ColumnValue) -> Result<Self>;
}

impl Column for i64 {
    fn of(value: &ColumnValue) -> Result<Self> {
        match value {
            ColumnValue::Integer(n) => Ok(*n),
            other => bail!("an integer column holds {other:?}"),
        }
    }
}

impl Column for u64 {
    fn of(value: &ColumnValue) -> Result<Self> {
        Ok(Self::try_from(i64::of(value)?)?)
    }
}

impl Column for u32 {
    fn of(value: &ColumnValue) -> Result<Self> {
        Ok(Self::try_from(i64::of(value)?)?)
    }
}

impl Column for bool {
    fn of(value: &ColumnValue) -> Result<Self> {
        Ok(i64::of(value)? != 0)
    }
}

impl Column for Vec<u8> {
    fn of(value: &ColumnValue) -> Result<Self> {
        match value {
            ColumnValue::Blob(bytes) => Ok(bytes.clone()),
            other => bail!("a blob column holds {other:?}"),
        }
    }
}

impl Column for String {
    fn of(value: &ColumnValue) -> Result<Self> {
        match value {
            ColumnValue::Text(text) => Ok(text.clone()),
            other => bail!("a text column holds {other:?}"),
        }
    }
}

impl<T: Column> Column for Option<T> {
    fn of(value: &ColumnValue) -> Result<Self> {
        match value {
            ColumnValue::Null => Ok(None),
            value => T::of(value).map(Some),
        }
    }
}

pub(super) trait Columns: Sized {
    fn of(row: &[ColumnValue]) -> Result<Self>;
}

macro_rules! columns {
    ($(($($column:ident $value:ident),+)),+ $(,)?) => {$(
        impl<$($column: Column),+> Columns for ($($column,)+) {
            fn of(row: &[ColumnValue]) -> Result<Self> {
                match row {
                    [$($value),+] => Ok(($($column::of($value)?,)+)),
                    _ => bail!("a row of another form: {row:?}"),
                }
            }
        }
    )+};
}

columns!(
    (A a),
    (A a, B b),
    (A a, B b, C c),
    (A a, B b, C c, D d),
    (A a, B b, C c, D d, E e),
    (A a, B b, C c, D d, E e, F f),
);

/// An integer, such as an id, as the column that holds it.
pub(super) fn integer<N>(n: N) -> Result<BindParam>
where
    i64: TryFrom<N>,
    <i64 as TryFrom<N>>::Error:
        std::error::Error + Send + Sync + 'static,
{
    Ok(BindParam::Integer(i64::try_from(n)?))
}

pub(super) fn number(agent: AgentNumber) -> BindParam {
    BindParam::Integer(i64::from(agent.0))
}

pub(super) fn case_name(case: &impl Serialize) -> Result<String> {
    match serde_json::to_value(case)? {
        serde_json::Value::String(name) => Ok(name),
        other => bail!("{other} is no case's name"),
    }
}

pub(super) fn from_case_name<T: DeserializeOwned>(
    name: String,
) -> Result<T> {
    Ok(serde_json::from_value(serde_json::Value::String(name))?)
}

fn read_only(sql: &str) -> Result<()> {
    let head = sql.trim_start().get(..6).unwrap_or("");
    anyhow::ensure!(
        head.eq_ignore_ascii_case("SELECT"),
        "rows are only read: {sql}"
    );
    Ok(())
}

pub(super) struct Writer<'a> {
    statements: &'a handle::Statements,
    at: At,
}

impl Writer<'_> {
    pub async fn execute(
        &self,
        sql: &str,
        params: Vec<BindParam>,
    ) -> Result<u64> {
        Ok(u64::try_from(
            self.statements.execute(sql, params).await?.changes,
        )?)
    }

    pub fn rows(&self) -> Rows<'_> {
        Rows {
            source: Source::Transaction(self.statements),
            at: self.at,
        }
    }

    pub fn at(&self) -> At {
        self.at
    }
}

pub(super) struct Transaction {
    statements: handle::Statements,
    open: handle::Open,
    at: At,
    ctx: Ctx<Realm>,
}

mod handle {
    use anyhow::Result;
    use hamlet_actor::{BindParam, Ctx, SqliteTransaction};
    use hamlet_actor_core::ExecuteResult;

    use super::Realm;

    pub(super) struct Statements(SqliteTransaction);

    impl Statements {
        pub async fn execute(
            &self,
            sql: &str,
            params: Vec<BindParam>,
        ) -> Result<ExecuteResult> {
            self.0.execute(sql, Some(params)).await
        }
    }

    pub(super) struct Open {
        tx: Option<SqliteTransaction>,
        ctx: Ctx<Realm>,
    }

    impl Drop for Open {
        fn drop(&mut self) {
            let Some(tx) = self.tx.take() else { return };
            self.ctx.register_task(async move {
                if let Err(error) = tx.rollback().await {
                    tracing::warn!("a rollback failed: {error:#}");
                }
            });
        }
    }

    impl Open {
        pub async fn commit(
            mut self,
            _statements: Statements,
        ) -> Result<()> {
            // A commit that fails rolls back by itself.
            if let Some(tx) = self.tx.take() {
                tx.commit().await?;
            }
            Ok(())
        }
    }

    pub(super) async fn begin(
        ctx: &Ctx<Realm>,
    ) -> Result<(Statements, Open)> {
        let tx = ctx.sql().begin_transaction(None).await?;
        Ok((
            Statements(tx.clone()),
            Open {
                tx: Some(tx),
                ctx: ctx.clone(),
            },
        ))
    }
}

#[must_use]
pub(super) struct Abandoned(());

pub(super) struct Write<'a, D> {
    pub changes: Vec<Change>,
    pub entries: Vec<record::Entry>,
    /// The id the changes are recorded under, with what they made.
    pub marker: Option<(&'a marker::Key, &'a D)>,
}

impl Transaction {
    pub async fn begin(ctx: &Ctx<Realm>) -> Result<Self> {
        let (statements, open) = handle::begin(ctx).await?;
        Ok(Self {
            statements,
            open,
            at: At::now()?,
            ctx: ctx.clone(),
        })
    }

    pub fn rows(&self) -> Rows<'_> {
        Rows {
            source: Source::Transaction(&self.statements),
            at: self.at,
        }
    }

    pub fn abandon(self) -> Abandoned {
        drop(self);
        Abandoned(())
    }

    pub async fn commit<D: Serialize>(
        self,
        realm: &Arc<Realm>,
        write: Write<'_, D>,
    ) -> Result<Vec<Push>> {
        let writer = Writer {
            statements: &self.statements,
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
            versions::raise(&writer, &affected).await?;
            record::write(&writer, write.entries).await?;
            let revision = if affected.seen.is_empty() {
                None
            } else {
                Some(versions::advance_revision(&writer).await?)
            };
            let pushes = Push::of(&affected);
            if let Some((key, done)) = write.marker {
                marker::record(&writer, key, done, &pushes).await?;
            }
            anyhow::Ok((affected, revision, pushes))
        }
        .await;
        let (affected, revision, mut pushes) = written?;
        let reserved = realm.drives.reserve(&affected.begun, &self);
        let Self {
            statements,
            open,
            at: _,
            ctx,
        } = self;
        open.commit(statements).await?;
        reserved.start(realm, &ctx);
        pushes.retain(|push| !matches!(push, Push::Drive(_)));
        if let Some(revision) = revision {
            realm.world.refresh(&ctx, revision, &affected).await;
        }
        Ok(pushes)
    }
}

pub(super) async fn commit(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    changes: Vec<Change>,
) -> Result<Vec<Push>> {
    Transaction::begin(ctx)
        .await?
        .commit(
            realm,
            Write::<()> {
                changes,
                entries: Vec::new(),
                marker: None,
            },
        )
        .await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use hamlet_messages::sender::Client;
    use hamlet_messages::workspace::AgentNumber;
    use hamlet_protocol::client::RequestId;

    use super::super::testing::Started;
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_transaction_dropped_open_rolls_back_and_frees_the_file()
     {
        let started = Started::new().await;
        let row = steps::StepId {
            kind: steps::Kind::Hire,
            n: 96,
        };
        commit(
            &started.realm,
            &started.ctx,
            vec![
                steps::Change::Begin { row }.into(),
                steps::Change::Advance {
                    row,
                    from: Some("a"),
                    to: "b",
                }
                .into(),
            ],
        )
        .await
        .unwrap_err();
        let next = started
            .commit(versions::Change::WantReach(AgentNumber(5)));
        tokio::time::timeout(Duration::from_secs(5), next)
            .await
            .expect("the next transaction begins at once")
            .unwrap();
        assert!(
            steps::row(&started.rows(), row).await.unwrap().is_none()
        );
        started.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_first_run_leaves_its_drives_to_the_commit_and_a_repeat()
     {
        let started = Started::new().await;
        let row = steps::StepId {
            kind: steps::Kind::Hire,
            n: 97,
        };
        let key = marker::Key::of(
            &Client,
            marker::Id::Request(RequestId::new()),
            "begin",
            &row,
        )
        .unwrap();
        let after_answer = Transaction::begin(&started.ctx)
            .await
            .unwrap()
            .commit(
                &started.realm,
                Write {
                    changes: vec![
                        steps::Change::Begin { row }.into(),
                    ],
                    entries: Vec::new(),
                    marker: Some((&key, &())),
                },
            )
            .await
            .unwrap();

        let Some(marker::Found::Same {
            pushes: on_repeat, ..
        }) = marker::find::<()>(&started.rows(), &key).await.unwrap()
        else {
            panic!("the marker holds the first run");
        };
        assert_eq!(
            (after_answer, on_repeat),
            (Vec::new(), vec![Push::Drive(row)])
        );
        started.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_begin_waits_for_the_first_to_end() {
        // A second begin while the first is open is not refused: it
        // waits, and begins once the first ends. It runs on a task of
        // its own, so dropping it while it waits would leave a
        // transaction that no one holds.
        let started = Started::new().await;
        let first = Transaction::begin(&started.ctx).await.unwrap();
        let mut second = Box::pin(Transaction::begin(&started.ctx));

        assert!(
            tokio::time::timeout(
                Duration::from_millis(500),
                &mut second
            )
            .await
            .is_err(),
            "the second begin ended while the first was open"
        );
        drop(first);
        let second =
            tokio::time::timeout(Duration::from_secs(5), second)
                .await
                .expect("the second begins once the first ends");
        drop(second.unwrap());
        started.stop().await;
    }

    #[test]
    fn a_read_refuses_a_write_behind_a_with() {
        assert!(read_only("SELECT n FROM marker").is_ok());
        assert!(
            read_only("WITH x AS (SELECT 1) DELETE FROM marker")
                .is_err()
        );
    }
}
