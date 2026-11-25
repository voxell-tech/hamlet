//! A chief's acts on a profile's life: its fork, its archive, its
//! return and its erasure, and its place in the city.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::Profile;
use hamlet_messages::profile::{
    CommitId, Erase, Missing, ProfileInput, VersionList, Versions,
};
use hamlet_messages::realm::profiles::{
    ArchiveProfile, CreateRefusal, EraseProfile, ForkProfile, Forked,
    LifeRefusal, Lifecycle, LifecycleRefusal, MoveHouse,
    ProfileRefusal, RestoreProfile, TurnAway, Welcome,
};
use hamlet_messages::realm::{
    Asked, Done, HamletId, HowEnded, ProfileId, Refusable,
};
use hamlet_messages::sender::{Client, sender};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::HouseAt;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use super::super::act::{self, Act, Decided, Decision};
use super::super::calls::{Calls, Made};
use super::super::change::{self, Rows};
use super::super::chiefs::hamlet::{HiresOf, Picked};
use super::super::chiefs::{self, hamlet};
use super::super::peer::When;
use super::super::steps::{self, Cx, StepId, Steps};
use super::super::tree::{self, Life};
use super::super::{Realm, erase, marker, record, retire};
use super::{Arrival, Kind, Status};

/// A profile of the city, not the Hand's and made, with its label.
struct Found {
    label: String,
    row: super::Row,
}

async fn found(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<Option<Found>> {
    Ok(super::row(rows, profile)
        .await?
        .filter(|row| {
            row.kind != Kind::Hand && row.status != Status::Making
        })
        .map(|row| Found {
            label: super::label(profile, &row.name),
            row,
        }))
}

/// `profile`, when it is a living profile of the city.
async fn living(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<Result<Found, ProfileRefusal>> {
    Ok(found(rows, profile)
        .await?
        .filter(|found| found.row.status == Status::Living)
        .ok_or(ProfileRefusal::NoSuchProfile { profile }))
}

/// `profile`, when it is an archived profile of the city.
fn archived(
    profile: ProfileId,
    found: Option<Found>,
) -> Result<String, LifecycleRefusal> {
    let Some(Found { label, row }) = found else {
        return Err(ProfileRefusal::NoSuchProfile { profile }.into());
    };
    match row.status {
        Status::Archived => Ok(label),
        Status::Erased => Err(LifeRefusal::Erased { label }.into()),
        Status::Living | Status::Making => {
            Err(LifeRefusal::Living { label }.into())
        }
    }
}

fn refused(why: impl Into<LifecycleRefusal>) -> Lifecycle {
    Lifecycle::refused(why.into())
}

pub(in crate::realm) struct Archiving {
    found: Result<Found, ProfileRefusal>,
    /// Each living hire, with the changes that end it.
    hires: Vec<(AgentNumber, Vec<change::Change>)>,
    has_a_mayor: bool,
}

impl Act<Client> for ArchiveProfile {
    type Output = Lifecycle;
    type Loaded = Archiving;
    /// Its label, and the hires it ended.
    type Done = (String, Vec<AgentNumber>);

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Archiving> {
        let mayors = chiefs::mayors(rows).await?;
        let mut hires = Vec::new();
        let mut has_a_mayor = false;
        for hire in hamlet::hires_where(
            rows,
            HiresOf::Profile(self.profile),
            Picked::Living,
        )
        .await?
        {
            has_a_mayor |= mayors.contains(&hire);
            let how = HowEnded::ProfileArchived;
            let changes =
                tree::ending(rows, hire, how, self.why.clone())
                    .await?;
            hires.push((hire, changes));
        }
        Ok(Archiving {
            found: living(rows, self.profile).await?,
            hires,
            has_a_mayor,
        })
    }

    fn decide(
        &self,
        _by: &Client,
        archiving: Archiving,
    ) -> Decision<Self::Done, Lifecycle> {
        let found = match archiving.found {
            Ok(found) => found,
            Err(why) => return Decision::Answer(refused(why)),
        };
        if archiving.has_a_mayor {
            return Decision::Answer(refused(LifeRefusal::TheMayor));
        }
        let mut decided = Decided::new((found.label, Vec::new()));
        for (hire, changes) in archiving.hires {
            decided.done.1.push(hire);
            decided.changes.extend(changes);
            decided = decided.record(record::Entry::Ended {
                hire,
                by: record::By::Chief,
                why: self.why.clone(),
            });
        }
        Decision::Change(
            decided
                .change(super::Change::Status {
                    profile: self.profile,
                    from: Status::Living,
                    to: Status::Archived,
                })
                .record(record::Entry::ProfileArchived {
                    by: record::By::Chief,
                    why: self.why.clone(),
                }),
        )
    }

    fn answers_after(&self, (_, hires): &Self::Done) -> Vec<StepId> {
        hires.iter().copied().map(retire::row).collect()
    }

    fn answer(
        &self,
        (label, _): Self::Done,
        _made: &Made,
    ) -> Lifecycle {
        Lifecycle::Done { label }
    }
}

pub(in crate::realm) struct Returning {
    found: Option<Found>,
    /// Each hire its archive ended in a living hamlet, with its
    /// lead.
    hires: Vec<(AgentNumber, Option<AgentNumber>)>,
    /// The first hamlet too full to take its hire back.
    full: Option<(HamletId, u32)>,
}

async fn returning(
    act: &RestoreProfile,
    rows: &Rows<'_>,
) -> Result<Returning> {
    let mut hires = Vec::new();
    let mut back = BTreeSet::new();
    let mut taken = HashMap::new();
    for hire in hamlet::hires_where(
        rows,
        HiresOf::Profile(act.profile),
        Picked::Ended,
    )
    .await?
    {
        let ended_with =
            tree::left(rows, hire).await?.is_some_and(|left| {
                left.how == HowEnded::ProfileArchived
            });
        let lineage = tree::lineage_of(rows, hire).await?;
        let Some(home) = lineage.hamlet else {
            continue;
        };
        let at_home = hamlet::status(rows, &home.id).await?
            == Some(hamlet::Status::Living);
        if !ended_with || !at_home {
            continue;
        }
        let living = match taken.get(&home.id) {
            Some(&living) => living,
            None => tree::living(rows, &home.id).await?,
        };
        if living >= home.most() {
            return Ok(Returning {
                found: found(rows, act.profile).await?,
                hires,
                full: Some((home.id.clone(), home.most())),
            });
        }
        taken.insert(home.id, living + 1);
        let lead = match lineage.agent.lead {
            Some(lead)
                if lead != hire
                    && (back.contains(&lead)
                        || tree::life(rows, lead).await?
                            == Life::Living) =>
            {
                lead
            }
            _ => home.mayor,
        };
        back.insert(hire);
        hires.push((hire, Some(lead)));
    }
    Ok(Returning {
        found: found(rows, act.profile).await?,
        hires,
        full: None,
    })
}

impl Act<Client> for RestoreProfile {
    type Output = Lifecycle;
    type Loaded = Returning;
    type Done = String;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Returning> {
        returning(self, rows).await
    }

    fn decide(
        &self,
        _by: &Client,
        returning: Returning,
    ) -> Decision<String, Lifecycle> {
        let label = match archived(self.profile, returning.found) {
            Ok(label) => label,
            Err(why) => return Decision::Answer(refused(why)),
        };
        if let Some((hamlet, most)) = returning.full {
            return Decision::Answer(refused(LifeRefusal::Full {
                hamlet,
                most,
            }));
        }
        let mut decided =
            Decided::new(label).change(super::Change::Status {
                profile: self.profile,
                from: Status::Archived,
                to: Status::Living,
            });
        for (agent, lead) in returning.hires {
            decided = decided
                .changes(tree::coming_back(agent, lead))
                .record(record::Entry::Restored {
                    hire: agent,
                    by: record::By::Chief,
                    why: self.why.clone(),
                });
        }
        Decision::Change(decided.record(
            record::Entry::ProfileRestored {
                by: record::By::Chief,
                why: self.why.clone(),
            },
        ))
    }

    fn answer(&self, label: String, _made: &Made) -> Lifecycle {
        Lifecycle::Done { label }
    }
}

/// The row that erases `profile` once its hires' branches are gone.
fn erase_row(profile: ProfileId) -> StepId {
    StepId {
        kind: steps::Kind::ProfileErase,
        n: u64::from(profile.0),
    }
}

pub(in crate::realm) struct Erasing {
    found: Option<Found>,
    /// Its hires not erased.
    hires: Vec<AgentNumber>,
}

impl Act<Client> for EraseProfile {
    type Output = Lifecycle;
    type Loaded = Erasing;
    type Done = String;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Erasing> {
        Ok(Erasing {
            found: found(rows, self.profile).await?,
            hires: hamlet::hires_where(
                rows,
                HiresOf::Profile(self.profile),
                Picked::Ended,
            )
            .await?,
        })
    }

    fn decide(
        &self,
        _by: &Client,
        erasing: Erasing,
    ) -> Decision<String, Lifecycle> {
        let label = match archived(self.profile, erasing.found) {
            Ok(label) => label,
            Err(why) => return Decision::Answer(refused(why)),
        };
        let mut decided = Decided::new(label);
        for agent in erasing.hires {
            decided = decided.change(tree::Change::Erased { agent });
        }
        Decision::Change(
            decided
                .change(super::Change::Status {
                    profile: self.profile,
                    from: Status::Archived,
                    to: Status::Erased,
                })
                .change(steps::Change::Begin {
                    row: erase_row(self.profile),
                })
                .record(record::Entry::ProfileErased {
                    by: record::By::Chief,
                }),
        )
    }

    fn answers_after(&self, _label: &String) -> Vec<StepId> {
        vec![erase_row(self.profile)]
    }

    fn answer(&self, label: String, _made: &Made) -> Lifecycle {
        Lifecycle::Done { label }
    }
}

pub(in crate::realm) struct Placing {
    found: Result<Found, ProfileRefusal>,
    /// The spots of every other house.
    taken: Vec<(f64, f64)>,
}

async fn placing(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<Placing> {
    Ok(Placing {
        found: living(rows, profile).await?,
        taken: super::houses(rows, Some(profile)).await?,
    })
}

/// A living profile of the city's own: not a blank hire's.
fn of_the_city(
    found: Result<Found, ProfileRefusal>,
) -> Result<Found, Lifecycle> {
    let found = found.map_err(refused)?;
    if found.row.kind != Kind::Shared {
        return Err(refused(ProfileRefusal::Apart {
            label: found.label,
        }));
    }
    Ok(found)
}

fn placed(
    profile: ProfileId,
    label: String,
    arrival: Arrival,
    house: Option<HouseAt>,
    entry: record::Entry,
) -> Decision<String, Lifecycle> {
    Decision::Change(
        Decided::new(label)
            .change(super::Change::Placed {
                profile,
                arrival,
                house,
            })
            .record(entry),
    )
}

impl Act<Client> for Welcome {
    type Output = Lifecycle;
    type Loaded = Placing;
    type Done = String;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Placing> {
        placing(rows, self.profile).await
    }

    fn decide(
        &self,
        _by: &Client,
        placing: Placing,
    ) -> Decision<String, Lifecycle> {
        let found = match of_the_city(placing.found) {
            Ok(found) => found,
            Err(answer) => return Decision::Answer(answer),
        };
        if found.row.arrival == Arrival::Housed {
            return Decision::Answer(refused(LifeRefusal::Housed {
                label: found.label,
            }));
        }
        let house = match super::free_spot(&placing.taken) {
            Ok(house) => house,
            Err(full) => {
                tracing::warn!("{full:#}");
                return Decision::Answer(refused(
                    LifeRefusal::Crowded { label: found.label },
                ));
            }
        };
        placed(
            self.profile,
            found.label,
            Arrival::Housed,
            Some(house),
            record::Entry::Welcomed {
                by: record::By::Chief,
                why: self.why.clone(),
            },
        )
    }

    fn answer(&self, label: String, _made: &Made) -> Lifecycle {
        Lifecycle::Done { label }
    }
}

impl Act<Client> for TurnAway {
    type Output = Lifecycle;
    type Loaded = Placing;
    type Done = String;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Placing> {
        placing(rows, self.profile).await
    }

    fn decide(
        &self,
        _by: &Client,
        placing: Placing,
    ) -> Decision<String, Lifecycle> {
        let found = match of_the_city(placing.found) {
            Ok(found) => found,
            Err(answer) => return Decision::Answer(answer),
        };
        if found.row.arrival == Arrival::Turned {
            return Decision::Answer(refused(
                LifeRefusal::TurnedAway { label: found.label },
            ));
        }
        placed(
            self.profile,
            found.label,
            Arrival::Turned,
            None,
            record::Entry::TurnedAway {
                by: record::By::Chief,
                why: self.why.clone(),
            },
        )
    }

    fn answer(&self, label: String, _made: &Made) -> Lifecycle {
        Lifecycle::Done { label }
    }
}

impl Act<Client> for MoveHouse {
    type Output = Lifecycle;
    type Loaded = Placing;
    type Done = String;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Placing> {
        placing(rows, self.profile).await
    }

    fn decide(
        &self,
        _by: &Client,
        placing: Placing,
    ) -> Decision<String, Lifecycle> {
        let found = match of_the_city(placing.found) {
            Ok(found) => found,
            Err(answer) => return Decision::Answer(answer),
        };
        let (x, z) = (self.x, self.z);
        let refusal = if found.row.arrival != Arrival::Housed {
            Some(LifeRefusal::AtHarbor {
                label: found.label.clone(),
            })
        } else if !(x.is_finite()
            && z.is_finite()
            && self.degrees.is_none_or(f64::is_finite))
        {
            Some(LifeRefusal::BadSite {
                site: format!("{x}, {z}"),
            })
        } else if !HouseAt::is_clear(placing.taken, x, z) {
            Some(LifeRefusal::Crowded {
                label: found.label.clone(),
            })
        } else {
            None
        };
        if let Some(why) = refusal {
            return Decision::Answer(refused(why));
        }
        let turn = self.degrees.map(f64::to_radians);
        placed(
            self.profile,
            found.label,
            Arrival::Housed,
            Some(super::house(x, z, turn)),
            record::Entry::HouseMoved {
                by: record::By::Chief,
            },
        )
    }

    fn answer(&self, label: String, _made: &Made) -> Lifecycle {
        Lifecycle::Done { label }
    }
}

/// A fork, its version's commit found before the realm decides it.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::realm) struct Forking<M>(pub M);

impl<M: Message> Action for Forking<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Forking<M> {
    type To = M::To;
    type From = M::From;
}

impl Handler<Forking<Asked<ForkProfile>>> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Forking(asked): Forking<Asked<ForkProfile>>,
    ) -> Result<Forked> {
        let name = <Asked<ForkProfile> as Action>::NAME;
        let by = sender(&ctx, &asked)?;
        let id = marker::Id::Request(asked.id);
        let key = marker::Key::of(&by, id, name, &asked.act)?;
        let rows = Rows::outside(&ctx)?;
        let from = asked.act.from;
        let at = if marker::find::<IgnoredAny>(&rows, &key)
            .await?
            .is_some()
            || living(&rows, from).await?.is_err()
        {
            None
        } else {
            let versions = self
                .peers
                .call(&Profile(from), &Versions, When::BeforeAnswer)
                .await?;
            match commit(&versions, asked.act.at.as_ref()) {
                Some(commit) => Some(commit),
                None => {
                    let version =
                        asked.act.at.clone().unwrap_or_else(|| {
                            hamlet_messages::profile::VersionName(
                                hamlet_git::MAIN.to_owned(),
                            )
                        });
                    return Ok(Forked::refused(
                        Missing::NoVersion {
                            profile: from,
                            version,
                        }
                        .into(),
                    ));
                }
            }
        };
        let fork = Fork {
            asked: asked.act,
            at,
        };
        act::run_keyed(&self, &ctx, by, Some(key), name, fork).await
    }
}

/// The commit of `version` in `versions`, or of `main`'s tip when
/// absent.
fn commit(
    versions: &VersionList,
    version: Option<&hamlet_messages::profile::VersionName>,
) -> Option<CommitId> {
    match version {
        Some(version) if version.0 != hamlet_git::MAIN => versions
            .versions
            .iter()
            .find(|seen| seen.version == *version)
            .map(|seen| seen.commit),
        _ => versions.commits.first().map(|seen| seen.commit),
    }
}

#[derive(Serialize)]
pub(in crate::realm) struct Fork {
    asked: ForkProfile,
    /// The version's commit, found when `from` is living and the
    /// fork was not asked before.
    at: Option<CommitId>,
}

pub(in crate::realm) struct Making {
    found: Result<Found, ProfileRefusal>,
    profile: ProfileId,
    house: HouseAt,
}

/// The row that makes the fork `profile`.
fn fork_row(profile: ProfileId) -> StepId {
    StepId {
        kind: steps::Kind::ProfileFork,
        n: u64::from(profile.0),
    }
}

impl Act<Client> for Fork {
    type Output = Forked;
    type Loaded = Making;
    /// The new profile, and the label of the one it is made from.
    type Done = (ProfileId, String);

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Making> {
        Ok(Making {
            found: living(rows, self.asked.from).await?,
            profile: super::next(rows).await?,
            house: super::free_spot(
                &super::houses(rows, None).await?,
            )?,
        })
    }

    fn decide(
        &self,
        _by: &Client,
        making: Making,
    ) -> Decision<Self::Done, Forked> {
        let found = match making.found {
            Ok(found) => found,
            Err(why) => {
                return Decision::Answer(Forked::refused(why.into()));
            }
        };
        let Ok(name) = hamlet::checked_name(&self.asked.name) else {
            return Decision::Answer(Forked::refused(
                CreateRefusal::BadName {
                    name: self.asked.name.trim().to_owned(),
                }
                .into(),
            ));
        };
        let Some(at) = self.at else {
            return Decision::Answer(Forked::refused(
                ProfileRefusal::NoSuchProfile {
                    profile: self.asked.from,
                }
                .into(),
            ));
        };
        let profile = making.profile;
        Decision::Change(
            Decided::new((profile, found.label))
                .change(super::Change::Fork {
                    profile,
                    name: name.to_owned(),
                    from: self.asked.from,
                    at,
                    house: making.house,
                })
                .change(steps::Change::Begin {
                    row: fork_row(profile),
                })
                .record(record::Entry::Forked {
                    by: record::By::Chief,
                }),
        )
    }

    fn answers_after(
        &self,
        (profile, _): &Self::Done,
    ) -> Vec<StepId> {
        vec![fork_row(*profile)]
    }

    fn answer(
        &self,
        (profile, from): Self::Done,
        _made: &Made,
    ) -> Forked {
        Forked::Forked { profile, from }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::realm) enum ForkStep {
    Create,
    Live,
}

impl Steps for ForkStep {
    const ALL: &'static [Self] = &[Self::Create, Self::Live];

    fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Live => "live",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::realm) enum EraseStep {
    Hires,
    Profile,
}

impl Steps for EraseStep {
    const ALL: &'static [Self] = &[Self::Hires, Self::Profile];

    fn name(self) -> &'static str {
        match self {
            Self::Hires => "hires",
            Self::Profile => "profile",
        }
    }
}

fn profile_of_row(n: u64) -> Result<ProfileId> {
    Ok(ProfileId(u32::try_from(n)?))
}

pub(in crate::realm) async fn fork(
    cx: &Cx<'_>,
    n: u64,
    step: ForkStep,
) -> Result<Vec<change::Change>> {
    let profile = profile_of_row(n)?;
    match step {
        ForkStep::Create => {
            let (from, at) =
                super::fork_of(&cx.rows()?, profile).await?;
            cx.peers()
                .create(
                    &Profile(profile),
                    &ProfileInput::Fork { profile: from, at },
                )
                .await?;
            Ok(Vec::new())
        }
        ForkStep::Live => Ok(vec![
            super::Change::Status {
                profile,
                from: Status::Making,
                to: Status::Living,
            }
            .into(),
        ]),
    }
}

/// A fork whose profile could not be made is erased unseen.
pub(in crate::realm) async fn fork_failed(
    _rows: &Rows<'_>,
    n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    let profile = profile_of_row(n)?;
    Ok((
        vec![
            super::Change::Status {
                profile,
                from: Status::Making,
                to: Status::Erased,
            }
            .into(),
            steps::Change::Close {
                row: fork_row(profile),
            }
            .into(),
        ],
        Calls::none(),
    ))
}

pub(in crate::realm) async fn erase(
    cx: &Cx<'_>,
    n: u64,
    step: EraseStep,
) -> Result<Vec<change::Change>> {
    let profile = profile_of_row(n)?;
    match step {
        EraseStep::Hires => {
            let hires = cx
                .rows()?
                .values::<u32>(
                    "SELECT agent FROM hire WHERE profile = ?1",
                    vec![super::id(profile)],
                )
                .await?;
            steps::after(
                cx,
                hires
                    .into_iter()
                    .map(|hire| erase::row(AgentNumber(hire))),
            )
            .await?;
        }
        EraseStep::Profile => {
            let id = super::id_of(&format!("profile/{n}/erase"))?;
            let erased = cx
                .peers()
                .call(
                    &Profile(profile),
                    &Erase { id },
                    When::AfterAnswer,
                )
                .await;
            if let Some(Done::Refused { why }) =
                retire::unless_gone(erased)?
            {
                bail!("{why}");
            }
        }
    }
    Ok(Vec::new())
}

pub(in crate::realm) async fn erase_failed(
    _rows: &Rows<'_>,
    n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    Ok((
        vec![
            steps::Change::Close {
                row: erase_row(profile_of_row(n)?),
            }
            .into(),
        ],
        Calls::none(),
    ))
}
