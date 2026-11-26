//! The `profile` table: each profile the realm hires from, with its
//! kind, its name, its life and its house in the city, and the
//! society tools that make, read and change one.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{Action, BindParam, ColumnValue, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::{Agent, Library, Profile};
use hamlet_messages::library::{NoSkill, Unshelve};
use hamlet_messages::profile::{
    By, CommitId, DreamOpen, Edit, NotAHire, Pin, PinAnswer,
    ProfileInput, Purge, PurgeAnswer, Read, ReadAnswer, Revert,
    RevertAnswer, Sha1, Tag, TagAnswer,
};
use hamlet_messages::realm::profiles::{
    EditProfile, EditProfileRefusal, PinHire, ProfileRefusal,
    PurgeProfile, PurgeProfileRefusal, ReadProfile,
    ReadProfileRefusal, RevertProfile, RevertProfileRefusal,
    TagProfile, TagProfileRefusal, UnshelveSkill,
};
use hamlet_messages::realm::{
    AgentRefusal, Asked, ChiefRequest, Done, Gate, ProfileId,
    Refusable, Tool, ToolCall,
};
use hamlet_messages::sender::{Client, Hand, sender};
use hamlet_messages::workspace::{AgentNumber, CallId};
use hamlet_protocol::client::{HouseAt, RequestId};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::act::Carried;
use super::callers::{self, ByHand, Relay};
use super::change::{self, Affected, Column as _, Rows, Writer};
use super::peer::When;
use super::steps::Cx;
use super::world::Part;
use super::{Realm, dreams, hire, record, settings, tree};

pub(super) mod lifecycle;
mod making;

pub(super) use making::{ChiefCreate, Create};

const HAND_CHARACTER: &str =
    include_str!("../../../hamlet_server/assets/hand.md");

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Hand,
    /// A blank hire's own, made for it alone (H38), from the realm's
    /// character: an agent's, as today
    /// (`crates/hamlet_server/src/calls/society.rs:641-648`,
    /// `:906-909`).
    Blank,
    /// A blank hire's own with no character: a chief's, as today
    /// (`crates/hamlet_server/src/commands.rs:544-547`,
    /// `:1427-1430`).
    Empty,
    /// One any hire may be made of.
    Shared,
}

impl Kind {
    /// The kind of the blank profile `by` makes for a hire.
    pub fn blank_by(by: &record::By) -> Self {
        match by {
            record::By::Chief => Self::Empty,
            record::By::Agent(_) => Self::Blank,
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    /// A fork whose actor is not made yet.
    Making,
    Living,
    Archived,
    Erased,
}

/// Where a profile is in the city.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Arrival {
    Harbor,
    Housed,
    Turned,
}

pub(super) struct Row {
    pub kind: Kind,
    pub name: String,
    pub status: Status,
    pub arrival: Arrival,
}

pub(super) async fn row(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<Option<Row>> {
    let Some((kind, name, status, arrival)) = rows
        .row::<(String, String, String, String)>(
            "SELECT kind, name, status, arrival FROM profile WHERE \
             id = ?1",
            vec![id(profile)],
        )
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(Row {
        kind: change::from_case_name(kind)?,
        name,
        status: change::from_case_name(status)?,
        arrival: change::from_case_name(arrival)?,
    }))
}

pub(super) async fn is_living(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<bool> {
    Ok(row(rows, profile)
        .await?
        .is_some_and(|kept| kept.status == Status::Living))
}

fn id(profile: ProfileId) -> BindParam {
    BindParam::Integer(i64::from(profile.0))
}

pub(super) fn address(profile: ProfileId) -> String {
    format!("scholar#{}", profile.0)
}

pub(super) fn label(profile: ProfileId, name: &str) -> String {
    format!("{} ({name})", address(profile))
}

pub(super) async fn label_of(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<String> {
    Ok(label(profile, &name_of(rows, profile).await?))
}

pub(super) async fn name_of(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<String> {
    let kept = row(rows, profile).await?.with_context(|| {
        format!("profile {} has no row", profile.0)
    })?;
    Ok(kept.name)
}

/// `named`, when it is a living shared profile.
pub(super) async fn shared(
    rows: &Rows<'_>,
    named: ProfileId,
) -> Result<Result<ProfileId, ProfileRefusal>> {
    let living = row(rows, named)
        .await?
        .filter(|kept| kept.status == Status::Living);
    Ok(match living {
        Some(Row {
            kind: Kind::Shared, ..
        }) => Ok(named),
        Some(Row {
            kind: Kind::Blank | Kind::Empty,
            name,
            ..
        }) => Err(ProfileRefusal::Apart {
            label: label(named, &name),
        }),
        Some(Row {
            kind: Kind::Hand, ..
        })
        | None => {
            Err(ProfileRefusal::NoSuchProfile { profile: named })
        }
    })
}

/// The id the next profile made in this transaction takes.
pub(super) async fn next(rows: &Rows<'_>) -> Result<ProfileId> {
    Ok(ProfileId(u32::try_from(rows.next_id("profile").await?)?))
}

/// Keeps `profile`, which `by` makes now.
pub(super) async fn insert(
    writer: &Writer<'_>,
    profile: ProfileId,
    kind: Kind,
    name: &str,
    by: &record::By,
) -> Result<()> {
    writer
        .execute(
            "INSERT INTO profile (id, kind, name, since, \
             arrival_by) VALUES (?1, ?2, ?3, ?4, ?5)",
            vec![
                id(profile),
                BindParam::Text(change::case_name(&kind)?),
                BindParam::Text(name.to_owned()),
                BindParam::Integer(writer.at().0),
                BindParam::Text(by.name()),
            ],
        )
        .await?;
    Ok(())
}

/// Makes `profile`'s actor from its character, or starts a shared
/// one, which was made with its own. A profile made already counts
/// as made.
pub(super) async fn start(
    cx: &Cx<'_>,
    profile: ProfileId,
) -> Result<()> {
    let rows = cx.rows()?;
    let kept = row(&rows, profile).await?.with_context(|| {
        format!("profile {} has no row", profile.0)
    })?;
    let text = match kept.kind {
        Kind::Shared => {
            drop(cx.realm.making.write().await);
            return cx.peers().get_or_start(&Profile(profile)).await;
        }
        Kind::Hand => HAND_CHARACTER.to_owned(),
        Kind::Blank => settings::read(&rows).await?.character,
        Kind::Empty => String::new(),
    };
    cx.peers()
        .create(&Profile(profile), &ProfileInput::Character { text })
        .await
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// A shared profile named `name` that `by` makes, arriving as
    /// `arrival`, housed at `house` when it is housed.
    Made {
        profile: ProfileId,
        name: String,
        arrival: Arrival,
        house: HouseAt,
        by: record::By,
    },
    /// A shared profile named `name`, making from `from`'s commit
    /// `at`, housed at `house`.
    Fork {
        profile: ProfileId,
        name: String,
        from: ProfileId,
        at: CommitId,
        house: HouseAt,
    },
    /// The profile is `to`, when it was `from`.
    Status {
        profile: ProfileId,
        from: Status,
        to: Status,
    },
    /// The profile is named `name`.
    Named { profile: ProfileId, name: String },
    /// `by` archived the profile now, for `why`.
    Left {
        profile: ProfileId,
        by: record::By,
        why: Option<String>,
    },
    /// `by` places the profile as `arrival`, its house at `house` or
    /// none.
    Placed {
        profile: ProfileId,
        arrival: Arrival,
        house: Option<HouseAt>,
        by: record::By,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Made {
            profile,
            name,
            arrival,
            house,
            by,
        } => {
            insert(writer, profile, Kind::Shared, &name, &by).await?;
            let house = (arrival == Arrival::Housed).then_some(house);
            place(writer, profile, arrival, house, &by).await?;
        }
        Change::Fork {
            profile,
            name,
            from,
            at,
            house,
        } => {
            writer
                .execute(
                    "INSERT INTO profile (id, kind, name, status, \
                     fork_of, fork_at, since) VALUES (?1, ?2, ?3, \
                     ?4, ?5, ?6, ?7)",
                    vec![
                        id(profile),
                        BindParam::Text(change::case_name(
                            &Kind::Shared,
                        )?),
                        BindParam::Text(name),
                        BindParam::Text(change::case_name(
                            &Status::Making,
                        )?),
                        id(from),
                        BindParam::Text(at.0.to_string()),
                        BindParam::Integer(writer.at().0),
                    ],
                )
                .await?;
            place(
                writer,
                profile,
                Arrival::Housed,
                Some(house),
                &record::By::Chief,
            )
            .await?;
        }
        Change::Status { profile, from, to } => {
            writer
                .execute(
                    "UPDATE profile SET status = ?3 WHERE id = ?1 \
                     AND status = ?2",
                    vec![
                        id(profile),
                        BindParam::Text(change::case_name(&from)?),
                        BindParam::Text(change::case_name(&to)?),
                    ],
                )
                .await?;
        }
        Change::Named { profile, name } => {
            writer
                .execute(
                    "UPDATE profile SET name = ?2 WHERE id = ?1",
                    vec![id(profile), BindParam::Text(name)],
                )
                .await?;
        }
        Change::Left { profile, by, why } => {
            writer
                .execute(
                    "UPDATE profile SET left_at = ?2, left_by = ?3, \
                     left_reason = ?4 WHERE id = ?1",
                    vec![
                        id(profile),
                        BindParam::Integer(writer.at().0),
                        BindParam::Text(by.name()),
                        why.map_or(BindParam::Null, BindParam::Text),
                    ],
                )
                .await?;
        }
        Change::Placed {
            profile,
            arrival,
            house,
            by,
        } => place(writer, profile, arrival, house, &by).await?,
    }
    Ok(Affected::seen([Part::State]))
}

async fn place(
    writer: &Writer<'_>,
    profile: ProfileId,
    arrival: Arrival,
    house: Option<HouseAt>,
    by: &record::By,
) -> Result<()> {
    let coordinate = |of: fn(&HouseAt) -> f64| {
        house.as_ref().map_or(BindParam::Null, |house| {
            BindParam::Float(of(house))
        })
    };
    writer
        .execute(
            "UPDATE profile SET arrival = ?2, house_x = ?3, house_z \
             = ?4, house_turn = ?5, arrival_by = ?6 WHERE id = ?1",
            vec![
                id(profile),
                BindParam::Text(change::case_name(&arrival)?),
                coordinate(|house| house.x),
                coordinate(|house| house.z),
                coordinate(|house| house.turn),
                BindParam::Text(by.name()),
            ],
        )
        .await?;
    Ok(())
}

/// The profile and the commit the fork `profile` is made from.
pub(super) async fn fork_of(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<(ProfileId, CommitId)> {
    let (from, at) = rows
        .row::<(u32, String)>(
            "SELECT fork_of, fork_at FROM profile WHERE id = ?1",
            vec![id(profile)],
        )
        .await?
        .with_context(|| {
            format!("profile {} is no fork", profile.0)
        })?;
    let at = Sha1::from_hex(&at).with_context(|| {
        format!("profile {}'s fork commit", profile.0)
    })?;
    Ok((ProfileId(from), CommitId(at)))
}

/// The spots of every house that stands in the city, an archived
/// profile's included, but `but`'s own.
pub(super) async fn houses(
    rows: &Rows<'_>,
    but: Option<ProfileId>,
) -> Result<Vec<(f64, f64)>> {
    let mut houses = Vec::new();
    for row in rows
        .select(
            "SELECT id, house_x, house_z FROM profile WHERE status \
             != 'erased' AND arrival = 'housed' AND house_x IS NOT \
             NULL AND house_z IS NOT NULL",
            Vec::new(),
        )
        .await?
    {
        let [id, ColumnValue::Float(x), ColumnValue::Float(z)] =
            &row[..]
        else {
            bail!("a house's row of another form: {row:?}");
        };
        if Some(ProfileId(u32::of(id)?)) != but {
            houses.push((*x, *z));
        }
    }
    Ok(houses)
}

/// The house at `x, z`, its door turned `turn`, else toward the
/// city's middle.
pub(super) fn house(x: f64, z: f64, turn: Option<f64>) -> HouseAt {
    HouseAt {
        x,
        z,
        turn: turn.unwrap_or_else(|| (-x).atan2(-z)),
    }
}

/// The first spot on a golden-angle spiral out from the city's middle
/// that keeps clear of every house in `taken`.
pub(super) fn free_spot(taken: &[(f64, f64)]) -> Result<HouseAt> {
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    let first = HouseAt::MIDDLE + HouseAt::APART * 0.5;
    let ground_per_spot =
        HouseAt::APART * HouseAt::APART * 1.1 / std::f64::consts::PI;
    for k in 0..100_000_u32 {
        let k = f64::from(k);
        let radius = (first * first + k * ground_per_spot).sqrt();
        let (x, z) = (
            radius * (k * golden).cos(),
            radius * (k * golden).sin(),
        );
        if HouseAt::is_clear(taken.iter().copied(), x, z) {
            return Ok(house(x, z, None));
        }
    }
    bail!("the city has no free spot left")
}

impl Relay<Hand> for ByHand<Tool<ReadProfile>> {
    type Call = Read;
    type Output = ReadAnswer<AgentRefusal<ReadProfileRefusal>>;

    async fn ask(
        &self,
        _by: &Hand,
        rows: &Rows<'_>,
    ) -> Result<Result<(ProfileId, Read), Self::Output>> {
        let asked = &self.0.act;
        Ok(match shared(rows, asked.profile).await? {
            Ok(profile) => Ok((
                profile,
                Read {
                    at: asked.at.clone(),
                    path: asked
                        .path
                        .clone()
                        .filter(|path| !path.is_empty()),
                },
            )),
            Err(why) => Err(ReadAnswer::refused(AgentRefusal::Act(
                why.into(),
            ))),
        })
    }

    fn answer(answer: ReadAnswer) -> Self::Output {
        match answer {
            ReadAnswer::Listed { paths } => {
                ReadAnswer::Listed { paths }
            }
            ReadAnswer::Text { text, cut } => {
                ReadAnswer::Text { text, cut }
            }
            ReadAnswer::Refused { why } => {
                ReadAnswer::refused(AgentRefusal::Act(why.into()))
            }
        }
    }
}

/// The realm's one chief, as a profile's history names it.
const CHIEF: u32 = 1;

/// A change to a profile: a chief asks for it, and an agent's call
/// of it waits for a chief to allow it.
pub(super) trait ProfileChange:
    ChiefRequest<Answer: Refusable<Why: From<ProfileRefusal>>>
    + ToolCall<Answer: Refusable<Why: From<Gate>>>
    + Clone
{
    type Call: Message<To = Profile>;

    fn profile(&self) -> ProfileId;

    /// The profile's call, asked under `id` for `by`.
    fn call(&self, id: RequestId, by: By) -> Self::Call;

    /// A chief's answer, from the profile's.
    fn answered(
        answer: <Self::Call as Action>::Output,
    ) -> <Self as ChiefRequest>::Answer;

    /// The words an agent is told of `answer` to its allowed call.
    fn told(&self, answer: <Self as ChiefRequest>::Answer) -> String;
}

/// `change` as the profile it names is asked it, under `id` for
/// `by`, or the answer when the profile is not one to change.
async fn asked<T: ProfileChange>(
    rows: &Rows<'_>,
    change: &T,
    id: RequestId,
    by: By,
) -> Result<Result<(ProfileId, T::Call), <T as ChiefRequest>::Answer>>
{
    Ok(shared(rows, change.profile())
        .await?
        .map(|profile| (profile, change.call(id, by)))
        .map_err(|why| Refusable::refused(why.into())))
}

/// The id `agent`'s call `call` is asked under: the same for each
/// repeat of the call.
pub(super) fn request_id(
    agent: AgentNumber,
    call: &CallId,
) -> Result<RequestId> {
    id_of(&format!("agent/{}/call/{}", agent.0, call.0))
}

/// The id a call is asked under, the same for each `text`.
pub(super) fn id_of(text: &str) -> Result<RequestId> {
    let digest = Sha256::digest(text);
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    let id = uuid::Builder::from_custom_bytes(bytes).into_uuid();
    Ok(RequestId::try_from(id.to_string())?)
}

impl<T: ProfileChange> Relay<Client> for Asked<T> {
    type Call = T::Call;
    type Output = <T as ChiefRequest>::Answer;

    async fn ask(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Result<(ProfileId, T::Call), Self::Output>> {
        let by = By::Chief { chief: CHIEF };
        asked(rows, &self.act, self.id.clone(), by).await
    }

    fn answer(answer: <T::Call as Action>::Output) -> Self::Output {
        T::answered(answer)
    }
}

impl Relay<Client> for Asked<PurgeProfile> {
    type Call = Purge;
    type Output = PurgeAnswer<PurgeProfileRefusal>;

    async fn ask(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Result<(ProfileId, Purge), Self::Output>> {
        let profile = match shared(rows, self.act.profile).await? {
            Ok(profile) => profile,
            Err(why) => {
                return Ok(Err(PurgeAnswer::refused(why.into())));
            }
        };
        if let Some((dream, _)) = dreams::open(rows)
            .await?
            .filter(|&(_, over)| over == profile)
        {
            let why = DreamOpen::Dreaming { dream };
            return Ok(Err(PurgeAnswer::refused(why.into())));
        }
        Ok(Ok((
            profile,
            Purge {
                id: self.id.clone(),
                path: self.act.path.clone(),
                by: By::Chief { chief: CHIEF },
            },
        )))
    }

    fn answer(answer: PurgeAnswer) -> Self::Output {
        match answer {
            PurgeAnswer::Purged {
                branches,
                still_hold,
            } => PurgeAnswer::Purged {
                branches,
                still_hold,
            },
            PurgeAnswer::Refused { why } => {
                PurgeAnswer::refused(why.into())
            }
        }
    }
}

/// A chief's purge, answered from the profile's answer once it has
/// asked every hire to remove the path. It is refused while a dream
/// of the profile is open, and no dream's folders are checked out of
/// the profile while it is in flight.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Purging(pub Asked<PurgeProfile>);

impl Action for Purging {
    type Output = <Asked<PurgeProfile> as Action>::Output;
    const NAME: &'static str = <Asked<PurgeProfile> as Action>::NAME;
}

impl Message for Purging {
    type To = <Asked<PurgeProfile> as Message>::To;
    type From = <Asked<PurgeProfile> as Message>::From;
}

impl Handler<Purging> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Purging(asked): Purging,
    ) -> Result<<Purging as Action>::Output> {
        let by = sender(&ctx, &asked)?;
        // Fails only while a dream's folders are checked out of the
        // profile, a dream `ask` refuses as open.
        let _purging =
            self.purging(asked.act.profile).try_read_owned().ok();
        callers::relay(&self, &ctx, &by, &asked, When::OverAgents)
            .await
    }
}

impl Handler<Asked<PinHire>> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Asked<PinHire>,
    ) -> Result<PinAnswer> {
        let Client = sender(&ctx, &asked)?;
        let hire = asked.act.hire;
        self.drives.ended(hire::row(hire), self.answer_within).await;
        let Some(profile) = Rows::outside(&ctx)?
            .value::<u32>(
                "SELECT hire.profile FROM hire JOIN agent ON \
                 agent.id = hire.agent WHERE hire.agent = ?1 AND \
                 agent.status = 'live'",
                vec![change::number(hire)],
            )
            .await?
        else {
            return Ok(PinAnswer::refused(
                NotAHire::NoHire { hire }.into(),
            ));
        };
        let pin = Pin {
            id: asked.id.clone(),
            hire,
            to: asked.act.to.clone(),
            by: By::Chief { chief: CHIEF },
        };
        let answer = self
            .peers
            .call(
                &Profile(ProfileId(profile)),
                &pin,
                When::BeforeAnswer,
            )
            .await?;
        let version = match &answer {
            PinAnswer::Pinned { version, .. } => {
                Some(version.clone())
            }
            PinAnswer::FollowsMain => None,
            PinAnswer::Refused { .. } => return Ok(answer),
        };
        change::commit(
            &self,
            &ctx,
            vec![tree::Change::Pin { hire, version }.into()],
        )
        .await?;
        Ok(answer)
    }
}

impl Handler<Asked<UnshelveSkill>> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Asked<UnshelveSkill>,
    ) -> Result<Done<NoSkill>> {
        let Client = sender(&ctx, &asked)?;
        let unshelve = Unshelve {
            id: asked.id.clone(),
            skill: asked.act.skill.clone(),
            reason: asked.act.reason.clone(),
            by: By::Chief { chief: CHIEF },
        };
        self.peers
            .call(&Library, &unshelve, When::BeforeAnswer)
            .await
    }
}

impl<T: ProfileChange> Relay<Agent> for Carried<T> {
    type Call = T::Call;
    type Output = <T as ChiefRequest>::Answer;

    async fn ask(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Result<(ProfileId, T::Call), Self::Output>> {
        let id = request_id(by.0, &self.called.call)?;
        asked(rows, &self.act, id, By::Agent { agent: by.0 }).await
    }

    fn answer(answer: <T::Call as Action>::Output) -> Self::Output {
        T::answered(answer)
    }
}

/// Asks the profile for the change `by`'s call `asking` names, which
/// a chief allowed, and gives the words `by` is told of it.
pub(super) async fn carry_out<T: ProfileChange>(
    realm: &Realm,
    ctx: &Ctx<Realm>,
    by: Agent,
    asking: Carried<T>,
) -> Result<String> {
    let answer =
        callers::relay(realm, ctx, &by, &asking, When::AfterAnswer)
            .await?;
    Ok(asking.act.told(answer))
}

impl ProfileChange for EditProfile {
    type Call = Edit;

    fn profile(&self) -> ProfileId {
        self.profile
    }

    fn call(&self, id: RequestId, by: By) -> Edit {
        Edit {
            id,
            path: self.path.clone(),
            text: self.text.clone(),
            message: self.message.clone(),
            by,
        }
    }

    fn answered(
        answer: Done<hamlet_messages::profile::EditRefusal>,
    ) -> Done<EditProfileRefusal> {
        match answer {
            Done::Done => Done::Done,
            Done::Refused { why } => Done::refused(why.into()),
        }
    }

    fn told(&self, answer: Done<EditProfileRefusal>) -> String {
        match answer {
            Done::Done => format!(
                "Wrote {} of {}.",
                self.path,
                address(self.profile)
            ),
            Done::Refused { why } => why.to_string(),
        }
    }
}

impl ProfileChange for TagProfile {
    type Call = Tag;

    fn profile(&self) -> ProfileId {
        self.profile
    }

    fn call(&self, id: RequestId, by: By) -> Tag {
        Tag {
            id,
            version: self.version.clone(),
            at: self.at,
            note: self.note.clone(),
            by,
        }
    }

    fn answered(answer: TagAnswer) -> TagAnswer<TagProfileRefusal> {
        match answer {
            TagAnswer::Tagged { version, commit } => {
                TagAnswer::Tagged { version, commit }
            }
            TagAnswer::Refused { why } => {
                TagAnswer::refused(why.into())
            }
        }
    }

    fn told(&self, answer: TagAnswer<TagProfileRefusal>) -> String {
        match answer {
            TagAnswer::Tagged { version, .. } => format!(
                "Tagged {}'s main as {}.",
                address(self.profile),
                version.0
            ),
            TagAnswer::Refused { why } => why.to_string(),
        }
    }
}

impl ProfileChange for RevertProfile {
    type Call = Revert;

    fn profile(&self) -> ProfileId {
        self.profile
    }

    fn call(&self, id: RequestId, by: By) -> Revert {
        Revert {
            id,
            what: self.what.clone(),
            reason: self.reason.clone(),
            by,
        }
    }

    fn answered(
        answer: RevertAnswer,
    ) -> RevertAnswer<RevertProfileRefusal> {
        match answer {
            RevertAnswer::Reverted { commit, clashes } => {
                RevertAnswer::Reverted { commit, clashes }
            }
            RevertAnswer::Unchanged => RevertAnswer::Unchanged,
            RevertAnswer::Refused { why } => {
                RevertAnswer::refused(why.into())
            }
        }
    }

    fn told(
        &self,
        answer: RevertAnswer<RevertProfileRefusal>,
    ) -> String {
        let name = address(self.profile);
        match answer {
            RevertAnswer::Reverted { clashes, .. }
                if !clashes.is_empty() =>
            {
                let paths = clashes
                    .into_iter()
                    .map(|clash| clash.path)
                    .collect::<Vec<_>>();
                format!(
                    "Reverted the commit on {name}'s main; these \
                     files changed again since and kept their later \
                     lines: {}",
                    paths.join(", ")
                )
            }
            RevertAnswer::Reverted { .. }
            | RevertAnswer::Unchanged => {
                format!("Reverted the commit on {name}'s main.")
            }
            RevertAnswer::Refused { why } => why.to_string(),
        }
    }
}
