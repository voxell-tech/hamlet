//! The realm through a host, called as a client and as agents, with
//! stand-ins for the actors it calls.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{
    ActorConfig, ActorFactory, ActorLifecycle, ShutdownKind,
};
use hamlet_actor_error::ActorError;
use hamlet_actors::realm;
use hamlet_host::{Address, Caller, Host, HostOptions, HostRef};
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::copies::Copies;
use hamlet_messages::agent::{
    AgentInput, Fill, Input, InputId, Place, PlaceVersion, Submit,
};
use hamlet_messages::library::{
    DropOfferParts, NoOffer, OfferRead, ReadOffer, ReadShelves,
    ShelfList, Skill, Verdict, VerdictRefusal,
};
use hamlet_messages::profile::{
    BadPath, Branch, CommitId, DreamEnd, DreamOf, DropSaveParts,
    Edit, EditRefusal, EndDream, Missing, Path as FilePath,
    PrepareDream, Prepared, ProfileInput, Purge, PurgeAnswer, Read,
    ReadAnswer, Revert, RevertAnswer, Sha1, Tag, TagAnswer,
    TagRefusal, VersionList, VersionName, VersionSeen, Versions,
};
use hamlet_messages::realm::budget::{
    BudgetLimit, Charge, MaySpend, SpendRefusal,
};
use hamlet_messages::realm::client::{Answer, Say};
use hamlet_messages::realm::world::{ReachSeen, Status};
use hamlet_messages::realm::{
    AgentName, Asked, Done, DoneTo, Found, FoundAnswer, HamletId,
    Hire, HireAnswer, HireRefusal, Named, NeedsChief, ProfileId,
    Refusable as _, RoadDone, Sent, SetLimits, Tell, Tier, Tool,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::{Need, Reach};
use hamlet_messages::workspace::{
    AgentNumber, AnswerTaken, CallId, ChiefAnswer, ConversationId,
    InForce, ReachVersion, RealmTask, SetReach,
};
use hamlet_protocol::client::{
    self as protocol, AgentId, Choice, RequestId,
};
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

#[path = "realm/allowance.rs"]
mod allowance;
#[path = "realm/asking.rs"]
mod asking;
#[path = "realm/budget.rs"]
mod budget;
#[path = "realm/callers.rs"]
mod callers;
#[path = "realm/chiefs.rs"]
mod chiefs;
#[path = "realm/choices.rs"]
mod choices;
#[path = "realm/copies.rs"]
mod copies;
#[path = "realm/dream_errands.rs"]
mod dream_errands;
#[path = "realm/dreams.rs"]
mod dreams;
#[path = "realm/drop_parts.rs"]
mod drop_parts;
#[path = "realm/errands.rs"]
mod errands;
#[path = "realm/first_start.rs"]
mod first_start;
#[path = "realm/found.rs"]
mod found;
#[path = "realm/laid.rs"]
mod laid;
#[path = "realm/lead.rs"]
mod lead;
#[path = "realm/letters.rs"]
mod letters;
#[path = "realm/librarian.rs"]
mod librarian;
#[path = "realm/living.rs"]
mod living;
#[path = "realm/oversight.rs"]
mod oversight;
#[path = "realm/parcels.rs"]
mod parcels;
#[path = "realm/places.rs"]
mod places;
#[path = "realm/profiles.rs"]
mod profiles;
#[path = "realm/prompt.rs"]
mod prompt;
#[path = "realm/record.rs"]
mod record;
#[path = "realm/repeats.rs"]
mod repeats;
#[path = "realm/reserved.rs"]
mod reserved;
#[path = "realm/reviews.rs"]
mod reviews;
#[path = "realm/say.rs"]
mod say;
#[path = "realm/secrets.rs"]
mod secrets;
#[path = "realm/settings.rs"]
mod settings;
#[path = "realm/society.rs"]
mod society;
#[path = "realm/steps.rs"]
mod steps;
#[path = "realm/tasks.rs"]
mod tasks;
#[path = "realm/waits.rs"]
mod waits;
#[path = "realm/workers.rs"]
mod workers;
#[path = "realm/world.rs"]
mod world;
#[path = "realm/writers.rs"]
mod writers;

const DEADLINE: Duration = Duration::from_secs(20);

const MAYOR: AgentNumber = AgentNumber(2);

const THREE: AgentNumber = AgentNumber(3);

const FOUR: AgentNumber = AgentNumber(4);

/// A call of the realm's that a test can hold or fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Callee {
    Branch,
    Create,
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
    Fill,
    /// The workspace's measure of a parcel, by its sender.
    Parcel,
    Place,
    /// A profile's preparation of a dream, by the profile's number.
    PrepareDream,
    /// A profile's purge, by the profile's number.
    Purge,
    SetReach,
    Submit,
    /// The library's verdict, by the offer's number.
    Verdict,
}

#[derive(Default)]
struct Seen {
    made: Mutex<Vec<(AgentNumber, AgentInput)>>,
    submits: Mutex<Vec<(AgentNumber, Submit)>>,
    places: Mutex<Vec<(AgentNumber, Place)>>,
    fills: Mutex<Vec<(AgentNumber, Fill)>>,
    copies: Mutex<Vec<(AgentNumber, Copies)>>,
    reaches: Mutex<Vec<SetReach>>,
    chief_answers: Mutex<Vec<ChiefAnswer>>,
    branches: Mutex<Vec<(ProfileId, Branch)>>,
    profiles: Mutex<Vec<(ProfileId, ProfileInput)>>,
    reads: Mutex<Vec<(ProfileId, Read)>>,
    edits: Mutex<Vec<(ProfileId, Edit)>>,
    tags: Mutex<Vec<(ProfileId, Tag)>>,
    reverts: Mutex<Vec<(ProfileId, Revert)>>,
    purges: Mutex<Vec<(ProfileId, Purge)>>,
    /// Each preparation of a dream, with whether a purge was held
    /// when it came.
    prepares: Mutex<Vec<(ProfileId, PrepareDream, bool)>>,
    dream_ends: Mutex<Vec<(ProfileId, EndDream)>>,
    /// Each profile answers a dream's preparation with nothing to
    /// dream over.
    nothing_to_dream: AtomicBool,
    save_drops: Mutex<Vec<(ProfileId, DropSaveParts)>>,
    offer_drops: Mutex<Vec<DropOfferParts>>,
    /// Each verdict the library was called with, failed calls too.
    verdicts: Mutex<Vec<Verdict>>,
    failing: Mutex<HashSet<(Callee, AgentNumber)>>,
    /// The calls that fail every time, with an error of the callee's
    /// own.
    refusing: Mutex<HashSet<(Callee, AgentNumber)>>,
    held: watch::Sender<HashSet<Callee>>,
    /// The calls that wait while the test holds their callee.
    waiting: Mutex<HashSet<(Callee, AgentNumber)>>,
    /// The callees held for one agent's next call alone, with that
    /// agent and whether its call came.
    alone: Mutex<HashMap<Callee, (AgentNumber, bool)>>,
}

impl Seen {
    /// Waits while the test holds `callee`, then fails it if the
    /// test asked it to fail once for `agent`.
    async fn called(
        &self,
        callee: Callee,
        agent: AgentNumber,
    ) -> anyhow::Result<()> {
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
        if self.refusing.lock().unwrap().contains(&(callee, agent)) {
            bail!("the stand-in refuses {callee:?} of {}", agent.0);
        }
        Ok(())
    }

    /// Fails `agent`'s next call of `callee` as an actor not ready.
    fn fail_once(&self, callee: Callee, agent: AgentNumber) {
        self.failing.lock().unwrap().insert((callee, agent));
    }

    fn fail_for_good(&self, callee: Callee, agent: AgentNumber) {
        self.refusing.lock().unwrap().insert((callee, agent));
    }

    fn has_failed(&self, callee: Callee, agent: AgentNumber) -> bool {
        !self.failing.lock().unwrap().contains(&(callee, agent))
    }

    fn is_waiting(&self, callee: Callee, agent: AgentNumber) -> bool {
        self.waiting.lock().unwrap().contains(&(callee, agent))
    }

    fn hold(&self, callee: Callee) {
        self.held.send_modify(|held| {
            held.insert(callee);
        });
    }

    /// Holds `agent`'s next call of `callee` until `release`; every
    /// other call of `callee` passes.
    fn hold_next(&self, callee: Callee, agent: AgentNumber) {
        self.alone.lock().unwrap().insert(callee, (agent, false));
        self.hold(callee);
    }

    fn release(&self, callee: Callee) {
        self.alone.lock().unwrap().remove(&callee);
        self.held.send_modify(|held| {
            held.remove(&callee);
        });
    }
}

#[derive(Serialize, Deserialize)]
struct Forward {
    name: String,
    args: Vec<u8>,
}

impl Action for Forward {
    type Output = Vec<u8>;
    const NAME: &'static str = "forward";
}

async fn forward<A: Actor>(
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

fn seen<A: Actor>(ctx: &Ctx<A>) -> anyhow::Result<Arc<Seen>> {
    HostRef::of(ctx.inner())?
        .value::<Seen>()
        .context("the host holds what the stand-ins saw")
}

/// The key of the stand-in's address, `kind/key`, as a number.
fn key<A: Actor, N: FromStr>(ctx: &Ctx<A>) -> anyhow::Result<N>
where
    N::Err: std::error::Error + Send + Sync + 'static,
{
    let id = ctx.actor_id();
    let (_, key) =
        id.split_once('/').context("an actor id is kind/key")?;
    Ok(key.parse()?)
}

struct AgentStandIn {
    seen: Arc<Seen>,
    agent: AgentNumber,
}

#[async_trait::async_trait]
impl Actor for AgentStandIn {
    /// `None` for an agent a test made to call as.
    type Input = Option<AgentInput>;
    type Actions = (Submit, Place, Fill, Copies, Forward);
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
        self.seen.submits.lock().unwrap().push((self.agent, submit));
        Ok(())
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

struct ProfileStandIn {
    seen: Arc<Seen>,
    profile: ProfileId,
}

#[async_trait::async_trait]
impl Actor for ProfileStandIn {
    /// `None` for a profile a test calls without making it.
    type Input = Option<ProfileInput>;
    type Actions = (
        Branch,
        Versions,
        Read,
        Edit,
        Tag,
        Revert,
        Purge,
        PrepareDream,
        EndDream,
        DropSaveParts,
    );
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            seen: seen(ctx)?,
            profile: ProfileId(key(ctx)?),
        })
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> anyhow::Result<()> {
        if let Some(input) = ctx.input()? {
            self.seen
                .profiles
                .lock()
                .unwrap()
                .push((self.profile, input));
        }
        Ok(())
    }
}

const HELD_VERSION: &str = "v1";

impl Handler<Branch> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        branch: Branch,
    ) -> anyhow::Result<Done<Missing>> {
        let agent = branch
            .name
            .strip_prefix("hires/")
            .context("a hire's branch is hires/<n>")?
            .parse()?;
        self.seen.called(Callee::Branch, AgentNumber(agent)).await?;
        let from = branch.from.clone();
        self.seen
            .branches
            .lock()
            .unwrap()
            .push((self.profile, branch));
        Ok(match from {
            Some(version) if version.0 != HELD_VERSION => {
                Done::Refused {
                    why: Missing::NoVersion {
                        profile: self.profile,
                        version,
                    },
                }
            }
            _ => Done::Done,
        })
    }
}

impl Handler<Versions> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Versions,
    ) -> anyhow::Result<VersionList> {
        Ok(VersionList {
            versions: vec![VersionSeen {
                version: VersionName(HELD_VERSION.to_owned()),
                commit: COMMIT,
                note: String::new(),
                by: "chief:1".to_owned(),
            }],
            commits: Vec::new(),
        })
    }
}

const HELD_FILE: &str = "CHARACTER.md";

impl Handler<Read> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        read: Read,
    ) -> anyhow::Result<ReadAnswer> {
        let path = read.path.clone();
        self.seen.reads.lock().unwrap().push((self.profile, read));
        Ok(match path {
            None => ReadAnswer::Listed {
                paths: vec![FilePath {
                    path: HELD_FILE.to_owned(),
                }],
            },
            Some(path) if path == HELD_FILE => ReadAnswer::Text {
                text: format!(
                    "scholar#{}'s character",
                    self.profile.0
                ),
                cut: false,
            },
            Some(path) => ReadAnswer::Refused {
                why: BadPath::NotAPath { path }.into(),
            },
        })
    }
}

/// The commit every stand-in profile's change answers with.
const COMMIT: CommitId = CommitId(Sha1([7; 20]));

/// A path a stand-in profile refuses to write.
const NOT_A_PATH: &str = "../out";

/// An offer the stand-in library does not hold.
const NO_OFFER: u32 = 404;

/// An offer of agent 9's in the stand-in library.
const OTHERS_OFFER: u32 = 405;

/// An offer whose verdict the stand-in library refuses as judged.
const JUDGED_OFFER: u32 = 406;

const SKILL_MD: &str =
    "---\nname: well\ndescription: Digs a well.\n---\n";

impl Handler<Edit> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        edit: Edit,
    ) -> anyhow::Result<Done<EditRefusal>> {
        let path = edit.path.clone();
        self.seen.edits.lock().unwrap().push((self.profile, edit));
        self.seen
            .called(Callee::Edit, AgentNumber(self.profile.0))
            .await?;
        Ok(if path == NOT_A_PATH {
            Done::Refused {
                why: BadPath::NotAPath { path }.into(),
            }
        } else {
            Done::Done
        })
    }
}

impl Handler<Tag> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        tag: Tag,
    ) -> anyhow::Result<TagAnswer> {
        let version = tag.version.clone();
        self.seen.tags.lock().unwrap().push((self.profile, tag));
        Ok(if version.0 == HELD_VERSION {
            TagAnswer::Refused {
                why: TagRefusal::Taken { version },
            }
        } else {
            TagAnswer::Tagged {
                version,
                commit: COMMIT,
            }
        })
    }
}

impl Handler<Revert> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        revert: Revert,
    ) -> anyhow::Result<RevertAnswer> {
        self.seen
            .reverts
            .lock()
            .unwrap()
            .push((self.profile, revert));
        Ok(RevertAnswer::Reverted {
            commit: COMMIT,
            clashes: vec![FilePath {
                path: "notes.md".to_owned(),
            }],
        })
    }
}

/// The hire every stand-in profile's purge leaves holding the path.
const STILL_HOLDS: AgentNumber = AgentNumber(3);

impl Handler<Purge> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        purge: Purge,
    ) -> anyhow::Result<PurgeAnswer> {
        self.seen
            .called(Callee::Purge, AgentNumber(self.profile.0))
            .await?;
        let path = purge.path.clone();
        self.seen.purges.lock().unwrap().push((self.profile, purge));
        Ok(if path == NOT_A_PATH {
            PurgeAnswer::Refused {
                why: BadPath::NotAPath { path }.into(),
            }
        } else {
            PurgeAnswer::Purged {
                branches: 2,
                still_hold: vec![STILL_HOLDS],
            }
        })
    }
}

impl Handler<PrepareDream> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        prepare: PrepareDream,
    ) -> anyhow::Result<Prepared> {
        let purging = self
            .seen
            .waiting
            .lock()
            .unwrap()
            .iter()
            .any(|&(callee, _)| callee == Callee::Purge);
        self.seen
            .called(Callee::PrepareDream, AgentNumber(self.profile.0))
            .await?;
        let dream = prepare.dream.0;
        let of_a_hire = matches!(prepare.of, DreamOf::Hire { .. });
        self.seen.prepares.lock().unwrap().push((
            self.profile,
            prepare,
            purging,
        ));
        if self.seen.nothing_to_dream.load(Ordering::SeqCst) {
            return Ok(Prepared::Nothing);
        }
        Ok(Prepared::Ready {
            main: format!("dreams/{dream}/main"),
            hire: of_a_hire.then(|| format!("dreams/{dream}/hire")),
        })
    }
}

impl Handler<EndDream> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        end: EndDream,
    ) -> anyhow::Result<DreamEnd> {
        self.seen
            .called(Callee::EndDream, AgentNumber(self.profile.0))
            .await?;
        self.seen
            .dream_ends
            .lock()
            .unwrap()
            .push((self.profile, end));
        Ok(DreamEnd::Nothing)
    }
}

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

impl Handler<DropSaveParts> for ProfileStandIn {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        dropped: DropSaveParts,
    ) -> anyhow::Result<()> {
        self.seen
            .called(Callee::DropSaveParts, dropped.agent)
            .await?;
        self.seen
            .save_drops
            .lock()
            .unwrap()
            .push((self.profile, dropped));
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

fn kind<A: Actor>(name: &str) -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<A>(name, ActorConfig::default());
    registry
        .into_factories()
        .remove(name)
        .expect("the registry holds the kind it registered")
}

async fn open(
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
    let mut host_options =
        HostOptions::default().with_value(seen.clone());
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

struct Realm {
    host: Host,
    seen: Arc<Seen>,
    options: Option<Arc<realm::Options>>,
    scratch: Scratch,
}

impl Realm {
    async fn start() -> Self {
        Self::start_with(None, Seen::default()).await
    }

    async fn start_with(
        options: Option<realm::Options>,
        seen: Seen,
    ) -> Self {
        let scratch = Scratch::new("realm");
        let seen = Arc::new(seen);
        let options = options.map(Arc::new);
        let host = open(&scratch, &seen, options.as_ref()).await;
        host.get_or_start(&address::Realm.address().unwrap())
            .await
            .unwrap();
        Self {
            host,
            seen,
            options,
            scratch,
        }
    }

    async fn restart(self) -> Self {
        self.host.shutdown().await.unwrap();
        let host =
            open(&self.scratch, &self.seen, self.options.as_ref())
                .await;
        host.get_or_start(&address::Realm.address().unwrap())
            .await
            .unwrap();
        Self { host, ..self }
    }

    async fn ask<A: Action>(
        &self,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        let args = hamlet_actor::action::encode_positional(action)?;
        let answer = self
            .host
            .call_for_client(
                &address::Realm.address()?,
                A::NAME,
                args,
                DEADLINE,
            )
            .await?;
        decoded(&answer)
    }

    async fn tool<A: Action>(
        &self,
        as_agent: AgentNumber,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        self.call_as(&address::Agent(as_agent), action).await
    }

    async fn call_as_workspace<A: Action>(
        &self,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        self.call_as(&address::Workspace, action).await
    }

    async fn call_as<A: Action>(
        &self,
        from: &impl Party,
        action: &A,
    ) -> anyhow::Result<A::Output> {
        let forward = Forward {
            name: A::NAME.to_owned(),
            args: hamlet_actor::action::encode_positional(action)?,
        };
        decoded(&call(&self.host, from, &forward).await?)
    }

    async fn live(&self, agent: AgentNumber) {
        world::seen_until(
            self,
            &format!("agent {} is live", agent.0),
            |seen| {
                seen.agents.iter().any(|shown| {
                    shown.agent == agent
                        && shown.status == Status::Live
                        && matches!(
                            shown.reach,
                            ReachSeen::Applied { .. }
                        )
                })
            },
        )
        .await;
    }

    async fn reach_held(&self, agent: AgentNumber) {
        until(
            &format!("the workspace holds agent {}'s reach", agent.0),
            || self.holds_reach(agent),
        )
        .await;
    }

    fn holds_reach(&self, agent: AgentNumber) -> bool {
        self.seen
            .reaches
            .lock()
            .unwrap()
            .iter()
            .any(|reach| reach.agent == agent)
    }

    async fn stopped_rows<T>(
        &self,
        read: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> T {
        let realm = address::Realm.address().unwrap();
        stopped_open(
            &self.host,
            &self.scratch,
            &realm,
            OpenFlags::SQLITE_OPEN_READ_ONLY,
            read,
        )
        .await
        .unwrap()
    }

    async fn write(&self, sql: &str) {
        let realm = address::Realm.address().unwrap();
        stopped_open(
            &self.host,
            &self.scratch,
            &realm,
            OpenFlags::default(),
            |db| db.execute_batch(sql),
        )
        .await
        .unwrap();
    }
}

/// Opens the file of the actor at `at` with `flags`. The host holds
/// the file locked while the actor runs, so this stops the actor and
/// starts it again.
async fn stopped_open<T>(
    host: &Host,
    dir: &Path,
    at: &Address,
    flags: OpenFlags,
    open: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> anyhow::Result<T> {
    host.stop(at, ShutdownKind::Sleep).await?;
    let file = dir
        .join("actors")
        .join(at.kind())
        .join(format!("{}.sqlite", at.key()));
    let opened = open(&Connection::open_with_flags(file, flags)?);
    host.get_or_start(at).await?;
    Ok(opened?)
}

async fn call<A: Action>(
    host: &Host,
    to: &impl Party,
    action: &A,
) -> anyhow::Result<A::Output> {
    let to = to.address()?;
    host.get_or_start(&to).await?;
    let args = hamlet_actor::action::encode_positional(action)?;
    decoded(&host.call(&to, A::NAME, args, DEADLINE).await?)
}

async fn until(what: &str, holds: impl Fn() -> bool) {
    let begun = Instant::now();
    while !holds() {
        assert!(begun.elapsed() < DEADLINE, "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn found(name: &str) -> Found {
    Found {
        name: name.to_owned(),
        mayor: None,
        tier: Tier::Fifty,
        purpose: None,
    }
}

async fn founded(realm: &Realm) -> HamletId {
    let answer = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Mill"),
        })
        .await
        .unwrap();
    let FoundAnswer::Founded {
        hamlet,
        mayor: MAYOR,
    } = answer
    else {
        panic!("the chief founds a hamlet with mayor 2: {answer:?}");
    };
    hamlet
}

async fn hamlet_with(count: u32) -> (Realm, HamletId) {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    realm.live(MAYOR).await;
    for n in 0..count {
        hires(&realm, MAYOR, &format!("h{n}")).await;
        realm.live(AgentNumber(3 + n)).await;
    }
    (realm, hamlet)
}

fn hire(under: Option<Named>) -> Hire {
    Hire {
        hamlet: None,
        under,
        profile: None,
        version: None,
        purpose: Some("Check the links".to_owned()),
        name: None,
    }
}

fn hired<R>(agent: u32) -> HireAnswer<R> {
    HireAnswer::Hired {
        agent: shown(AgentNumber(agent)),
    }
}

async fn hires(realm: &Realm, lead: AgentNumber, call: &str) {
    let hired = realm.tool(lead, &tool(call, hire(None))).await;
    assert!(
        matches!(hired, Ok(HireAnswer::Hired { .. })),
        "{hired:?}"
    );
}

async fn chief_hires(
    realm: &Realm,
    hamlet: &HamletId,
    act: Hire,
) -> HireAnswer<HireRefusal> {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act: Hire {
                hamlet: Some(hamlet.clone()),
                ..act
            },
        })
        .await
        .unwrap()
}

fn made(realm: &Realm, agent: AgentNumber) -> Option<AgentInput> {
    realm
        .seen
        .made
        .lock()
        .unwrap()
        .iter()
        .find(|(made, _)| *made == agent)
        .map(|(_, input)| input.clone())
}

async fn made_until(realm: &Realm, agent: AgentNumber) -> AgentInput {
    until(&format!("agent {} is made", agent.0), || {
        made(realm, agent).is_some()
    })
    .await;
    made(realm, agent).unwrap()
}

fn standing(realm: &Realm, agent: AgentNumber) -> RealmTask {
    made(realm, agent).expect("the agent was made").task
}

fn charge(
    call: &str,
    task: &RealmTask,
    micros: u32,
    ms: u32,
) -> Charge {
    Charge {
        call: CallId(call.to_owned()),
        task: task.clone(),
        micros,
        took_ms: ms,
        tokens: None,
    }
}

async fn may_spend(
    realm: &Realm,
    agent: AgentNumber,
    task: &RealmTask,
) -> Done<SpendRefusal> {
    let may = MaySpend { task: task.clone() };
    realm.tool(agent, &may).await.unwrap()
}

async fn limit(
    realm: &Realm,
    hamlet: Option<&HamletId>,
    budget: Vec<BudgetLimit>,
) {
    let set = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: SetLimits {
                hamlet: hamlet.cloned(),
                budget,
                limits: Vec::new(),
            },
        })
        .await
        .unwrap();
    assert_eq!(set, Done::Done);
}

/// The words of a refusal, as the chief or the agent reads them.
fn words(answer: impl Serialize) -> String {
    let answer = serde_json::to_value(answer).unwrap();
    assert_eq!(answer["type"], "refused", "{answer}");
    answer["words"].as_str().unwrap().to_owned()
}

/// The answer of an agent's act on `agent`.
fn done_to<R>(agent: AgentNumber) -> DoneTo<R> {
    DoneTo::Done {
        agent: shown(agent),
    }
}

/// The answer of an agent's act on the road between `a` and `b`.
fn road_done<R>(a: AgentNumber, b: AgentNumber) -> RoadDone<R> {
    RoadDone::Done {
        a: shown(a),
        b: shown(b),
    }
}

/// The answer of a letter sent to `to`.
fn sent<R>(to: AgentNumber) -> Sent<R> {
    Sent::Sent { to: shown(to) }
}

/// `agent` as the realm names it to chiefs and agents.
fn shown(agent: AgentNumber) -> AgentName {
    let name = if agent == Hand::NUMBER {
        "hand".to_owned()
    } else {
        format!("hire#{}", agent.0)
    };
    AgentName { agent, name }
}

/// The copies `agent` was pushed, oldest first.
fn copies_to(realm: &Realm, agent: AgentNumber) -> Vec<Copies> {
    realm
        .seen
        .copies
        .lock()
        .unwrap()
        .iter()
        .filter(|(to, _)| *to == agent)
        .map(|(_, copies)| copies.clone())
        .collect()
}

fn submits_to(realm: &Realm, agent: AgentNumber) -> Vec<Submit> {
    realm
        .seen
        .submits
        .lock()
        .unwrap()
        .iter()
        .filter(|(to, _)| *to == agent)
        .map(|(_, submit)| submit.clone())
        .collect()
}

async fn seen_submits(
    realm: &Realm,
    agent: AgentNumber,
    count: usize,
) -> Vec<Submit> {
    until(&format!("agent {} saw {count} inputs", agent.0), || {
        submits_to(realm, agent).len() >= count
    })
    .await;
    submits_to(realm, agent)
}

async fn told_of_failed(
    realm: &Realm,
    lead: AgentNumber,
    hire: AgentNumber,
) -> String {
    let id = InputId(format!("hire/{}", hire.0));
    let told = || {
        submits_to(realm, lead)
            .into_iter()
            .find(|seen| seen.id == id)
    };
    until(
        &format!("agent {} is told of hire {}", lead.0, hire.0),
        || told().is_some(),
    )
    .await;
    match told().map(|seen| seen.input) {
        Some(Input::Notice { text, .. }) => text,
        other => panic!("not a notice: {other:?}"),
    }
}

fn to(agent: AgentNumber) -> Named {
    Named::Agent { agent }
}

fn tell(to: Named, text: &str) -> Tell {
    Tell {
        to,
        text: text.to_owned(),
        paths: Vec::new(),
    }
}

fn needs(agent: AgentNumber, call: &str) -> NeedsChief {
    NeedsChief {
        agent,
        call: CallId(call.to_owned()),
        conversation: ConversationId("c".to_owned()),
        realm_task: None,
        needs: vec![Need::Command],
        subject: "rm -rf build".to_owned(),
        change: None,
        reach_version: ReachVersion(1),
    }
}

fn answer(wait: &str, choice: Choice, text: Option<&str>) -> Answer {
    Answer(protocol::Answer {
        choice,
        id: RequestId::new(),
        picks: None,
        text: text.map(str::to_owned),
        wait: wait.to_owned(),
    })
}

fn say(id: &RequestId, to: AgentNumber, text: &str) -> Say {
    Say(protocol::Say {
        files: Vec::new(),
        id: id.clone(),
        images: Vec::new(),
        text: text.to_owned(),
        to: AgentId(u64::from(to.0)),
    })
}

async fn purpose_of(
    realm: &Realm,
    agent: AgentNumber,
) -> Option<String> {
    realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT purpose FROM hire WHERE agent = ?1",
                [agent.0],
                |row| row.get(0),
            )
        })
        .await
}

fn tool<T>(call: &str, act: T) -> Tool<T> {
    Tool {
        call: CallId(call.to_owned()),
        task: None,
        conversation: None,
        act,
    }
}

/// `act` called in a turn on `task`.
fn paid<T>(call: &str, task: &RealmTask, act: T) -> Tool<T> {
    Tool {
        task: Some(task.clone()),
        ..tool(call, act)
    }
}

fn decoded<T: DeserializeOwned>(bytes: &[u8]) -> anyhow::Result<T> {
    ciborium::from_reader(bytes).context("decode the answer")
}

fn error_code<T: std::fmt::Debug>(
    called: anyhow::Result<T>,
) -> String {
    let error = ActorError::extract(&called.unwrap_err());
    format!("{}.{}", error.group(), error.code())
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
