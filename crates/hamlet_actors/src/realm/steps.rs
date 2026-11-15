//! The `step` table and its one walk: an operation that spans actors
//! is a row, driven from its last step done.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{BindParam, Ctx};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::change::{self, Affected, Rows, Writer};
use super::peer::Peers;
use super::{Realm, calls, hire};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Hire,
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "U4 closes rows with it")
    )]
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
        BindParam::Integer(i64::try_from(row.n)?),
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

    #[expect(dead_code, reason = "U3 calls it")]
    pub fn peers(&self) -> &Peers {
        &self.realm.peers
    }

    /// Commits `changes` before the step's call, and makes the pushes
    /// they set off, waiting for each.
    pub async fn commit(
        &self,
        changes: Vec<change::Change>,
    ) -> Result<()> {
        for made in
            change::commit(self.realm, self.ctx, changes).await?
        {
            calls::push(self.realm, self.ctx, made).await?;
        }
        Ok(())
    }
}

async fn drive(realm: Arc<Realm>, ctx: Ctx<Realm>, row: StepId) {
    let cx = Cx {
        realm: &realm,
        ctx: &ctx,
    };
    let driven = match row.kind {
        Kind::Hire => {
            walk(&cx, row, async move |cx: &Cx<'_>, step| {
                hire::run(cx, row.n, step).await
            })
            .await
        }
    };
    if let Err(error) = driven {
        tracing::warn!(?row, "a step failed: {error:#}");
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
        cx.commit(changes).await?;
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
    pub async fn finished(&self, row: StepId, within: Duration) {
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
    use std::cell::RefCell;
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
        let read = row(&started.rows(), ROW).await.unwrap().unwrap();
        assert_eq!(read.done.as_deref(), Some("second"));
        started.stop().await;
    }

    #[test]
    fn a_reservation_dropped_unstarted_gives_its_rows_back() {
        let drives = Drives::default();
        drop(drives.take(&[ROW]));
        assert_eq!(drives.take(&[ROW]).rows.len(), 1);
    }

    #[tokio::test]
    async fn a_wait_for_a_drive_ends_with_it() {
        let drives = Drives::default();
        tokio::time::timeout(
            Duration::from_secs(1),
            drives.finished(ROW, Duration::from_secs(60)),
        )
        .await
        .expect("no drive runs, so the wait ends at once");

        let reserved = drives.take(&[ROW]);
        let waiting = drives.finished(ROW, Duration::from_secs(60));
        drop(reserved);
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("the wait ends with the drive");
    }
}
