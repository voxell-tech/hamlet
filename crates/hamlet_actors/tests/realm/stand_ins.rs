//! The actors the realm calls, stood in for, and the host that
//! runs the realm among them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory, ActorLifecycle};
use hamlet_actors::realm;
use hamlet_host::{Caller, Host, HostOptions, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::chat::{Stop, Stopped};
use hamlet_messages::agent::copies::Copies;
use hamlet_messages::agent::{
    AgentInput, DropInbox, Erase, Fill, InboxCursor, InboxPart,
    InputId, OnTask, Place, PlaceVersion, ReadInbox, Retire, Retired,
    Submit, TakeInbox,
};
use hamlet_messages::bytes::Base64;
use hamlet_messages::library::{
    DropOfferParts, NoOffer, OfferRead, ReadOffer, ReadShelves,
    ShelfList, Skill, Verdict, VerdictRefusal,
};
use hamlet_messages::profile::{
    Branch, Bundle, CommitId, DropBranch, DropSaveParts, Edit,
    EndDream, MergeBranch, Piece, PrepareDream, ProfileInput, Purge,
    Read, Revert, Sha1, Source, Tag, Unmerge,
};
use hamlet_messages::realm::{Done, ProfileId, Refusable as _};
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::{
    AgentNumber, AnswerTaken, ChiefAnswer, InForce, ReachVersion,
    SetReach,
};
use hamlet_protocol::client::RequestId;
use hamlet_testing::Scratch;
use rusqlite::OpenFlags;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use super::profile_stand_in::ProfileStandIn;
use super::{DEADLINE, THREE, call, stopped_open};

/// A call of the realm's that a test can hold or fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Callee {
    Branch,
    /// A profile's read of a branch for a merge, by the profile's
    /// number.
    Bundle,
    Create,
    /// A profile's drop of a hire's branch, by the hire's number.
    DropBranch,
    DropInbox,
    /// The library's drop of an agent's offers under way, by the
    /// agent's number.
    DropOfferParts,
    /// A profile's drop of an agent's save under way, by the agent's
    /// number.
    DropSaveParts,
    /// A profile's edit, by the profile's number.
    Edit,
    /// A profile's end of a dream, by the profile's number.
    EndDream,
    /// An agent's erasure.
    Erase,
    /// A profile's erasure, by the profile's number.
    EraseProfile,
    Fill,
    /// A profile's making as a fork, by the profile's number.
    Fork,
    /// A profile's merge, by the absorbed hire's number.
    Merge,
    /// The workspace's measure of a parcel, by its sender.
    Parcel,
    Place,
    /// A profile's preparation of a dream, by the profile's number.
    PrepareDream,
    /// A profile's purge, by the profile's number.
    Purge,
    Read,
    ReadInbox,
    Retire,
    SetReach,
    /// An agent's stop, by the agent's number.
    Stop,
    Submit,
    TakeInbox,
    /// A profile's unmerge, by the profile's number.
    Unmerge,
    /// The library's verdict, by the offer's number.
    Verdict,
}

/// An agent's `inbox/`, by path: each file's bytes and whether it is
/// executable.
pub(super) type Inbox = BTreeMap<String, (Vec<u8>, bool)>;

#[derive(Default)]
pub(super) struct Seen {
    pub(super) made: Mutex<Vec<(AgentNumber, AgentInput)>>,
    pub(super) submits: Mutex<Vec<(AgentNumber, Submit)>>,
    /// Each submit and stop an agent took, in the order it took
    /// them.
    pub(super) took: Mutex<Vec<(AgentNumber, Took)>>,
    /// The turns each agent answers a stop with; one not here
    /// answers none.
    pub(super) turns: Mutex<HashMap<AgentNumber, u32>>,
    pub(super) places: Mutex<Vec<(AgentNumber, Place)>>,
    pub(super) fills: Mutex<Vec<(AgentNumber, Fill)>>,
    pub(super) copies: Mutex<Vec<(AgentNumber, Copies)>>,
    pub(super) reaches: Mutex<Vec<SetReach>>,
    pub(super) chief_answers: Mutex<Vec<ChiefAnswer>>,
    pub(super) branches: Mutex<Vec<(ProfileId, Branch)>>,
    pub(super) profiles: Mutex<Vec<(ProfileId, ProfileInput)>>,
    pub(super) reads: Mutex<Vec<(ProfileId, Read)>>,
    pub(super) edits: Mutex<Vec<(ProfileId, Edit)>>,
    pub(super) tags: Mutex<Vec<(ProfileId, Tag)>>,
    pub(super) reverts: Mutex<Vec<(ProfileId, Revert)>>,
    pub(super) purges: Mutex<Vec<(ProfileId, Purge)>>,
    /// Each preparation of a dream, with whether a purge was held
    /// when it came.
    pub(super) prepares: Mutex<Vec<(ProfileId, PrepareDream, bool)>>,
    pub(super) dream_ends: Mutex<Vec<(ProfileId, EndDream)>>,
    /// What the realm's step drives told, in order.
    pub(super) steps: Mutex<Vec<realm::StepTold>>,
    /// Each profile answers a dream's preparation with nothing to
    /// dream over.
    pub(super) nothing_to_dream: AtomicBool,
    pub(super) save_drops: Mutex<Vec<(ProfileId, DropSaveParts)>>,
    pub(super) offer_drops: Mutex<Vec<DropOfferParts>>,
    /// Each verdict the library was called with, failed calls too.
    pub(super) verdicts: Mutex<Vec<Verdict>>,
    pub(super) retires: Mutex<Vec<AgentNumber>>,
    pub(super) erases: Mutex<Vec<AgentNumber>>,
    pub(super) merges: Mutex<Vec<(ProfileId, MergeBranch)>>,
    pub(super) bundles: Mutex<Vec<(ProfileId, Bundle)>>,
    pub(super) unmerges: Mutex<Vec<(ProfileId, Unmerge)>>,
    pub(super) branch_drops: Mutex<Vec<(ProfileId, DropBranch)>>,
    pub(super) profile_erases: Mutex<Vec<ProfileId>>,
    pub(super) inboxes: Mutex<HashMap<AgentNumber, Inbox>>,
    /// What the callees keep of the calls above: each merge and
    /// unmerge once by its id, and each retire, erase and branch
    /// drop once by its agent, as a callee answers a repeat without
    /// doing it again.
    pub(super) kept: Mutex<Kept>,
    /// Each agent asked whether it is on a chief's task, with the
    /// question.
    pub(super) asked_on_task: Mutex<Vec<(AgentNumber, OnTask)>>,
    /// The agents that answer they are on a chief's task.
    pub(super) on_task: Mutex<HashSet<AgentNumber>>,
    failing: Mutex<HashSet<(Callee, AgentNumber)>>,
    /// The agents whose every call fails as an actor not ready.
    down: Mutex<HashSet<AgentNumber>>,
    /// The calls that fail every time, with an error of the callee's
    /// own, and how many times each came.
    refusing: Mutex<HashMap<(Callee, AgentNumber), usize>>,
    held: watch::Sender<HashSet<Callee>>,
    /// The calls that wait while the test holds their callee.
    pub(super) waiting: Mutex<HashSet<(Callee, AgentNumber)>>,
    /// The callees held for one agent's next call alone, with that
    /// agent and whether its call came.
    alone: Mutex<HashMap<Callee, (AgentNumber, bool)>>,
}

#[derive(Default)]
pub(super) struct Kept {
    pub(super) merges: Vec<RequestId>,
    pub(super) unmerges: Vec<RequestId>,
    pub(super) retires: Vec<AgentNumber>,
    pub(super) erases: Vec<AgentNumber>,
    pub(super) branch_drops: Vec<AgentNumber>,
}

/// Keeps `done` in `kept` unless it is there.
pub(super) fn keep_once<T: PartialEq>(kept: &mut Vec<T>, done: T) {
    if !kept.contains(&done) {
        kept.push(done);
    }
}

impl Seen {
    /// Waits while the test holds `callee`, then fails it if the
    /// test asked it to fail once for `agent`.
    pub(super) async fn called(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) -> anyhow::Result<()> {
        if self.down.lock().unwrap().contains(&agent) {
            return Err(ActorLifecycle::NotReady.build());
        }
        let mut held = self.held.subscribe();
        let passes = match self.alone.lock().unwrap().get_mut(&callee)
        {
            Some((held, came)) if *held == agent && !*came => {
                *came = true;
                false
            }
            Some(_) => true,
            None => false,
        };
        if !passes {
            if held.borrow().contains(&callee) {
                self.waiting.lock().unwrap().insert((callee, agent));
            }
            let released =
                held.wait_for(|held| !held.contains(&callee)).await;
            self.waiting.lock().unwrap().remove(&(callee, agent));
            released?;
        }
        if self.failing.lock().unwrap().remove(&(callee, agent)) {
            return Err(ActorLifecycle::NotReady.build());
        }
        if let Some(came) =
            self.refusing.lock().unwrap().get_mut(&(callee, agent))
        {
            *came += 1;
            bail!("the stand-in refuses {callee:?} of {}", agent.0);
        }
        Ok(())
    }

    /// Fails `agent`'s next call of `callee` as an actor not ready.
    pub(super) fn fail_once(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) {
        self.failing.lock().unwrap().insert((callee, agent));
    }

    pub(super) fn fail_for_good(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) {
        self.refusing.lock().unwrap().insert((callee, agent), 0);
    }

    /// How many times `callee` refused `agent`.
    pub(super) fn refused(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) -> usize {
        self.refusing
            .lock()
            .unwrap()
            .get(&(callee, agent))
            .copied()
            .unwrap_or(0)
    }

    pub(super) fn stop_refusing(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) {
        self.refusing.lock().unwrap().remove(&(callee, agent));
    }

    /// Fails every call of `agent`'s as an actor not ready, until
    /// [`Seen::up`].
    pub(super) fn down(&self, agent: AgentNumber) {
        self.down.lock().unwrap().insert(agent);
    }

    pub(super) fn up(&self, agent: AgentNumber) {
        self.down.lock().unwrap().remove(&agent);
    }

    pub(super) fn has_failed(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) -> bool {
        !self.failing.lock().unwrap().contains(&(callee, agent))
    }

    pub(super) fn is_waiting(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) -> bool {
        self.waiting.lock().unwrap().contains(&(callee, agent))
    }

    pub(super) fn hold(&self, callee: Callee) {
        self.held.send_modify(|held| {
            held.insert(callee);
        });
    }

    /// Holds `agent`'s next call of `callee` until `release`; every
    /// other call of `callee` passes.
    pub(super) fn hold_next(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) {
        self.alone.lock().unwrap().insert(callee, (agent, false));
        self.hold(callee);
    }

    pub(super) fn release(&self, callee: Callee) {
        self.alone.lock().unwrap().remove(&callee);
        self.held.send_modify(|held| {
            held.remove(&callee);
        });
    }
}

/// A call an agent took.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Took {
    Submit(InputId),
    Stop(Stop),
}

#[derive(Serialize, Deserialize)]
pub(super) struct Forward {
    pub(super) name: String,
    pub(super) args: Vec<u8>,
}

impl Action for Forward {
    type Output = Vec<u8>;
    const NAME: &'static str = "forward";
}

pub(super) async fn forward<A: Actor>(
    ctx: &Ctx<A>,
    forward: Forward,
) -> anyhow::Result<Vec<u8>> {
    HostRef::of(ctx.inner())?
        .call(
            &address::Realm.address()?,
            &forward.name,
            forward.args,
            DEADLINE,
        )
        .await
}

pub(super) fn seen<A: Actor>(
    ctx: &Ctx<A>,
) -> anyhow::Result<Arc<Seen>> {
    HostRef::of(ctx.inner())?
        .value::<Seen>()
        .context("the host holds what the stand-ins saw")
}

/// The key of the stand-in's address, `kind/key`, as a number.
pub(super) fn key<A: Actor, N: FromStr>(
    ctx: &Ctx<A>,
) -> anyhow::Result<N>
where
    N::Err: std::error::Error + Send + Sync + 'static,
{
    let id = ctx.actor_id();
    let (_, key) =
        id.split_once('/').context("an actor id is kind/key")?;
    Ok(key.parse()?)
}

pub(super) struct AgentStandIn {
    seen: Arc<Seen>,
    agent: AgentNumber,
}

#[async_trait::async_trait]
impl Actor for AgentStandIn {
    /// `None` for an agent a test made to call as.
    type Input = Option<AgentInput>;
    type Actions = (
        Submit,
        Stop,
        Place,
        Fill,
        Copies,
        Retire,
        Erase,
        ReadInbox,
        TakeInbox,
        DropInbox,
        OnTask,
        Forward,
    );
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            seen: seen(ctx)?,
            agent: AgentNumber(key(ctx)?),
        })
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> anyhow::Result<()> {
        self.seen.called(Callee::Create, self.agent).await?;
        if let Some(input) = ctx.input()? {
            self.seen.made.lock().unwrap().push((self.agent, input));
        }
        Ok(())
    }
}

impl Handler<Submit> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        submit: Submit,
    ) -> anyhow::Result<()> {
        self.seen.called(Callee::Submit, self.agent).await?;
        self.seen
            .took
            .lock()
            .unwrap()
            .push((self.agent, Took::Submit(submit.id.clone())));
        self.seen.submits.lock().unwrap().push((self.agent, submit));
        Ok(())
    }
}

impl Handler<Stop> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        stop: Stop,
    ) -> anyhow::Result<Stopped> {
        self.seen.called(Callee::Stop, self.agent).await?;
        self.seen
            .took
            .lock()
            .unwrap()
            .push((self.agent, Took::Stop(stop)));
        let turns = self.seen.turns.lock().unwrap();
        Ok(Stopped {
            turns: turns.get(&self.agent).copied().unwrap_or(0),
        })
    }
}

impl Handler<Place> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        place: Place,
    ) -> anyhow::Result<()> {
        self.seen.called(Callee::Place, self.agent).await?;
        self.seen.places.lock().unwrap().push((self.agent, place));
        Ok(())
    }
}

impl Handler<Fill> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        fill: Fill,
    ) -> anyhow::Result<()> {
        self.seen.called(Callee::Fill, self.agent).await?;
        self.seen.fills.lock().unwrap().push((self.agent, fill));
        Ok(())
    }
}

/// The tip of the branch of the hire `agent`, once retired.
pub(super) fn retired(agent: AgentNumber) -> CommitId {
    CommitId(Sha1([agent.0.to_le_bytes()[0]; 20]))
}

impl Handler<Retire> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Retire,
    ) -> anyhow::Result<Retired> {
        self.seen.called(Callee::Retire, self.agent).await?;
        self.seen.retires.lock().unwrap().push(self.agent);
        keep_once(
            &mut self.seen.kept.lock().unwrap().retires,
            self.agent,
        );
        Ok(Retired {
            tip: retired(self.agent),
        })
    }
}

impl Handler<Erase> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Erase,
    ) -> anyhow::Result<()> {
        self.seen.called(Callee::Erase, self.agent).await?;
        self.seen.erases.lock().unwrap().push(self.agent);
        keep_once(
            &mut self.seen.kept.lock().unwrap().erases,
            self.agent,
        );
        Ok(())
    }
}

/// Answers one file a part, so a merge reads `inbox/` in more than
/// one.
impl Handler<ReadInbox> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        read: ReadInbox,
    ) -> anyhow::Result<InboxPart> {
        self.seen.called(Callee::ReadInbox, self.agent).await?;
        let inboxes = self.seen.inboxes.lock().unwrap();
        let mut rest =
            inboxes.get(&self.agent).into_iter().flatten().filter(
                |(path, _)| {
                    read.after
                        .as_ref()
                        .is_none_or(|after| **path >= after.path)
                },
            );
        let files = rest
            .next()
            .map(|(path, (bytes, executable))| Piece {
                path: path.clone(),
                offset: 0,
                bytes: Base64(bytes.clone()),
                last: true,
                executable: *executable,
            })
            .into_iter()
            .collect();
        let next = rest.next().map(|(path, _)| InboxCursor {
            path: path.clone(),
            offset: 0,
        });
        Ok(InboxPart { files, next })
    }
}

impl Handler<TakeInbox> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        take: TakeInbox,
    ) -> anyhow::Result<()> {
        self.seen.called(Callee::TakeInbox, self.agent).await?;
        let mut inboxes = self.seen.inboxes.lock().unwrap();
        let inbox = inboxes.entry(self.agent).or_default();
        for piece in take.files {
            if piece.offset != 0 || !piece.last {
                bail!("the stand-in takes only whole files");
            }
            let path =
                format!("from-hire-{}/{}", take.from.0, piece.path);
            inbox.insert(path, (piece.bytes.0, piece.executable));
        }
        Ok(())
    }
}

impl Handler<DropInbox> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        dropped: DropInbox,
    ) -> anyhow::Result<()> {
        self.seen.called(Callee::DropInbox, self.agent).await?;
        let mut inboxes = self.seen.inboxes.lock().unwrap();
        if let Some(inbox) = inboxes.get_mut(&self.agent) {
            for path in dropped.paths {
                inbox.remove(&path);
            }
        }
        Ok(())
    }
}

impl Handler<OnTask> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        asked: OnTask,
    ) -> anyhow::Result<bool> {
        self.seen
            .asked_on_task
            .lock()
            .unwrap()
            .push((self.agent, asked));
        Ok(self.seen.on_task.lock().unwrap().contains(&self.agent))
    }
}

impl Handler<Copies> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        copies: Copies,
    ) -> anyhow::Result<()> {
        self.seen.copies.lock().unwrap().push((self.agent, copies));
        Ok(())
    }
}

impl Handler<Forward> for AgentStandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Forward,
    ) -> anyhow::Result<Vec<u8>> {
        forward(&ctx, asked).await
    }
}

/// An offer the stand-in library does not hold.
pub(super) const NO_OFFER: u32 = 404;

/// An offer of agent 9's in the stand-in library.
pub(super) const OTHERS_OFFER: u32 = 405;

/// An offer whose verdict the stand-in library refuses as judged.
pub(super) const JUDGED_OFFER: u32 = 406;

pub(super) const SKILL_MD: &str =
    "---\nname: well\ndescription: Digs a well.\n---\n";

struct LibraryStandIn {
    seen: Arc<Seen>,
}

#[async_trait::async_trait]
impl Actor for LibraryStandIn {
    type Input = ();
    type Actions = (ReadShelves, ReadOffer, Verdict, DropOfferParts);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self { seen: seen(ctx)? })
    }
}

/// The one skill on every stand-in library's shelves.
fn shelved() -> Skill {
    Skill {
        name: "dig".to_owned(),
        description: "Digs a well.".to_owned(),
        usage: "dig <depth>".to_owned(),
        shelved_by: None,
    }
}

impl Handler<ReadShelves> for LibraryStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: ReadShelves,
    ) -> anyhow::Result<ShelfList> {
        Ok(ShelfList {
            version: 1,
            skills: vec![shelved()],
        })
    }
}

impl Handler<ReadOffer> for LibraryStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        ReadOffer { offer }: ReadOffer,
    ) -> anyhow::Result<OfferRead> {
        let agent = match offer.0 {
            NO_OFFER => {
                return Ok(OfferRead::Refused {
                    why: NoOffer::NoSuchOffer { offer },
                });
            }
            OTHERS_OFFER => AgentNumber(9),
            _ => THREE,
        };
        Ok(OfferRead::Offered {
            agent,
            skill: "well".to_owned(),
            reason: "for digging".to_owned(),
            paths: vec!["SKILL.md".to_owned(), "dig.sh".to_owned()],
            skill_md: SKILL_MD.to_owned(),
        })
    }
}

impl Handler<DropOfferParts> for LibraryStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        dropped: DropOfferParts,
    ) -> anyhow::Result<()> {
        self.seen
            .called(Callee::DropOfferParts, dropped.agent)
            .await?;
        self.seen.offer_drops.lock().unwrap().push(dropped);
        Ok(())
    }
}

impl Handler<Verdict> for LibraryStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        verdict: Verdict,
    ) -> anyhow::Result<Done<VerdictRefusal>> {
        let offer = verdict.offer;
        self.seen.verdicts.lock().unwrap().push(verdict);
        self.seen
            .called(Callee::Verdict, AgentNumber(offer.0))
            .await?;
        Ok(if offer.0 == JUDGED_OFFER {
            Done::refused(VerdictRefusal::Judged { offer })
        } else {
            Done::Done
        })
    }
}

struct WorkspaceStandIn {
    seen: Arc<Seen>,
}

#[async_trait::async_trait]
impl Actor for WorkspaceStandIn {
    type Input = ();
    type Actions = (SetReach, ChiefAnswer, Forward);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self { seen: seen(ctx)? })
    }
}

impl Handler<SetReach> for WorkspaceStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        set: SetReach,
    ) -> anyhow::Result<InForce> {
        self.seen.called(Callee::SetReach, set.agent).await?;
        let mut kept = self.seen.reaches.lock().unwrap();
        let in_force = kept
            .iter()
            .filter(|reach| reach.agent == set.agent)
            .map(|reach| reach.version)
            .max();
        if let Some(version) = in_force
            && version >= set.version
        {
            return Ok(InForce { version });
        }
        let version = set.version;
        kept.push(set);
        Ok(InForce { version })
    }
}

impl Handler<ChiefAnswer> for WorkspaceStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        answer: ChiefAnswer,
    ) -> anyhow::Result<AnswerTaken> {
        self.seen.chief_answers.lock().unwrap().push(answer);
        Ok(AnswerTaken::Taken)
    }
}

impl Handler<Forward> for WorkspaceStandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Forward,
    ) -> anyhow::Result<Vec<u8>> {
        forward(&ctx, asked).await
    }
}

pub(super) fn kind<A: Actor>(name: &str) -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<A>(name, ActorConfig::default());
    registry
        .into_factories()
        .remove(name)
        .expect("the registry holds the kind it registered")
}

pub(super) async fn open(
    dir: &Path,
    seen: &Arc<Seen>,
    options: Option<&Arc<realm::Options>>,
) -> Host {
    let kinds = HashMap::from([
        (address::Realm::KIND.to_owned(), realm::factory()),
        (
            address::Agent::KIND.to_owned(),
            kind::<AgentStandIn>(address::Agent::KIND),
        ),
        (
            address::Profile::KIND.to_owned(),
            kind::<ProfileStandIn>(address::Profile::KIND),
        ),
        (
            address::Workspace::KIND.to_owned(),
            kind::<WorkspaceStandIn>(address::Workspace::KIND),
        ),
        (
            address::Library::KIND.to_owned(),
            kind::<LibraryStandIn>(address::Library::KIND),
        ),
    ]);
    let (told, mut telling) = mpsc::unbounded_channel();
    let keeping = seen.clone();
    tokio::spawn(async move {
        while let Some(told) = telling.recv().await {
            keeping.steps.lock().unwrap().push(told);
        }
    });
    let mut host_options = HostOptions::default()
        .with_value(seen.clone())
        .with_value(Arc::new(realm::StepsTold(told)));
    if let Some(options) = options {
        host_options = host_options.with_value(options.clone());
    }
    let host = Host::open_with(dir, kinds, host_options).unwrap();
    for started in [
        address::Workspace.address().unwrap(),
        address::Library.address().unwrap(),
    ] {
        host.get_or_start(&started).await.unwrap();
    }
    host
}

#[tokio::test(flavor = "multi_thread")]
async fn the_workspace_stand_in_keeps_only_a_newer_reach() {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    seen.fail_once(Callee::SetReach, AgentNumber(2));
    let host = open(&scratch, &seen, None).await;
    let set = async |version| {
        let set = SetReach {
            agent: AgentNumber(2),
            version: ReachVersion(version),
            reach: Reach::Ended,
        };
        call(&host, &address::Workspace, &set).await
    };
    assert!(set(2).await.is_err(), "it fails `set_reach` once");
    for (version, in_force) in [(2, 2), (1, 2), (3, 3)] {
        assert_eq!(
            set(version).await.unwrap().version,
            ReachVersion(in_force)
        );
    }
    let kept = seen
        .reaches
        .lock()
        .unwrap()
        .iter()
        .map(|reach| reach.version)
        .collect::<Vec<_>>();
    assert_eq!(kept, [ReachVersion(2), ReachVersion(3)]);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_and_profile_stand_ins_take_their_calls() {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    let host = open(&scratch, &seen, None).await;
    let place = Place {
        version: PlaceVersion(1),
        lead: None,
        hires: Vec::new(),
        roads: Vec::new(),
        tools_off: Vec::new(),
        folder: None,
        manual: false,
        agrees_roads: false,
    };
    let branch = Branch {
        name: "hires/2".into(),
        from: None,
    };

    call(&host, &address::Agent(AgentNumber(2)), &place)
        .await
        .unwrap();
    call(&host, &address::Profile(ProfileId(1)), &branch)
        .await
        .unwrap();

    assert_eq!(
        *seen.places.lock().unwrap(),
        [(AgentNumber(2), place)]
    );
    assert_eq!(
        *seen.branches.lock().unwrap(),
        [(ProfileId(1), branch)]
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_and_profile_stand_ins_take_the_calls_of_a_merge_and_an_erasure()
 {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    let host = open(&scratch, &seen, None).await;
    let hire = address::Agent(AgentNumber(3));
    let profile = address::Profile(ProfileId(2));
    let merge = MergeBranch {
        id: RequestId::new(),
        into: "hires/2".into(),
        absorbed: AgentNumber(3),
        message: "Took in what hire#3 learned".into(),
        from: Source::Branch {
            name: "hires/3".into(),
        },
    };
    let bundle = Bundle {
        branch: "hires/3".into(),
        after: None,
    };
    let unmerge = Unmerge {
        id: RequestId::new(),
        branch: "hires/2".into(),
        merge: retired(AgentNumber(2)),
    };
    let drop = DropBranch {
        name: "hires/3".into(),
    };

    let tip = call(&host, &hire, &Retire).await.unwrap().tip;
    call(&host, &hire, &Erase).await.unwrap();
    call(&host, &profile, &merge).await.unwrap();
    call(&host, &profile, &bundle).await.unwrap();
    call(&host, &profile, &unmerge).await.unwrap();
    call(&host, &profile, &drop).await.unwrap();
    call(
        &host,
        &profile,
        &hamlet_messages::profile::Erase {
            id: RequestId::new(),
        },
    )
    .await
    .unwrap();

    let two = ProfileId(2);
    assert_eq!(tip, retired(AgentNumber(3)));
    assert_eq!(*seen.retires.lock().unwrap(), [AgentNumber(3)]);
    assert_eq!(*seen.erases.lock().unwrap(), [AgentNumber(3)]);
    assert_eq!(*seen.merges.lock().unwrap(), [(two, merge)]);
    assert_eq!(*seen.bundles.lock().unwrap(), [(two, bundle)]);
    assert_eq!(*seen.unmerges.lock().unwrap(), [(two, unmerge)]);
    assert_eq!(*seen.branch_drops.lock().unwrap(), [(two, drop)]);
    assert_eq!(*seen.profile_erases.lock().unwrap(), [two]);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_held_reach_answers_once_released() {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    let host = open(&scratch, &seen, None).await;
    let set = SetReach {
        agent: AgentNumber(2),
        version: ReachVersion(1),
        reach: Reach::Ended,
    };
    seen.hold(Callee::SetReach);

    let answer = tokio::spawn({
        let host = host.clone();
        async move { call(&host, &address::Workspace, &set).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let held = answer.is_finished();
    seen.release(Callee::SetReach);

    assert!(!held, "a held call does not answer");
    assert_eq!(
        answer.await.unwrap().unwrap().version,
        ReachVersion(1)
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_read_opens_the_file_and_starts_the_actor_again() {
    let scratch = Scratch::new("realm-stand-ins");
    let seen = Arc::new(Seen::default());
    let host = open(&scratch, &seen, None).await;
    let workspace = address::Workspace.address().unwrap();

    let tables = stopped_open(
        &host,
        &scratch,
        &workspace,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
        |db| {
            db.query_row(
                "SELECT count(*) FROM sqlite_master",
                [],
                |row| row.get::<_, i64>(0),
            )
        },
    )
    .await
    .unwrap();

    assert!(tables >= 0);
    assert!(host.is_running(&workspace));
    host.shutdown().await.unwrap();
}
