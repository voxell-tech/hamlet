//! The `step` table and its one walk: an operation that spans actors
//! is a row, driven from its last step done.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{BindParam, Ctx};
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::calls::{Calls, Delivery, Made};
use super::change::{self, Affected, Rows, Writer};
use super::peer::{self, Peers, When};
use super::workers::errands;
use super::{
    Realm, calls, dreams, drop_parts, hire, librarian, parcels,
    reviews, tree, versions, workers,
};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Hire,
    Worker,
    Errand,
    Review,
    Parcel,
    Dream,
    Verdict,
    DropParts,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(super) struct StepId {
    pub kind: Kind,
    pub n: u64,
}

/// One operation's steps, in the order they are done.
pub(super) trait Steps:
    Copy + Eq + Send + Sync + 'static
{
    const ALL: &'static [Self];

    fn name(self) -> &'static str;
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// A new row with no step done.
    Begin { row: StepId },
    /// `to` is done, when `from` was the last step done and the row
    /// is open.
    Advance {
        row: StepId,
        from: Option<&'static str>,
        to: &'static str,
    },
    /// The row is driven no further.
    Close { row: StepId },
}

/// An `Advance` that finds the row moved or closed changes nothing
/// and fails.
pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Begin { row } => begin(writer, row).await,
        Change::Advance { row, from, to } => {
            advance(writer, row, from, to).await
        }
        Change::Close { row } => close(writer, row).await,
    }
}

async fn begin(writer: &Writer<'_>, row: StepId) -> Result<Affected> {
    let mut params = key(row)?;
    params.push(BindParam::Integer(writer.at().0));
    writer
        .execute(
            "INSERT INTO step (kind, n, at) VALUES (?1, ?2, ?3)",
            params,
        )
        .await?;
    Ok(Affected {
        begun: vec![row],
        ..Affected::default()
    })
}

fn key(row: StepId) -> Result<Vec<BindParam>> {
    Ok(vec![
        BindParam::Text(change::case_name(&row.kind)?),
        change::integer(row.n)?,
    ])
}

async fn advance(
    writer: &Writer<'_>,
    row: StepId,
    from: Option<&'static str>,
    to: &'static str,
) -> Result<Affected> {
    let mut params = key(row)?;
    params.push(BindParam::Text(to.to_owned()));
    params.push(from.map_or(BindParam::Null, |from| {
        BindParam::Text(from.to_owned())
    }));
    let moved = writer
        .execute(
            "UPDATE step SET done = ?3 WHERE kind = ?1 AND n = ?2 \
             AND done IS ?4 AND closed = 0",
            params,
        )
        .await?;
    if moved == 0 {
        bail!("{row:?} moved or closed before `{to}`");
    }
    Ok(Affected::default())
}

async fn close(writer: &Writer<'_>, row: StepId) -> Result<Affected> {
    writer
        .execute(
            "UPDATE step SET closed = 1 WHERE kind = ?1 AND n = ?2",
            key(row)?,
        )
        .await?;
    Ok(Affected::default())
}

/// The query of the numbers of `kind`'s open rows that have not done
/// `step`.
pub(super) fn not_done<S: Steps>(
    kind: Kind,
    step: S,
) -> Result<String> {
    let before = S::ALL
        .iter()
        .take_while(|&&each| each != step)
        .map(|each| format!("'{}'", each.name()))
        .collect::<Vec<_>>();
    let done = if before.is_empty() {
        "done IS NULL".to_owned()
    } else {
        format!("(done IS NULL OR done IN ({}))", before.join(", "))
    };
    Ok(format!(
        "SELECT n FROM step WHERE kind = '{}' AND closed = 0 AND \
         {done}",
        change::case_name(&kind)?
    ))
}

pub(super) struct Row {
    pub done: Option<String>,
    pub closed: bool,
}

pub(super) async fn row(
    rows: &Rows<'_>,
    id: StepId,
) -> Result<Option<Row>> {
    Ok(rows
        .row::<(Option<String>, bool)>(
            "SELECT done, closed FROM step WHERE kind = ?1 AND n = \
             ?2",
            key(id)?,
        )
        .await?
        .map(|(done, closed)| Row { done, closed }))
}

pub(super) struct Cx<'a> {
    pub realm: &'a Arc<Realm>,
    pub ctx: &'a Ctx<Realm>,
}

impl Cx<'_> {
    pub fn rows(&self) -> Result<Rows<'_>> {
        Rows::outside(self.ctx)
    }

    pub fn peers(&self) -> &Peers {
        &self.realm.peers
    }

    /// Commits `changes` in `tx`, then makes `deliveries` and those
    /// the changes decided, each in its turn.
    pub async fn commit_delivering(
        &self,
        tx: change::Transaction,
        changes: Vec<change::Change>,
        deliveries: Calls,
    ) -> Result<Made> {
        let write = change::Write::<()> {
            changes,
            entries: Vec::new(),
            marker: None,
        };
        let (pushes, lined) =
            tx.commit_lined(self.realm, write, deliveries).await?;
        calls::push_all(self.realm, self.ctx, pushes).await;
        Ok(calls::deliver_all(
            self.realm,
            self.ctx,
            lined.after_answer,
            When::AfterAnswer,
        )
        .await)
    }

    pub async fn commit(
        &self,
        changes: Vec<change::Change>,
    ) -> Result<Vec<anyhow::Error>> {
        let pushes =
            change::commit(self.realm, self.ctx, changes).await?;
        Ok(calls::push_all(self.realm, self.ctx, pushes).await)
    }

    pub async fn want_reach(&self, agent: AgentNumber) -> Result<()> {
        let failed = self
            .commit(vec![versions::Change::WantReach(agent).into()])
            .await?;
        failed.into_iter().next().map_or(Ok(()), Err)
    }
}

/// The first wait before a step that failed in passing is run again,
/// doubled after each failure up to [`LAST_RETRY`].
pub(super) const FIRST_RETRY: Duration = Duration::from_secs(1);

const LAST_RETRY: Duration = Duration::from_secs(60);

/// Drives `row` until it is done. A step that fails in passing runs
/// again while the realm runs; one that fails for good ends its row
/// as a failure of its kind.
async fn drive(realm: Arc<Realm>, ctx: Ctx<Realm>, row: StepId) {
    let (realm, ctx) = (&realm, &ctx);
    let refused = retried(ctx, None, move || async move {
        let walked = walk_row(realm, ctx, row).await;
        if let Err(error) = &walked {
            tracing::warn!(?row, "a step failed: {error:#}");
        }
        walked
    })
    .await;
    if let Some(why) = refused
        && let Err(error) = end_failed(realm, ctx, row, why).await
    {
        tracing::warn!(?row, "ending a failed row: {error:#}");
    }
}

/// Runs `attempt`, after `wait` when one is given, and again after
/// each failure in passing, waiting [`FIRST_RETRY`] doubled after
/// each up to [`LAST_RETRY`], until it is done or the realm stops.
pub(super) async fn retried<F>(
    ctx: &Ctx<Realm>,
    mut wait: Option<Duration>,
    mut attempt: impl FnMut() -> F,
) -> Option<String>
where
    F: Future<Output = Result<()>>,
{
    let stopping = ctx.abort_signal();
    loop {
        if let Some(wait) = wait {
            tokio::select! {
                () = stopping.cancelled() => return None,
                () = tokio::time::sleep(wait) => {}
            }
        }
        let Err(error) = attempt().await else {
            return None;
        };
        if peer::refused(&error) {
            return Some(peer::words(&error));
        }
        wait =
            Some(wait.map_or(FIRST_RETRY, |wait| {
                (wait * 2).min(LAST_RETRY)
            }));
    }
}

/// Ends the row that failed for good, and closes what it ran as
/// failed.
async fn end_failed(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    row: StepId,
    why: String,
) -> Result<()> {
    let agent = u32::try_from(row.n).map(AgentNumber);
    let tx = change::Transaction::begin(ctx).await?;
    let rows = tx.rows();
    let (changes, calls) = match row.kind {
        Kind::Hire => tree::failed(&rows, agent?, why).await?,
        Kind::Worker => (
            workers::failed(&rows, agent?).await?,
            Calls::after_answer(Delivery::WorkerAnswer(agent?)),
        ),
        Kind::Errand => {
            (errands::failed(&rows, row.n).await?, Calls::none())
        }
        Kind::Review => (reviews::failed(row.n)?, Calls::none()),
        Kind::Parcel => parcels::failed(&rows, row.n).await?,
        Kind::Dream | Kind::Verdict | Kind::DropParts => {
            (vec![Change::Close { row }.into()], Calls::none())
        }
    };
    if changes.is_empty() {
        return Ok(());
    }
    Cx { realm, ctx }
        .commit_delivering(tx, changes, calls)
        .await?;
    Ok(())
}

async fn walk_row(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    row: StepId,
) -> Result<()> {
    let cx = Cx { realm, ctx };
    match row.kind {
        Kind::Hire => {
            walk(&cx, row, async move |cx: &Cx<'_>, step| {
                hire::run(cx, row.n, step).await
            })
            .await
        }
        Kind::Worker => {
            walk(&cx, row, async move |cx: &Cx<'_>, step| {
                workers::run(cx, row.n, step).await
            })
            .await
        }
        Kind::Errand => {
            walk(&cx, row, async move |cx: &Cx<'_>, step| {
                errands::run(cx, row.n, step).await
            })
            .await
        }
        Kind::Review => {
            walk(&cx, row, async move |cx: &Cx<'_>, step| {
                reviews::run(cx, row.n, step).await
            })
            .await
        }
        Kind::Parcel => {
            walk(&cx, row, async move |cx: &Cx<'_>, step| {
                parcels::run(cx, row.n, step).await
            })
            .await
        }
        Kind::Dream => {
            walk(&cx, row, async move |cx: &Cx<'_>, step| {
                dreams::run(cx, row.n, step).await
            })
            .await
        }
        Kind::Verdict => {
            walk(&cx, row, async move |cx: &Cx<'_>, step| {
                librarian::run(cx, row.n, step).await
            })
            .await
        }
        Kind::DropParts => {
            walk(&cx, row, async move |cx: &Cx<'_>, step| {
                drop_parts::run(cx, row.n, step).await
            })
            .await
        }
    }
}

/// Runs each step of `S` after the row's last one done.
async fn walk<S: Steps>(
    cx: &Cx<'_>,
    id: StepId,
    run: impl AsyncFn(&Cx<'_>, S) -> Result<Vec<change::Change>>,
) -> Result<()> {
    loop {
        let Some(row) = row(&cx.rows()?, id).await? else {
            return Ok(());
        };
        if row.closed {
            return Ok(());
        }
        let next = match row.done.as_deref() {
            None => 0,
            Some(done) => {
                S::ALL
                    .iter()
                    .position(|step| step.name() == done)
                    .with_context(|| {
                        format!("no step is named `{done}`")
                    })?
                    + 1
            }
        };
        let Some(&step) = S::ALL.get(next) else {
            return Ok(());
        };
        let mut changes = run(cx, step).await?;
        changes.push(
            Change::Advance {
                row: id,
                from: next
                    .checked_sub(1)
                    .and_then(|last| S::ALL.get(last))
                    .map(|last| last.name()),
                to: step.name(),
            }
            .into(),
        );
        if next + 1 == S::ALL.len() {
            changes.push(Change::Close { row: id }.into());
        }
        let failed = cx.commit(changes).await?;
        if !failed.is_empty() {
            bail!("{} of the commit's pushes failed", failed.len());
        }
    }
}

#[derive(Default)]
pub(super) struct Drives {
    running: Arc<Mutex<HashMap<StepId, watch::Receiver<()>>>>,
}

#[must_use]
pub(super) struct Reserved {
    rows: Vec<(StepId, watch::Sender<()>)>,
    running: Arc<Mutex<HashMap<StepId, watch::Receiver<()>>>>,
}

impl Reserved {
    /// Drives each row on a task of the realm's.
    pub fn start(mut self, realm: &Arc<Realm>, ctx: &Ctx<Realm>) {
        for (row, ended) in std::mem::take(&mut self.rows) {
            let running = self.running.clone();
            let realm = realm.clone();
            let driving = ctx.clone();
            ctx.register_task(async move {
                drive(realm, driving, row).await;
                running
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&row);
                drop(ended);
            });
        }
    }
}

impl Drop for Reserved {
    fn drop(&mut self) {
        let mut running = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for (row, _) in &self.rows {
            running.remove(row);
        }
    }
}

impl Drives {
    pub fn reserve(
        &self,
        rows: &[StepId],
        _open: &change::Transaction,
    ) -> Reserved {
        self.take(rows)
    }

    /// Takes the entry of each of `rows` that no drive holds.
    fn take(&self, rows: &[StepId]) -> Reserved {
        let mut running = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let rows = rows
            .iter()
            .filter_map(|&row| match running.entry(row) {
                Entry::Occupied(_) => None,
                Entry::Vacant(entry) => {
                    let (ended, waits) = watch::channel(());
                    entry.insert(waits);
                    Some((row, ended))
                }
            })
            .collect();
        Reserved {
            rows,
            running: self.running.clone(),
        }
    }

    pub fn start(
        &self,
        realm: &Arc<Realm>,
        ctx: &Ctx<Realm>,
        row: StepId,
    ) {
        self.take(&[row]).start(realm, ctx);
    }

    /// Waits until `row`'s drive ends, at most `within`, and at once
    /// when none runs.
    pub async fn finished(
        &self,
        row: StepId,
        within: Duration,
        _abandoned: change::Abandoned,
    ) {
        let Some(mut ended) = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&row)
            .cloned()
        else {
            return;
        };
        // The drive sends nothing: its sender's drop ends the wait.
        let _ = tokio::time::timeout(within, ended.changed()).await;
    }
}

pub(super) async fn undone(
    rows: &Rows<'_>,
    drives: &Drives,
) -> Result<Reserved> {
    let open = rows
        .rows::<(String, u64)>(
            "SELECT kind, n FROM step WHERE closed = 0",
            Vec::new(),
        )
        .await?
        .into_iter()
        .map(|(kind, n)| {
            Ok(StepId {
                kind: change::from_case_name(kind)?,
                n,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(drives.take(&open))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    use super::super::testing::Started;
    use super::*;

    const ROW: StepId = StepId {
        kind: Kind::Hire,
        n: 99,
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn a_step_moves_only_from_the_last_one_done_of_an_open_row()
    {
        let started = Started::new().await;
        let commit = async |made: Change| started.commit(made).await;
        commit(Change::Begin { row: ROW }).await.unwrap();
        commit(Change::Advance {
            row: ROW,
            from: None,
            to: "a",
        })
        .await
        .unwrap();
        commit(Change::Advance {
            row: ROW,
            from: None,
            to: "b",
        })
        .await
        .unwrap_err();
        commit(Change::Close { row: ROW }).await.unwrap();
        commit(Change::Advance {
            row: ROW,
            from: Some("a"),
            to: "b",
        })
        .await
        .unwrap_err();

        let read = row(&started.rows(), ROW).await.unwrap().unwrap();
        assert_eq!(
            (read.done.as_deref(), read.closed),
            (Some("a"), true)
        );
        started.stop().await;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Two {
        First,
        Second,
    }

    impl Steps for Two {
        const ALL: &'static [Self] = &[Self::First, Self::Second];

        fn name(self) -> &'static str {
            match self {
                Self::First => "first",
                Self::Second => "second",
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_walk_runs_each_step_left_once_in_order() {
        let started = Started::new().await;
        let closed = StepId { n: 98, ..ROW };
        for row in [ROW, closed] {
            started.commit(Change::Begin { row }).await.unwrap();
        }
        started.commit(Change::Close { row: closed }).await.unwrap();
        let cx = Cx {
            realm: &started.realm,
            ctx: &started.ctx,
        };
        let ran = RefCell::new(Vec::new());
        let run = async |_: &Cx<'_>, step: Two| {
            ran.borrow_mut().push(step);
            Ok(Vec::new())
        };
        for row in [ROW, ROW, closed] {
            walk(&cx, row, run).await.unwrap();
        }

        assert_eq!(*ran.borrow(), [Two::First, Two::Second]);
        let rows = started.rows();
        let read = row(&rows, ROW).await.unwrap().unwrap();
        assert_eq!(
            (read.done.as_deref(), read.closed),
            (Some("second"), true)
        );
        let at_start =
            undone(&rows, &Drives::default()).await.unwrap();
        assert!(
            at_start.rows.iter().all(|&(row, _)| row != ROW),
            "a start drives no finished row"
        );
        started.stop().await;
    }

    #[test]
    fn a_reservation_dropped_unstarted_gives_its_rows_back() {
        let drives = Drives::default();
        drop(drives.take(&[ROW]));
        assert_eq!(drives.take(&[ROW]).rows.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_wait_for_no_drive_ends_at_once() {
        let started = Started::new().await;
        let held =
            change::Transaction::begin(&started.ctx).await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            Drives::default().finished(
                ROW,
                Duration::from_secs(60),
                held.abandon(),
            ),
        )
        .await
        .expect("no drive runs, so the wait ends at once");
        started.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_wait_for_a_drive_ends_after_its_commit() {
        let started = Started::new().await;
        let drives = Drives::default();
        let held =
            change::Transaction::begin(&started.ctx).await.unwrap();
        let reserved = drives.take(&[ROW]);
        let committed = Cell::new(false);
        let drive = async {
            started.commit(Change::Close { row: ROW }).await.unwrap();
            committed.set(true);
            drop(reserved);
        };
        let wait = async {
            drives
                .finished(
                    ROW,
                    Duration::from_secs(60),
                    held.abandon(),
                )
                .await;
            committed.get()
        };
        let ((), ended_after_commit) =
            tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(drive, wait)
            })
            .await
            .expect("the wait ends with the drive");
        assert!(ended_after_commit);
        started.stop().await;
    }
}
