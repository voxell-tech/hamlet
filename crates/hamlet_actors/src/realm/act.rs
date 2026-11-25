use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, bail};
use hamlet_actor::Ctx;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{Gate, SocietyTool, Tool, ToolCall};
use hamlet_messages::workspace::{CallId, ConversationId, RealmTask};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::callers::{self, Stood, Witness};
use super::calls::{self, Calls, Made, Push};
use super::change::{self, Change, Rows};
use super::lanes::Lined;
use super::oversight::reserved::{self, Kept};
use super::peer::When;
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

    /// The step rows whose ends its answer waits for, all within
    /// the realm's `answer_within`, so the caller reads what the
    /// steps did once answered. Such an act lines no delivery before
    /// its answer.
    fn answers_after(
        &self,
        _done: &Self::Done,
    ) -> Vec<steps::StepId> {
        Vec::new()
    }

    /// The answer of a first run and of a repeat.
    fn answer(&self, done: Self::Done, made: &Made) -> Self::Output;

    /// The answer once a row of `answers_after` failed for good.
    fn failed(&self, done: Self::Done, made: &Made) -> Self::Output {
        self.answer(done, made)
    }
}

/// A read of a copy the realm holds in memory.
pub(super) trait Look<By>:
    Send + Sync + Sized + 'static
{
    type Output: Send + 'static;

    fn look(&self, by: &By, realm: &Realm) -> Self::Output;
}

/// The outcome of an act that only reads: there is none, so it
/// answers from `decide`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum Unchanged {}

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
    pub fn changes(
        mut self,
        changes: impl IntoIterator<Item = Change>,
    ) -> Self {
        self.changes.extend(changes);
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
    let key = id
        .map(|id| marker::Key::of(&by, id, name, &act))
        .transpose()?;
    run_keyed(realm, ctx, by, key, name, act).await
}

/// Runs `act` recorded under `key`, whose digest covers the whole
/// call that carried it.
pub(super) async fn run_keyed<By, A>(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    by: By,
    key: Option<marker::Key>,
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
        key,
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
pub(super) async fn run_tool<T: ToolAct<Agent>>(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    by: Agent,
    name: &'static str,
    tool: Tool<T>,
) -> Result<Result<<T::Run as Act<Agent>>::Output, Stood>> {
    let (key, called, act) = keyed(&by, name, tool)?;
    run_called(realm, ctx, by, (key, called), name, T::TOOL, act)
        .await
}

/// Runs `act`, which a call of the society tool `tool` carried under
/// `key`, as [`run_tool`] does.
pub(super) async fn run_called<A: Act<Agent> + Serialize>(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    by: Agent,
    (key, called): (marker::Key, Called),
    name: &'static str,
    tool: SocietyTool,
    act: A,
) -> Result<Result<A::Output, Stood>> {
    exec(
        realm,
        ctx,
        by,
        Some(key),
        name,
        act,
        async move |by: &Agent, rows: &Rows<'_>, act: &A| {
            if let Some(stood) = callers::standing(rows, by.0).await?
            {
                return Ok(Some(Barred::Stood(stood)));
            }
            let kept =
                reserved::reserve(rows, tool, by.0, &called, act)
                    .await?;
            Ok(kept.map(|kept| match kept {
                Kept::Waits(kept) => Barred::Reserved(kept),
                Kept::Refused(why) => Barred::Stood(Stood::Gate(why)),
            }))
        },
        |wait| Ok(Stood::Gate(Gate::Reserved { wait })),
    )
    .await
}

/// The call that carried a society tool, with its turn's task and
/// conversation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Called {
    pub call: CallId,
    pub task: Option<RealmTask>,
    pub conversation: Option<ConversationId>,
}

/// An act with the call that carried it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Carried<T> {
    pub called: Called,
    pub act: T,
}

/// A read as the realm takes it, with the secrets its text must not
/// hold.
#[derive(Serialize)]
pub(super) struct Reading<T> {
    pub read: T,
    #[serde(skip)]
    pub secrets: Vec<String>,
}

/// A society tool's act as the realm runs it for a caller of kind
/// `By`, given the call that carried it.
pub(super) trait ToolAct<By>: ToolCall {
    type Run: Act<By, Output = Self::Answer> + Serialize;

    fn run(self, called: &Called) -> Self::Run;
}

impl<By, T> ToolAct<By> for T
where
    T: ToolCall + Act<By, Output = T::Answer> + Serialize,
{
    type Run = T;

    fn run(self, _called: &Called) -> T {
        self
    }
}

/// The marker key of `by`'s call of `tool`, the call, and the act it
/// runs.
pub(super) fn keyed<By: Witness, T: ToolAct<By>>(
    by: &By,
    name: &'static str,
    tool: Tool<T>,
) -> Result<(marker::Key, Called, T::Run)> {
    let (key, called, act) = called(by, name, tool)?;
    let act = act.run(&called);
    Ok((key, called, act))
}

/// The marker key of `tool`, the call that carried it, and its act as
/// it came.
pub(super) fn called<By: Witness, T: Serialize>(
    by: &By,
    name: &'static str,
    tool: Tool<T>,
) -> Result<(marker::Key, Called, T)> {
    let key = marker::Key::of(
        by,
        marker::Id::Call(tool.call.clone()),
        name,
        &tool,
    )?;
    let Tool {
        call,
        task,
        conversation,
        act,
    } = tool;
    let called = Called {
        call,
        task,
        conversation,
    };
    Ok((key, called, act))
}

/// An act that the calling turn's task pays for.
#[derive(Serialize)]
pub(super) struct Paid<A> {
    pub task: Option<RealmTask>,
    pub act: A,
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
    key: Option<marker::Key>,
    name: &'static str,
    act: A,
    bar: impl AsyncFn(&By, &Rows<'_>, &A) -> Result<Option<Barred<S>>>,
    reserved: impl Fn(WaitId) -> Result<S>,
) -> Result<Result<A::Output, S>>
where
    By: Witness,
    A: Act<By> + Serialize,
{
    hamlet_host::fault::point(&format!("called/{name}"));
    let mut waited = false;
    let (done, pushes, lined) = loop {
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
            } => {
                let calls = act.deliveries(&by, &done);
                let lined =
                    realm.lanes.line(&tx.rows(), calls).await?;
                break (done, pushes, lined);
            }
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
                hamlet_host::fault::point(&format!(
                    "committed/{name}"
                ));
                after_reserving(realm, ctx, pushes);
                return Ok(Err(reserved(wait)?));
            }
            Read::Decided(decision) => decision,
        };
        match decision {
            Decision::Answer(output) => return Ok(Ok(output)),
            Decision::Again(done) => {
                let calls = act.deliveries(&by, &done);
                let lined =
                    realm.lanes.line(&tx.rows(), calls).await?;
                break (done, Vec::new(), lined);
            }
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
                let calls = act.deliveries(&by, &decided.done);
                let (pushes, lined) = tx
                    .commit_lined(
                        realm,
                        change::Write {
                            changes: decided.changes,
                            entries: decided.entries,
                            marker: key
                                .as_ref()
                                .map(|key| (key, &ran)),
                        },
                        calls,
                    )
                    .await?;
                hamlet_host::fault::point(&format!(
                    "committed/{name}"
                ));
                break (decided.done, pushes, lined);
            }
        }
    };
    let Lined {
        before_answer,
        after_answer,
    } = lined;
    let made = calls::deliver_all(
        realm,
        ctx,
        before_answer,
        When::BeforeAnswer,
    )
    .await;
    let rows = act.answers_after(&done);
    ctx.register_task(calls::after_answer(
        realm.clone(),
        ctx.clone(),
        after_answer,
        pushes,
    ));
    let within = Instant::now() + realm.answer_within;
    for &row in &rows {
        realm
            .drives
            .ended(
                row,
                within.saturating_duration_since(Instant::now()),
            )
            .await;
    }
    // A drive commits a failure's end before it ends, so the rows
    // read here say whether the steps failed.
    if steps::any_failed(&Rows::outside(ctx)?, &rows).await? {
        return Ok(Ok(act.failed(done, &made)));
    }
    Ok(Ok(act.answer(done, &made)))
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
    look.look(by, realm)
}
