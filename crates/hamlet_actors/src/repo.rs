//! The one way to a repository and the store beside it.
//!
//! An [`Owner`] takes every call through [`admit`], which checks the
//! caller and hands the action a [`Call`]. An action is straight-line
//! code over that call: it never sees its `Ctx`, a
//! `hamlet_git::Repository`, a SQL handle or a `HostRef`.
//!
//! - [`Call::refuse`] answers a request whose shape is refused.
//! - [`Call::read`] runs a closure that reads, beside other reads.
//! - [`Call::once`] runs a closure that may change the repository and
//!   the store, once per id, alone.
//!
//! A closure runs on a blocking thread holding the owner's exclusion.
//! It is not `async`, so nothing is awaited while the exclusion is
//! held. A call to another actor is queued by [`Writer::tell`] and
//! made after the unit. The unit, from the id's lookup to the store's
//! commit, is one blocking job, so an actor's stop cannot fall
//! between a ref's move and the marker.

mod checkout;
pub(crate) mod git;
pub(crate) mod parts;
pub(crate) mod store;
#[cfg(test)]
pub(crate) mod testing;

use std::future::Future;
use std::marker::PhantomData;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use hamlet_actor::{Actor, Ctx, SqliteDb};
use hamlet_host::{Address, Caller, HostRef};
use hamlet_messages::address::Party;
use hamlet_messages::realm::Refusable;
use hamlet_messages::sender::{Sender, sender, wrong_caller};
use hamlet_messages::{Changes, Message};
use tokio::runtime::Handle;
use tokio::sync::RwLock;

use self::git::{Folder, Git, Held, Reads};
use self::store::{Rows, Sql, Store, Table, UnitSql, Writes, marker};
use crate::call;
use crate::workspace::calls::now_ms;

/// The longest a call made after a unit waits.
const TELL_WITHIN: Duration = Duration::from_secs(10);
/// The longest a call made before a unit waits.
const ASK_WITHIN: Duration = Duration::from_secs(4);

/// An actor that keeps a repository and a store beside it.
pub(crate) trait Owner:
    Actor<ConnParams = Option<Caller>> + Sized
{
    /// The party its calls to other actors are stamped as.
    type Party: Party;

    fn repo(&self) -> &Repo;

    /// Runs at the end of every unit that ended kept, its first run
    /// and the repeat of one a kill cut off, before the marker.
    fn settle(_settle: &mut Settle<'_, Self>) -> Result<()> {
        Ok(())
    }
}

/// A kind of sender an [`Owner`] takes calls from.
pub(crate) trait Admits<S: Sender>: Owner {
    /// The store's record of the caller, handed to the action.
    type Known: Send + 'static;

    /// `None` for a caller the store has no row for. Its call fails
    /// as a wrong caller's does.
    fn known(
        store: &Store<'_>,
        from: &S,
    ) -> Result<Option<Self::Known>>;
}

/// An action: the whole of what an owner does with one message.
pub(crate) trait Takes<M: Message>:
    Owner + Admits<M::From>
{
    fn take(
        call: Call<Self, M>,
    ) -> impl Future<Output = Result<Reply<M>>> + Send;
}

/// An owner's repository behind its exclusion: a unit holds it
/// alone, reads share it.
pub(crate) struct Repo {
    folder: Arc<RwLock<Folder>>,
    host: HostRef,
}

impl Repo {
    /// Opens an owner's repository at its start: the store's tables,
    /// then the folder at `path`, made by `make` when it is not
    /// there, then `ready` over it.
    pub async fn start<A: Owner>(
        ctx: &Ctx<A>,
        schema: &str,
        path: PathBuf,
        make: impl FnOnce(&Git<'_>) -> Result<()> + Send + 'static,
        ready: impl FnOnce(&Held) -> Result<()> + Send + 'static,
    ) -> Result<Self> {
        let host = HostRef::of(ctx.inner())?;
        ctx.sql()
            .exec(format!("{};{schema}", marker::SCHEMA))
            .await?;
        let home = host.folder().to_owned();
        let at = now_ms()?;
        let folder = tokio::task::spawn_blocking(move || {
            let folder = Folder::start(path, home, at, make)?;
            ready(&folder.open(at)?)?;
            Ok::<_, anyhow::Error>(folder)
        })
        .await??;
        Ok(Self {
            folder: Arc::new(RwLock::new(folder)),
            host,
        })
    }

    /// `look`'s read of the repository, beside other reads, outside
    /// any call.
    pub async fn reads<T, F>(&self, look: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Reads) -> Result<T> + Send + 'static,
    {
        let folder = self.folder.clone().read_owned().await;
        let at = now_ms()?;
        tokio::task::spawn_blocking(move || {
            look(folder.open(at)?.reads()?)
        })
        .await?
    }
}

/// A message as it arrived: who sent it, by the host's stamp.
pub(crate) struct Asked<M: Message> {
    caller: Caller,
    from: M::From,
    message: M,
    /// Milliseconds since the epoch: the time of every commit and
    /// row the call makes.
    at: i64,
}

impl<M> Clone for Asked<M>
where
    M: Message + Clone,
    M::From: Clone,
{
    fn clone(&self) -> Self {
        Self {
            caller: self.caller.clone(),
            from: self.from.clone(),
            message: self.message.clone(),
            at: self.at,
        }
    }
}

/// A call whose caller the host's stamp admits.
pub(crate) struct Call<A: Takes<M>, M: Message> {
    owner: Arc<A>,
    ctx: Ctx<A>,
    asked: Asked<M>,
}

/// An answer. Only this module makes one, so an action answers what
/// [`Call::refuse`], [`Call::read`] or [`Call::once`] gave it.
pub(crate) struct Reply<M: Message>(M::Output);

/// The body of every `Handler` of an owner: the caller's check, then
/// the action.
pub(crate) async fn admit<A, M>(
    owner: Arc<A>,
    ctx: Ctx<A>,
    message: M,
) -> Result<M::Output>
where
    A: Takes<M>,
    M: Message,
{
    let from = sender(&ctx, &message)?;
    let caller = ctx
        .conn()
        .context("the call has no connection")?
        .params()?
        .context("the call has no caller")?;
    let asked = Asked {
        caller,
        from,
        message,
        at: now_ms()?,
    };
    let Reply(answer) = A::take(Call { owner, ctx, asked }).await?;
    Ok(answer)
}

impl<A: Takes<M>, M: Message> Deref for Call<A, M> {
    type Target = M;

    fn deref(&self) -> &M {
        &self.asked.message
    }
}

impl<A, M> Call<A, M>
where
    A: Takes<M>,
    M: Message,
    M::From: Send + Sync + 'static,
{
    pub fn owner(&self) -> &A {
        &self.owner
    }

    /// Answers `refused` to a request whose shape is refused, after
    /// its caller is admitted and before its id is looked at.
    pub async fn refuse(
        self,
        why: <M::Output as Refusable>::Why,
    ) -> Result<Reply<M>>
    where
        M::Output: Refusable,
        <M::Output as Refusable>::Why: Send + 'static,
    {
        self.read(move |_| Ok(M::Output::refused(why))).await
    }

    /// Answers what `look` reads.
    pub async fn read<F>(self, look: F) -> Result<Reply<M>>
    where
        F: FnOnce(&Look<'_, A, M>) -> Result<M::Output>
            + Send
            + 'static,
    {
        let db = self.ctx.sql().clone();
        shared(self.owner.repo(), db, self.asked, look)
            .await
            .map(Reply)
    }
}

/// Runs `look` on a blocking thread beside other reads.
async fn shared<A, M, T, F>(
    repo: &Repo,
    db: SqliteDb,
    asked: Asked<M>,
    look: F,
) -> Result<T>
where
    A: Admits<M::From>,
    M: Message,
    M::From: Send + Sync + 'static,
    T: Send + 'static,
    F: FnOnce(&Look<'_, A, M>) -> Result<T> + Send + 'static,
{
    let folder = repo.folder.clone().read_owned().await;
    let runtime = Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut held = folder.open(asked.at)?;
        let sql = UnitSql::new(db, runtime);
        let seen = read::<A, M, T, F>(&mut held, &sql, &asked, look);
        sql.rollback();
        drop(folder);
        seen
    })
    .await?
}

impl<A, M> Call<A, M>
where
    A: Takes<M>,
    M: Changes,
    M::From: Send + Sync + 'static,
{
    /// Runs `work` unless a kept call holds the message's id, and
    /// answers what it ended with; a repeat gets what the first run
    /// was answered.
    pub async fn once<F>(self, work: F) -> Result<Reply<M>>
    where
        F: FnOnce(Before<'_, A, M>) -> Result<Ended<M>>
            + Send
            + 'static,
    {
        let folder =
            self.owner.repo().folder.clone().write_owned().await;
        let (db, runtime, asked) =
            (self.ctx.sql().clone(), Handle::current(), self.asked);
        let changed = tokio::task::spawn_blocking(move || {
            let mut held = folder.open(asked.at)?;
            let sql = UnitSql::new(db, runtime);
            let changed =
                change::<A, M, F>(&mut held, &sql, &asked, work)?;
            sql.commit()?;
            drop(folder);
            Ok::<_, anyhow::Error>(changed)
        })
        .await??;
        if !changed.told.is_empty() {
            let host = self.owner.repo().host.clone();
            self.ctx.register_task(tell_all(host, changed.told));
        }
        if changed.ends {
            self.ctx.destroy()?;
        }
        Ok(Reply(changed.answer))
    }
}

impl<A, M> Call<A, M>
where
    A: Takes<M>,
    M: Changes,
    M::From: Send + Sync + 'static,
{
    /// Admits the caller and checks the id now, so that the action
    /// may call other actors before it writes. A repeat gets its kept
    /// answer, and a reused id fails, before any of those calls is
    /// made.
    pub async fn fresh(self) -> Result<Checked<A, M>>
    where
        M: Clone,
        M::From: Clone,
    {
        let folder =
            self.owner.repo().folder.clone().read_owned().await;
        let (db, runtime) =
            (self.ctx.sql().clone(), Handle::current());
        let key =
            marker::Key::of(&self.asked.caller, &self.asked.message)?;
        let asked = self.asked.clone();
        let kept = tokio::task::spawn_blocking(move || {
            let sql = UnitSql::new(db, runtime);
            let kept =
                known::<A, M>(&sql, &asked).and_then(
                    |_| match &key {
                        Some(key) => marker::find::<M>(&sql, key),
                        None => Ok(None),
                    },
                );
            sql.rollback();
            drop(folder);
            kept
        })
        .await??;
        Ok(match kept {
            Some(answer) => Checked::Repeat(Reply(answer)),
            None => Checked::First(Fresh(self)),
        })
    }
}

/// The outcome of [`Call::fresh`] under the id.
#[expect(
    clippy::large_enum_variant,
    reason = "made once per call and matched at once, never kept"
)]
pub(crate) enum Checked<A: Takes<M>, M: Changes> {
    First(Fresh<A, M>),
    /// The answer the first run was given. An action can only return
    /// it.
    Repeat(Reply<M>),
}

/// A call whose id no kept call holds.
pub(crate) struct Fresh<A: Takes<M>, M: Changes>(Call<A, M>);

impl<A: Takes<M>, M: Changes> Deref for Fresh<A, M> {
    type Target = M;

    fn deref(&self) -> &M {
        &self.0
    }
}

impl<A, M> Fresh<A, M>
where
    A: Takes<M>,
    M: Changes,
    M::From: Send + Sync + 'static,
{
    /// Answers what `look` reads, beside other reads: what to ask
    /// before the unit.
    pub async fn look<T, F>(&self, look: F) -> Result<T>
    where
        M: Clone,
        M::From: Clone,
        T: Send + 'static,
        F: FnOnce(&Look<'_, A, M>) -> Result<T> + Send + 'static,
    {
        let Call { owner, ctx, asked } = &self.0;
        shared(owner.repo(), ctx.sql().clone(), asked.clone(), look)
            .await
    }

    /// Calls each of `to` at once, each within [`ASK_WITHIN`], and
    /// gives each answer or its error, in order. No exclusion is
    /// held: none exists outside a closure.
    pub async fn ask_all<T>(
        &self,
        to: &[(T::To, T)],
    ) -> Vec<Result<T::Output>>
    where
        T: Message<From = A::Party>,
    {
        let host = &self.0.owner.repo().host;
        let calls = to
            .iter()
            .map(|(to, message)| call(host, to, message, ASK_WITHIN));
        futures::future::join_all(calls).await
    }

    pub async fn once<F>(self, work: F) -> Result<Reply<M>>
    where
        F: FnOnce(Before<'_, A, M>) -> Result<Ended<M>>
            + Send
            + 'static,
    {
        self.0.once(work).await
    }
}

/// A closure's view: the message, the store's record of its caller,
/// the repository and the store.
pub(crate) struct Look<'a, A: Admits<M::From>, M: Message> {
    held: &'a mut Held,
    sql: &'a dyn Sql,
    asked: &'a Asked<M>,
    from: &'a A::Known,
}

impl<A: Admits<M::From>, M: Message> Look<'_, A, M> {
    pub fn message(&self) -> &M {
        &self.asked.message
    }

    /// The time of the call, in milliseconds since the epoch.
    pub fn at(&self) -> i64 {
        self.asked.at
    }

    /// The store's record of the caller.
    pub fn from(&self) -> &A::Known {
        self.from
    }

    pub fn git(&self) -> Result<&Reads> {
        self.held.reads()
    }

    /// The realm folder, which the checkouts are under.
    pub fn home(&self) -> &Path {
        self.held.home()
    }

    /// The changes that make the files of `base` those of a folder
    /// under the realm folder, both but for the paths `skip` names.
    pub fn changes(
        &self,
        under: &Path,
        base: hamlet_git::CommitId,
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<hamlet_git::Change>> {
        self.held.changes(under, base, skip)
    }

    /// The rows of a table, for the module that owns it: only that
    /// module can make its token.
    pub fn rows<T: Table>(&self, owner: T) -> Rows<'_, T> {
        Store(self.sql).rows(owner)
    }
}

/// A unit before its first write. It ends refused, having kept
/// nothing, ends kept with nothing to change, or goes on to write.
pub(crate) struct Before<'a, A: Admits<M::From>, M: Message> {
    look: Look<'a, A, M>,
    told: &'a mut Vec<Told>,
}

impl<'a, A: Admits<M::From>, M: Message> Deref for Before<'a, A, M> {
    type Target = Look<'a, A, M>;

    fn deref(&self) -> &Self::Target {
        &self.look
    }
}

impl<'a, A: Admits<M::From>, M: Message> Before<'a, A, M> {
    /// Ends the unit refused. Nothing was written: only
    /// [`Before::write`] gives a writer, and it takes `self` too.
    pub fn refuse(
        self,
        why: <M::Output as Refusable>::Why,
    ) -> Result<Ended<M>>
    where
        M::Output: Refusable,
    {
        Ok(Ended::of(End::Refused(M::Output::refused(why))))
    }

    /// Ends a unit that found nothing to change, or found its own
    /// effect from a run a kill cut off, with the answer kept.
    pub fn kept(self, answer: M::Output) -> Result<Ended<M>> {
        Ok(Ended::of(End::Kept(answer)))
    }

    pub fn write(self) -> Writer<'a, A, M> {
        Writer {
            look: self.look,
            told: self.told,
        }
    }
}

/// A unit that writes. It can only end kept.
pub(crate) struct Writer<'a, A: Admits<M::From>, M: Message> {
    look: Look<'a, A, M>,
    told: &'a mut Vec<Told>,
}

impl<'a, A: Admits<M::From>, M: Message> Deref for Writer<'a, A, M> {
    type Target = Look<'a, A, M>;

    fn deref(&self) -> &Self::Target {
        &self.look
    }
}

impl<A: Admits<M::From>, M: Message> Writer<'_, A, M> {
    pub fn writes(&self) -> Result<Git<'_>> {
        self.look.held.writes()
    }

    /// The rows of a table, to write, for the module that owns it.
    pub fn table<T: Table>(&self, _owner: T) -> Writes<'_, T> {
        Writes::new(self.look.sql)
    }

    /// Takes `file` out of the whole history: each branch's name,
    /// old tip and new tip.
    pub fn purge(
        &mut self,
        file: &str,
    ) -> Result<
        Vec<(String, hamlet_git::CommitId, hamlet_git::CommitId)>,
    > {
        self.look.held.purge(file)
    }

    /// Makes a folder under the realm folder hold the files of `at`,
    /// but for the paths `skip` names.
    pub fn check_out(
        &mut self,
        at: hamlet_git::CommitId,
        under: &Path,
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<()> {
        self.look.held.check_out(at, under, skip)
    }

    pub fn remove_folder(&mut self, under: &Path) -> Result<()> {
        self.look.held.remove_folder(under)
    }

    /// Queues a call to another actor, made once the unit is kept and
    /// the exclusion released. A call that fails is logged and
    /// dropped, and a repeat answered from the marker makes none.
    pub fn tell<T>(&mut self, to: &T::To, message: &T) -> Result<()>
    where
        T: Message<From = A::Party>,
    {
        self.told.push(Told::of(to, message)?);
        Ok(())
    }

    pub fn kept(self, answer: M::Output) -> Result<Ended<M>> {
        Ok(Ended::of(End::Kept(answer)))
    }

    /// Ends the unit kept, then removes the repository, and the owner
    /// destroys itself once it has answered.
    pub fn erase(self, answer: M::Output) -> Result<Ended<M>> {
        Ok(Ended {
            end: End::Kept(answer),
            ends: true,
        })
    }
}

/// The outcome of a unit. Only a [`Before`] or a [`Writer`] makes
/// one.
pub(crate) struct Ended<M: Message> {
    end: End<M::Output>,
    /// The owner destroys itself once it has answered: an erase.
    ends: bool,
}

impl<M: Message> Ended<M> {
    fn of(end: End<M::Output>) -> Self {
        Self { end, ends: false }
    }
}

enum End<O> {
    Refused(O),
    Kept(O),
}

/// The owner's settle step, run after a kept unit.
pub(crate) struct Settle<'a, A: Owner> {
    held: &'a Held,
    sql: &'a dyn Sql,
    told: &'a mut Vec<Told>,
    owner: PhantomData<A>,
}

impl<A: Owner> Settle<'_, A> {
    pub fn git(&self) -> Result<&Reads> {
        self.held.reads()
    }

    pub fn writes(&self) -> Result<Git<'_>> {
        self.held.writes()
    }

    /// The rows of a table, to write, for the module that owns it.
    pub fn table<T: Table>(&self, _owner: T) -> Writes<'_, T> {
        Writes::new(self.sql)
    }

    pub fn tell<T>(&mut self, to: &T::To, message: &T) -> Result<()>
    where
        T: Message<From = A::Party>,
    {
        self.told.push(Told::of(to, message)?);
        Ok(())
    }
}

/// A call queued for after the unit, as the host carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Told {
    to: Address,
    name: &'static str,
    args: Vec<u8>,
}

impl Told {
    pub fn of<T: Message>(to: &T::To, message: &T) -> Result<Self> {
        Ok(Self {
            to: to.address()?,
            name: T::NAME,
            args: hamlet_actor::action::encode_positional(message)?,
        })
    }
}

/// The end of a unit that may change.
struct Changed<O> {
    answer: O,
    told: Vec<Told>,
    ends: bool,
}

/// A read over any store: the caller's row, then `look`.
fn read<A, M, T, F>(
    held: &mut Held,
    sql: &dyn Sql,
    asked: &Asked<M>,
    look: F,
) -> Result<T>
where
    A: Admits<M::From>,
    M: Message,
    F: FnOnce(&Look<'_, A, M>) -> Result<T>,
{
    let from = known::<A, M>(sql, asked)?;
    look(&Look {
        held,
        sql,
        asked,
        from: &from,
    })
}

fn known<A, M>(sql: &dyn Sql, asked: &Asked<M>) -> Result<A::Known>
where
    A: Admits<M::From>,
    M: Message,
{
    A::known(&Store(sql), &asked.from)?.ok_or_else(|| {
        wrong_caller(M::NAME, Some(asked.caller.clone()))
    })
}

/// A unit that may change, over any store: the caller's row, the id,
/// the work, the settle step, the marker.
fn change<A, M, F>(
    held: &mut Held,
    sql: &dyn Sql,
    asked: &Asked<M>,
    work: F,
) -> Result<Changed<M::Output>>
where
    A: Admits<M::From>,
    M: Changes,
    F: FnOnce(Before<'_, A, M>) -> Result<Ended<M>>,
{
    let from = known::<A, M>(sql, asked)?;
    let key = marker::Key::of(&asked.caller, &asked.message)?;
    if let Some(key) = &key
        && let Some(answer) = marker::find::<M>(sql, key)?
    {
        return Ok(Changed {
            answer,
            told: Vec::new(),
            ends: false,
        });
    }
    let mut told = Vec::new();
    let ended = work(Before {
        look: Look {
            held,
            sql,
            asked,
            from: &from,
        },
        told: &mut told,
    })?;
    let answer = match ended.end {
        End::Refused(answer) => {
            return Ok(Changed {
                answer,
                told: Vec::new(),
                ends: false,
            });
        }
        End::Kept(answer) => {
            ensure!(
                !is_refused(&answer)?,
                "{} wrote and then answered refused",
                M::NAME
            );
            answer
        }
    };
    if ended.ends {
        held.erase()?;
    } else {
        A::settle(&mut Settle {
            held,
            sql,
            told: &mut told,
            owner: PhantomData,
        })?;
    }
    if let Some(key) = &key {
        marker::keep(sql, key, &answer, asked.at)?;
    }
    Ok(Changed {
        answer,
        told,
        ends: ended.ends,
    })
}

/// Whether `answer` is its family's `refused` arm.
fn is_refused<T: serde::Serialize>(answer: &T) -> Result<bool> {
    let refused = ciborium::Value::Text("refused".into());
    Ok(match ciborium::Value::serialized(answer)? {
        ciborium::Value::Map(fields) => {
            fields.iter().any(|(name, value)| {
                name.as_text() == Some("type") && *value == refused
            })
        }
        _ => false,
    })
}

async fn tell_all(host: HostRef, told: Vec<Told>) {
    for Told { to, name, args } in told {
        if let Err(error) =
            host.call(&to, name, args, TELL_WITHIN).await
        {
            tracing::warn!(?error, %to, name, "dropped a call");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use hamlet_actor::{Action, Handler, Registry};
    use hamlet_actor_core::{ActorConfig, ActorFactory};
    use hamlet_actor_error::ActorError;
    use hamlet_git::{Change, MAIN};
    use hamlet_host::{Host, HostOptions};
    use hamlet_messages::Repeat;
    use hamlet_messages::address::{self, Agent};
    use hamlet_messages::profile::{
        CommitId, Moved, Removal, RemovePath, Sha1,
    };
    use hamlet_messages::realm::ProfileId;
    use hamlet_messages::sender::Client;
    use hamlet_messages::workspace::AgentNumber;
    use hamlet_testing::Scratch;
    use serde::{Deserialize, Serialize};

    use super::git::By;
    use super::testing::Hostless;
    use super::*;

    /// An owner with no table of its own, called by a client.
    struct Probe {
        repo: Repo,
    }

    /// Asks [`ASKED`] to remove the path `a.md` holds, then tells
    /// agent 7 `main`'s tip.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Ask {
        id: String,
    }

    impl Action for Ask {
        type Output = Vec<Option<Removal>>;
        const NAME: &'static str = "ask";
    }

    impl Message for Ask {
        type To = address::Profile;
        type From = Client;
    }

    impl Changes for Ask {
        fn repeat(&self) -> Repeat<'_> {
            Repeat::Id(&self.id)
        }
    }

    /// Erases the probe's repository.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Wipe {
        id: String,
    }

    impl Action for Wipe {
        type Output = ();
        const NAME: &'static str = "wipe";
    }

    impl Message for Wipe {
        type To = address::Profile;
        type From = Client;
    }

    impl Changes for Wipe {
        fn repeat(&self) -> Repeat<'_> {
            Repeat::Id(&self.id)
        }
    }

    /// The agents [`Ask`] asks: 7 removes the path, 8 removes it
    /// later, 9 fails.
    const ASKED: [u32; 3] = [7, 8, 9];

    const REPOSITORY: &str = "probe.git";

    fn made(git: &Git<'_>) -> Result<()> {
        let file = Change::Write {
            path: "a.md".into(),
            bytes: b"a".to_vec(),
        };
        git.commit(MAIN, None, &[file], "made", By::Realm).map(drop)
    }

    #[async_trait::async_trait]
    impl Actor for Probe {
        type Input = ();
        type Actions = (Ask, Wipe);
        type Events = ();
        type ConnParams = Option<Caller>;
        type Action = ();

        async fn create(ctx: &Ctx<Self>) -> Result<Self> {
            let path =
                HostRef::of(ctx.inner())?.folder().join(REPOSITORY);
            let repo =
                Repo::start(ctx, "", path, made, |_| Ok(())).await?;
            Ok(Self { repo })
        }
    }

    impl<M> Handler<M> for Probe
    where
        M: Message,
        M::From: Send + Sync + 'static,
        Probe: Takes<M>,
    {
        async fn handle(
            self: Arc<Self>,
            ctx: Ctx<Self>,
            message: M,
        ) -> Result<M::Output> {
            admit(self, ctx, message).await
        }
    }

    impl Owner for Probe {
        type Party = address::Profile;

        fn repo(&self) -> &Repo {
            &self.repo
        }
    }

    impl Admits<Client> for Probe {
        type Known = ();

        fn known(_: &Store<'_>, _: &Client) -> Result<Option<()>> {
            Ok(Some(()))
        }
    }

    fn tip(git: &Reads) -> Result<CommitId> {
        let tip = git.tip(MAIN)?.context("a probe has a main")?;
        Ok(CommitId(
            Sha1::from_hex(&tip.to_string()).context("a SHA-1")?,
        ))
    }

    impl Takes<Ask> for Probe {
        async fn take(call: Call<Self, Ask>) -> Result<Reply<Ask>> {
            let fresh = match call.fresh().await? {
                Checked::Repeat(reply) => return Ok(reply),
                Checked::First(fresh) => fresh,
            };
            let path = fresh.look(|look| {
                let main = look
                    .git()?
                    .tip(MAIN)?
                    .context("a probe has a main")?;
                let read = look.git()?.read(main, "a.md")?;
                Ok(String::from_utf8(
                    read.context("a probe has a.md")?,
                )?)
            });
            let path = path.await?;
            let to = ASKED.map(|n| {
                let path = path.clone();
                (Agent(AgentNumber(n)), RemovePath { path })
            });
            let answers = fresh
                .ask_all(&to)
                .await
                .into_iter()
                .map(Result::ok)
                .collect();
            fresh
                .once(move |unit| {
                    let tip = tip(unit.git()?)?;
                    let mut writer = unit.write();
                    let moved = Moved {
                        tip,
                        character: None,
                    };
                    writer.tell(&Agent(AgentNumber(7)), &moved)?;
                    writer.kept(answers)
                })
                .await
        }
    }

    impl Takes<Wipe> for Probe {
        async fn take(call: Call<Self, Wipe>) -> Result<Reply<Wipe>> {
            call.once(|unit| unit.write().erase(())).await
        }
    }

    /// The calls the agents were sent, as `<call>:<agent>`.
    #[derive(Default)]
    struct Heard(Mutex<Vec<String>>);

    /// Stands in for the agents.
    struct Stand {
        number: u32,
        heard: Arc<Heard>,
    }

    impl Stand {
        fn hear(&self, name: &str) {
            let mut heard = self.heard.0.lock().unwrap();
            heard.push(format!("{name}:{}", self.number));
        }
    }

    #[async_trait::async_trait]
    impl Actor for Stand {
        type Input = ();
        type Actions = (RemovePath, Moved);
        type Events = ();
        type ConnParams = Option<Caller>;
        type Action = ();

        async fn create(ctx: &Ctx<Self>) -> Result<Self> {
            let host = HostRef::of(ctx.inner())?;
            Ok(Self {
                number: host.own_address().key().parse()?,
                heard: host
                    .value::<Heard>()
                    .context("the test gives what is heard")?,
            })
        }
    }

    impl Handler<RemovePath> for Stand {
        async fn handle(
            self: Arc<Self>,
            _: Ctx<Self>,
            asked: RemovePath,
        ) -> Result<Removal> {
            self.hear(&format!(
                "{} {}",
                RemovePath::NAME,
                asked.path
            ));
            match self.number {
                7 => Ok(Removal::Removed),
                8 => Ok(Removal::Later),
                _ => anyhow::bail!(
                    "agent {} cannot remove it",
                    self.number
                ),
            }
        }
    }

    impl Handler<Moved> for Stand {
        async fn handle(
            self: Arc<Self>,
            _: Ctx<Self>,
            _: Moved,
        ) -> Result<()> {
            self.hear(Moved::NAME);
            Ok(())
        }
    }

    fn factory<A: Actor>(kind: &str) -> ActorFactory {
        let mut registry = Registry::new();
        let config = ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        };
        registry.register_actor_with::<A>(kind, config);
        registry.into_factories().remove(kind).unwrap()
    }

    fn probe() -> Address {
        address::Profile(ProfileId(1)).address().unwrap()
    }

    async fn open(dir: &Path, heard: Arc<Heard>) -> Host {
        let kinds = HashMap::from([
            (
                address::Profile::KIND.to_owned(),
                factory::<Probe>(address::Profile::KIND),
            ),
            (Agent::KIND.to_owned(), factory::<Stand>(Agent::KIND)),
        ]);
        let host = Host::open_with(
            dir,
            kinds,
            HostOptions::default().with_value(heard),
        )
        .unwrap();
        host.get_or_start(&probe()).await.unwrap();
        for number in ASKED {
            let agent = Agent(AgentNumber(number)).address().unwrap();
            host.get_or_start(&agent).await.unwrap();
        }
        host
    }

    async fn ask<M: Message>(
        host: &Host,
        message: &M,
    ) -> Result<M::Output> {
        let args = hamlet_actor::action::encode_positional(message)?;
        let answer = host
            .call_for_client(
                &probe(),
                M::NAME,
                args,
                Duration::from_secs(20),
            )
            .await?;
        Ok(ciborium::from_reader(answer.as_slice())?)
    }

    async fn until(what: &str, done: impl Fn() -> bool) {
        let waited =
            tokio::time::timeout(Duration::from_secs(5), async {
                while !done() {
                    tokio::time::sleep(Duration::from_millis(10))
                        .await;
                }
            });
        waited.await.unwrap_or_else(|_| panic!("{what} within 5s"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_fresh_call_asks_each_agent_once_and_tells_after_its_unit()
     {
        let dir = Scratch::new("repo-ask");
        let heard = Arc::new(Heard::default());
        let host = open(&dir, heard.clone()).await;
        let asked = Ask { id: "a".into() };
        let answers = ask(&host, &asked).await.unwrap();
        assert_eq!(
            answers,
            [Some(Removal::Removed), Some(Removal::Later), None]
        );
        assert_eq!(ask(&host, &asked).await.unwrap(), answers);
        until("agent 7 told", || {
            heard.0.lock().unwrap().contains(&"moved:7".into())
        })
        .await;
        let mut heard = heard.0.lock().unwrap().clone();
        heard.sort();
        assert_eq!(
            heard,
            [
                "moved:7",
                "remove_path a:7",
                "remove_path a:8",
                "remove_path a:9"
            ]
        );
        host.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_erase_removes_the_repository_and_ends_the_owner() {
        let dir = Scratch::new("repo-erase");
        let host = open(&dir, Arc::default()).await;
        ask(&host, &Wipe { id: "w".into() }).await.unwrap();
        assert!(!dir.join(REPOSITORY).exists());
        until("the probe stopped", || !host.is_running(&probe()))
            .await;
        let after =
            ask(&host, &Wipe { id: "w".into() }).await.unwrap_err();
        let error = ActorError::extract(&after);
        assert_eq!(
            (error.group(), error.code()),
            ("actor", "not_found")
        );
        host.shutdown().await.unwrap();
    }

    #[test]
    fn a_unit_reaches_its_checkouts_and_purges_its_history() {
        let dir = Scratch::new("repo-unit");
        let mut hostless =
            Hostless::new(&dir, dir.join(REPOSITORY), "", made)
                .unwrap();
        let under = Path::new("dreams/1/main");
        let asked = Ask { id: "a".into() };
        let (answer, told) = hostless
            .once::<Probe, Ask, _>(
                Caller::Client,
                asked.clone(),
                |unit| {
                    assert_eq!(unit.home(), &*dir);
                    assert_eq!(unit.from(), &());
                    assert_eq!(unit.at(), 1);
                    let base = unit
                        .git()?
                        .tip(MAIN)?
                        .context("a probe has a main")?;
                    let moved = Moved {
                        tip: tip(unit.git()?)?,
                        character: None,
                    };
                    let mut writer = unit.write();
                    writer.check_out(base, under, &|_| false)?;
                    std::fs::write(
                        dir.join(under).join("b.md"),
                        "b",
                    )?;
                    let changes =
                        writer.changes(under, base, &|_| false)?;
                    let b = Change::Write {
                        path: "b.md".into(),
                        bytes: b"b".to_vec(),
                    };
                    assert_eq!(changes, [b]);
                    writer.remove_folder(Path::new("dreams/1"))?;
                    let purged = writer.purge("a.md")?;
                    assert_eq!(purged.len(), 1);
                    assert_eq!(
                        writer.git()?.tip(MAIN)?,
                        Some(purged[0].2)
                    );
                    assert_eq!(
                        writer.git()?.read(purged[0].2, "a.md")?,
                        None
                    );
                    writer.tell(&Agent(AgentNumber(7)), &moved)?;
                    writer.kept(vec![Some(Removal::Removed)])
                },
            )
            .unwrap();
        assert_eq!(answer, [Some(Removal::Removed)]);
        assert_eq!(told.len(), 1);
        assert!(!dir.join("dreams/1").exists());

        let (again, told) = hostless
            .once::<Probe, Ask, _>(Caller::Client, asked, |_| {
                panic!("a repeat runs again")
            })
            .unwrap();
        assert_eq!((again, told), (answer, Vec::new()));
    }
}
