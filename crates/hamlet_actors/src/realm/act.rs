//! How an action of the realm is written, and the one function that
//! runs every one of them.

#![expect(
    dead_code,
    reason = "the scaffold: the units' bodies use these"
)]

use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::realm::SocietyTool;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::callers::{Member, Stood, Witness};
use super::calls::{self, Calls, Made, Push};
use super::change::{self, Change, Rows};
use super::peer::When;
use super::world::World;
use super::{Realm, marker, record, steps};

/// An action that may change the realm, made by a caller of kind
/// `By`. Its author writes what it reads, what it decides and what it
/// sends; [`run`] owns the order: the id, the transaction, the
/// commit, the answer, the calls.
pub(super) trait Act<By>:
    Send + Sync + Sized + 'static
{
    /// The answer on the wire.
    type Output: Send + 'static;
    /// The rows it decides over, with every new id it gives out.
    /// It holds what `decide` reads and no more, so a commit stays
    /// short.
    type Loaded: Send;
    /// The ids a first run made. Kept under the call's id, so a
    /// repeat answers and delivers from them alone. The realm's own
    /// type, never a message (W-F5).
    type Done: Serialize + DeserializeOwned + Send + Sync;

    /// Reads what [`Act::decide`] needs, from the modules that own
    /// the rows. It is given `Rows`, which only reads, and nothing
    /// that reaches the host.
    fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> impl Future<Output = Result<Self::Loaded>> + Send;

    /// The whole decision, with no clock, no id of its own, no row
    /// read and no call: a function of values, tested on values.
    fn decide(
        &self,
        by: &By,
        loaded: Self::Loaded,
    ) -> Decision<Self::Done, Self::Output>;

    /// The calls that carry this act's own content to another actor,
    /// built from `done` alone, so a repeat makes them again.
    fn deliveries(&self, _by: &By, _done: &Self::Done) -> Calls {
        Calls::none()
    }

    /// The answer of a first run and of a repeat. `made` tells how
    /// the deliveries before the answer went; it is empty for an act
    /// with none.
    fn answer(&self, done: Self::Done, made: &Made) -> Self::Output;
}

/// A read of the copy in memory. It opens no transaction, so it
/// answers while one runs.
pub(super) trait Look<By>:
    Send + Sync + Sized + 'static
{
    type Output: Send + 'static;

    fn look(&self, by: &By, world: &World) -> Self::Output;
}

/// The outcome of [`Act::decide`].
#[derive(Debug, PartialEq)]
pub(super) enum Decision<D, O> {
    /// A first run: these changes are committed, then it answers and
    /// delivers from `done`.
    Change(Decided<D>),
    /// The act's own row tells that it ran before: it answers and
    /// delivers from `done` again and changes nothing.
    Again(D),
    /// It answers this and changes nothing: a refusal, or the answer
    /// of a read.
    Answer(O),
    /// It is for a hire whose steps are not done. The runner waits
    /// for the row's drive with no transaction open, up to the
    /// realm's `hiring_wait`, and decides again, once, however the
    /// wait ended. A second `WaitFor` is answered with `refusal`.
    WaitFor { row: steps::StepId, refusal: O },
}

/// The changes of a first run, as values.
#[derive(Debug, PartialEq)]
pub(super) struct Decided<D> {
    pub done: D,
    pub changes: Vec<Change>,
    /// The entries it adds to the record (H17).
    pub entries: Vec<record::Entry>,
}

impl<D> Decided<D> {
    pub fn new(done: D) -> Self {
        Self {
            done,
            changes: Vec::new(),
            entries: Vec::new(),
        }
    }

    #[must_use]
    pub fn change(mut self, change: impl Into<Change>) -> Self {
        self.changes.push(change.into());
        self
    }

    #[must_use]
    pub fn record(mut self, entry: record::Entry) -> Self {
        self.entries.push(entry);
        self
    }
}

/// Runs `act` for `by`. `id` is the id a repeat carries, `None` for a
/// bare call.
pub(super) async fn run<By, A>(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    by: By,
    id: Option<marker::Id>,
    name: &'static str,
    act: A,
) -> Result<A::Output>
where
    By: Witness,
    A: Act<By> + Serialize,
{
    let stood = exec(realm, ctx, by, id, name, act, async |_, _| {
        Ok(None::<Infallible>)
    });
    Ok(stood.await?.unwrap_or_else(|never| match never {}))
}

/// Runs a society tool's `act`: as [`run`], and the caller's standing
/// (ended, or the tool turned off by its lead) is read in the same
/// transaction after the id, before the act loads.
pub(super) async fn run_tool<By, A>(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    by: By,
    id: marker::Id,
    name: &'static str,
    tool: SocietyTool,
    act: A,
) -> Result<Result<A::Output, Stood>>
where
    By: Member,
    A: Act<By> + Serialize,
{
    // `move`: the closure holds `tool`, a `Copy`; a borrow does not
    // live long enough.
    exec(
        realm,
        ctx,
        by,
        Some(id),
        name,
        act,
        async move |by, rows| by.standing(rows, tool).await,
    )
    .await
}

/// The one sequence of every action. `stand` reads what refuses the
/// caller itself, after the id and before the act.
async fn exec<By, A, S>(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    by: By,
    id: Option<marker::Id>,
    name: &'static str,
    act: A,
    stand: impl AsyncFn(&By, &Rows<'_>) -> Result<Option<S>>,
) -> Result<Result<A::Output, S>>
where
    By: Witness,
    A: Act<By> + Serialize,
{
    let key = id
        .map(|id| marker::Key::of(&by, id, name, &act))
        .transpose()?;
    let mut waited = false;
    let (done, pushes) = loop {
        let tx = change::Transaction::begin(ctx).await?;
        // Every read under the open transaction. An error rolls it
        // back before it returns: the host has no `Drop` for a
        // transaction, so one left open stops every other action of
        // the realm until the framework's 60 s timeout.
        let read = async {
            if let Some(key) = &key
                && let Some(found) =
                    marker::find::<A::Done>(&tx.rows(), key).await?
            {
                return match found {
                    marker::Found::Same { done, pushes } => {
                        Ok(Read::Repeat { done, pushes })
                    }
                    marker::Found::Other => Err(key.reused()),
                };
            }
            if let Some(stood) = stand(&by, &tx.rows()).await? {
                return Ok(Read::Stood(stood));
            }
            let loaded = act.load(&by, &tx.rows()).await?;
            anyhow::Ok(Read::Decided(act.decide(&by, loaded)))
        }
        .await;
        let decision = match read {
            Err(error) => {
                tx.rollback().await?;
                return Err(error);
            }
            Ok(Read::Repeat { done, pushes }) => {
                tx.rollback().await?;
                break (done, pushes);
            }
            Ok(Read::Stood(stood)) => {
                tx.rollback().await?;
                return Ok(Err(stood));
            }
            Ok(Read::Decided(decision)) => decision,
        };
        match decision {
            Decision::Answer(output) => {
                tx.rollback().await?; // nothing is kept
                return Ok(Ok(output));
            }
            Decision::Again(done) => {
                tx.rollback().await?;
                break (done, Vec::new());
            }
            Decision::WaitFor { row, refusal } => {
                tx.rollback().await?; // the wait holds nothing
                if waited {
                    return Ok(Ok(refusal));
                }
                // However the wait ends (the drive ended, the bound
                // passed, or no drive ran), it decides again once: a
                // drive that went live just before the wait began is
                // seen then.
                realm.drives.finished(row, realm.hiring_wait).await;
                waited = true;
            }
            Decision::Change(decided) => {
                // `commit` rolls back its own errors.
                let pushes = tx
                    .commit(
                        realm,
                        ctx,
                        change::Write {
                            changes: decided.changes,
                            entries: decided.entries,
                            marker: key
                                .as_ref()
                                .map(|key| (key, &decided.done)),
                        },
                    )
                    .await?;
                break (decided.done, pushes);
            }
        }
    };
    // Past here the first run and the repeat are one path, and no
    // transaction is open.
    let calls = act.deliveries(&by, &done);
    let made = calls::deliver_all(
        realm,
        ctx,
        calls.before_answer,
        When::BeforeAnswer,
    )
    .await;
    let answer = act.answer(done, &made);
    ctx.register_task(calls::after_answer(
        realm.clone(),
        ctx.clone(),
        calls.after_answer,
        pushes,
    ));
    Ok(Ok(answer))
}

/// What one pass of [`exec`] read under its transaction.
enum Read<D, O, S> {
    /// The call's id was given before to this request.
    Repeat { done: D, pushes: Vec<Push> },
    /// The caller itself is refused.
    Stood(S),
    /// The act decided.
    Decided(Decision<D, O>),
}

/// Answers `look` for `by` from the copy in memory.
pub(super) fn look<By, L>(
    realm: &Realm,
    by: &By,
    look: &L,
) -> L::Output
where
    By: Witness,
    L: Look<By>,
{
    look.look(by, &realm.world.current())
}
