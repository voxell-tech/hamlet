use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::Ctx;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{Gate, SocietyTool};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::callers::{Member, Stood, Witness};
use super::calls::{self, Calls, Made, Push};
use super::change::{self, Change, Rows};
use super::oversight::reserved;
use super::peer::When;
use super::world::World;
use super::{Realm, marker, record, steps};

/// An action that may change the realm, made by a caller of kind
/// `By`.
pub(super) trait Act<By>:
    Send + Sync + Sized + 'static
{
    type Output: Send + 'static;
    /// The rows it decides over, with every new id it gives out.
    type Loaded: Send;
    /// The ids a first run made.
    type Done: Serialize + DeserializeOwned + Send + Sync;

    fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> impl Future<Output = Result<Self::Loaded>> + Send;

    fn decide(
        &self,
        by: &By,
        loaded: Self::Loaded,
    ) -> Decision<Self::Done, Self::Output>;

    /// The calls that carry this act's own content to another actor.
    fn deliveries(&self, _by: &By, _done: &Self::Done) -> Calls {
        Calls::none()
    }

    /// The answer of a first run and of a repeat.
    fn answer(&self, done: Self::Done, made: &Made) -> Self::Output;
}

/// A read of the copy in memory.
pub(super) trait Look<By>:
    Send + Sync + Sized + 'static
{
    type Output: Send + 'static;

    fn look(&self, by: &By, world: &World) -> Self::Output;
}

/// The rows and the outcome of an act whose unit is not built yet:
/// it has no value, so such an act never answers.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum Unbuilt {}

#[derive(Debug, PartialEq)]
pub(super) enum Decision<D, O> {
    /// A first run.
    Change(Decided<D>),
    /// The act's own row tells that it ran before.
    Again(D),
    /// It answers this and changes nothing.
    Answer(O),
    /// It is for a hire whose steps are not done.
    WaitFor { row: steps::StepId, refusal: O },
}

#[derive(Debug, PartialEq)]
pub(super) struct Decided<D> {
    pub done: D,
    pub changes: Vec<Change>,
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
    let stood = exec(
        realm,
        ctx,
        by,
        id,
        name,
        act,
        async |_, _, _| Ok(None::<Barred<Infallible>>),
        |wait| {
            bail!("wait {} keeps an act no chief reserves", wait.0)
        },
    );
    Ok(stood.await?.unwrap_or_else(|never| match never {}))
}

/// Runs a society tool: refused by the caller's standing, then kept
/// for a chief when the limits reserve it for one, then the act.
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
    exec(
        realm,
        ctx,
        by,
        Some(id),
        name,
        act,
        async move |by: &By, rows: &Rows<'_>, act: &A| {
            if let Some(stood) = by.standing(rows).await? {
                return Ok(Some(Barred::Stood(stood)));
            }
            let kept = reserved::reserve(rows, tool, by.agent(), act)
                .await?;
            Ok(kept.map(Barred::Reserved))
        },
        |wait| Ok(Stood::Gate(Gate::Reserved { wait })),
    )
    .await
}

/// What a caller meets before its act is loaded.
enum Barred<S> {
    /// It is refused.
    Stood(S),
    /// The act is kept for a chief by these changes.
    Reserved(Decided<WaitId>),
}

/// What a marker keeps of a first run.
#[derive(Serialize, Deserialize)]
enum Ran<D> {
    Act(D),
    /// The act waits for a chief.
    Reserved(WaitId),
}

#[expect(
    clippy::too_many_arguments,
    reason = "the two runs differ only in the bar"
)]
async fn exec<By, A, S>(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    by: By,
    id: Option<marker::Id>,
    name: &'static str,
    act: A,
    bar: impl AsyncFn(&By, &Rows<'_>, &A) -> Result<Option<Barred<S>>>,
    reserved: impl Fn(WaitId) -> Result<S>,
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
        #[cfg(feature = "faults")]
        assert!(
            std::env::var("HAMLET_REALM_PANIC").as_deref()
                != Ok(name),
            "HAMLET_REALM_PANIC names `{name}`"
        );
        let tx = change::Transaction::begin(ctx).await?;
        let read = async {
            if let Some(key) = &key
                && let Some(found) =
                    marker::find::<Ran<A::Done>>(&tx.rows(), key)
                        .await?
            {
                return match found {
                    marker::Found::Same { done, pushes } => {
                        Ok(Read::Repeat { done, pushes })
                    }
                    marker::Found::Other => Err(key.reused()),
                };
            }
            match bar(&by, &tx.rows(), &act).await? {
                Some(Barred::Stood(stood)) => {
                    return Ok(Read::Stood(stood));
                }
                Some(Barred::Reserved(kept)) => {
                    return Ok(Read::Reserved(kept));
                }
                None => {}
            }
            let loaded = act.load(&by, &tx.rows()).await?;
            anyhow::Ok(Read::Decided(act.decide(&by, loaded)))
        }
        .await;
        let decision = match read? {
            Read::Repeat {
                done: Ran::Act(done),
                pushes,
            } => break (done, pushes),
            Read::Repeat {
                done: Ran::Reserved(wait),
                pushes,
            } => {
                after_reserving(realm, ctx, pushes);
                return Ok(Err(reserved(wait)?));
            }
            Read::Stood(stood) => return Ok(Err(stood)),
            Read::Reserved(kept) => {
                let wait = kept.done;
                let ran = Ran::<()>::Reserved(wait);
                let pushes = tx
                    .commit(
                        realm,
                        change::Write {
                            changes: kept.changes,
                            entries: kept.entries,
                            marker: key
                                .as_ref()
                                .map(|key| (key, &ran)),
                        },
                    )
                    .await?;
                after_reserving(realm, ctx, pushes);
                return Ok(Err(reserved(wait)?));
            }
            Read::Decided(decision) => decision,
        };
        match decision {
            Decision::Answer(output) => return Ok(Ok(output)),
            Decision::Again(done) => break (done, Vec::new()),
            Decision::WaitFor { row, refusal } => {
                if waited {
                    return Ok(Ok(refusal));
                }
                realm
                    .drives
                    .finished(row, realm.hiring_wait, tx.abandon())
                    .await;
                waited = true;
            }
            Decision::Change(decided) => {
                let ran = Ran::Act(&decided.done);
                let pushes = tx
                    .commit(
                        realm,
                        change::Write {
                            changes: decided.changes,
                            entries: decided.entries,
                            marker: key
                                .as_ref()
                                .map(|key| (key, &ran)),
                        },
                    )
                    .await?;
                break (decided.done, pushes);
            }
        }
    };
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

fn after_reserving(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    pushes: Vec<Push>,
) {
    ctx.register_task(calls::after_answer(
        realm.clone(),
        ctx.clone(),
        Vec::new(),
        pushes,
    ));
}

enum Read<D, O, S> {
    Repeat { done: Ran<D>, pushes: Vec<Push> },
    Stood(S),
    Reserved(Decided<WaitId>),
    Decided(Decision<D, O>),
}

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
