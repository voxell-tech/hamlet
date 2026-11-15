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

use super::change::{self, Affected, Column as _, Rows, Writer};
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

/// The row's key as `?1` and `?2`.
fn key(row: StepId) -> Result<Vec<BindParam>> {
    Ok(vec![
        BindParam::Text(change::name(&row.kind)?),
        BindParam::Integer(i64::try_from(row.n)?),
    ])
}

async fn advance(
    writer: &Writer<'_>,
    row: StepId,
    from: Option<&'static str>,
    to: &'static str,
) -> Result<Affected> {
    todo!()
}

async fn close(writer: &Writer<'_>, row: StepId) -> Result<Affected> {
    todo!()
}

pub(super) struct Row {
    pub id: StepId,
    pub done: Option<String>,
    pub closed: bool,
}

pub(super) async fn row(
    rows: &Rows<'_>,
    id: StepId,
) -> Result<Option<Row>> {
    rows.select(
        "SELECT done, closed FROM step WHERE kind = ?1 AND n = ?2",
        key(id)?,
    )
    .await?
    .first()
    .map(|found| match found.as_slice() {
        [done, closed] => Ok(Row {
            id,
            done: Option::<String>::of(done)?,
            closed: i64::of(closed)? != 0,
        }),
        _ => bail!("a step row of another form: {found:?}"),
    })
    .transpose()
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

/// The entries of rows about to be committed.
#[must_use]
pub(super) struct Reserved {
    rows: Vec<(StepId, watch::Sender<()>)>,
    running: Arc<Mutex<HashMap<StepId, watch::Receiver<()>>>>,
}

impl Reserved {
    /// Drives each row on a task of the realm's.
    pub fn start(self, realm: &Arc<Realm>, ctx: &Ctx<Realm>) {
        for (row, ended) in self.rows {
            let running = self.running.clone();
            let realm = realm.clone();
            let driving = ctx.clone();
            ctx.register_task(async move {
                drive(realm, driving, row).await;
                running
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&row);
                // Wakes each waiter of `finished`.
                drop(ended);
            });
        }
    }
}

impl Drives {
    /// Takes the entry of each of `rows` that no drive holds.
    pub fn reserve(&self, rows: &[StepId]) -> Reserved {
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
        self.reserve(&[row]).start(realm, ctx);
    }

    /// Waits until `row`'s drive ends, at most `within`, and at once
    /// when none runs.
    pub async fn finished(&self, row: StepId, within: Duration) {
        todo!()
    }
}

/// Starts a drive of every open row with a step left.
pub(super) async fn drive_undone(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
) -> Result<()> {
    let open = Rows::outside(ctx)?
        .select(
            "SELECT kind, n FROM step WHERE closed = 0",
            Vec::new(),
        )
        .await?;
    for found in open {
        let [kind, n] = found.as_slice() else {
            bail!("a step row of another form: {found:?}");
        };
        realm.drives.start(
            realm,
            ctx,
            StepId {
                kind: change::named(String::of(kind)?)?,
                n: u64::of(n)?,
            },
        );
    }
    Ok(())
}
