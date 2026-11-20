//! The `profile` table: each profile the realm hires from, with its
//! kind and its name, and the society tools that make, read and
//! change one.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use hamlet_actor::{Action, BindParam, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::{Agent, Profile};
use hamlet_messages::profile::{
    By, DreamOpen, Edit, ProfileInput, Purge, PurgeAnswer, Read,
    ReadAnswer, Revert, RevertAnswer, Tag, TagAnswer,
};
use hamlet_messages::realm::profiles::{
    CreateProfile, CreateRefusal, EditProfile, EditProfileRefusal,
    Made, ProfileRefusal, PurgeProfile, PurgeProfileRefusal,
    ReadProfile, ReadProfileRefusal, RevertProfile,
    RevertProfileRefusal, TagProfile, TagProfileRefusal,
};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{
    AgentRefusal, Asked, ChiefRequest, Done, Gate, ProfileId,
    Refusable, Tool, ToolCall,
};
use hamlet_messages::sender::{Client, Hand, sender};
use hamlet_messages::workspace::{AgentNumber, CallId};
use hamlet_protocol::client::RequestId;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::act::{Act, Called, Carried, Decided, Decision, ToolAct};
use super::callers::{self, ByHand, Relay};
use super::change::{self, Affected, Rows, Writer};
use super::chiefs::hamlet;
use super::oversight::reserved::{self, Kept};
use super::peer::When;
use super::steps::Cx;
use super::{Realm, calls, dreams, record, settings};

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

struct Row {
    kind: Kind,
    name: String,
}

async fn row(
    rows: &Rows<'_>,
    profile: ProfileId,
) -> Result<Option<Row>> {
    let Some((kind, name)) = rows
        .row::<(String, String)>(
            "SELECT kind, name FROM profile WHERE id = ?1",
            vec![id(profile)],
        )
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(Row {
        kind: change::from_case_name(kind)?,
        name,
    }))
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

/// `named`, when it is a shared profile: one a hire may be made of,
/// and a society tool may read or change.
pub(super) async fn shared(
    rows: &Rows<'_>,
    named: ProfileId,
) -> Result<Result<ProfileId, ProfileRefusal>> {
    Ok(match row(rows, named).await? {
        Some(Row {
            kind: Kind::Shared, ..
        }) => Ok(named),
        Some(Row {
            kind: Kind::Blank | Kind::Empty,
            name,
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

pub(super) async fn insert(
    writer: &Writer<'_>,
    profile: ProfileId,
    kind: Kind,
    name: &str,
) -> Result<()> {
    writer
        .execute(
            "INSERT INTO profile (id, kind, name) VALUES (?1, ?2, \
             ?3)",
            vec![
                id(profile),
                BindParam::Text(change::case_name(&kind)?),
                BindParam::Text(name.to_owned()),
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
    /// A shared profile named `name`.
    Made { profile: ProfileId, name: String },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Made { profile, name } => {
            insert(writer, profile, Kind::Shared, &name).await?;
        }
    }
    Ok(Affected::default())
}

fn given(character: Option<&str>) -> Option<&str> {
    character.filter(|text| !text.is_empty())
}

impl Act<Agent> for CreateProfile {
    type Output = Made<AgentRefusal<CreateRefusal>>;
    type Loaded = ProfileId;
    type Done = (ProfileId, String);

    async fn load(
        &self,
        _by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<ProfileId> {
        next(rows).await
    }

    fn decide(
        &self,
        _by: &Agent,
        profile: ProfileId,
    ) -> Decision<Self::Done, Self::Output> {
        let refuse = |why| {
            Decision::Answer(Made::refused(AgentRefusal::Act(why)))
        };
        let Ok(name) = hamlet::checked_name(&self.name) else {
            return refuse(CreateRefusal::BadName {
                name: self.name.trim().to_owned(),
            });
        };
        if given(self.character.as_deref()).is_some_and(|text| {
            text.chars().count() > settings::CHARACTER_MOST
        }) {
            return refuse(CreateRefusal::LongCharacter);
        }
        let name = name.to_owned();
        Decision::Change(
            Decided::new((profile, name.clone()))
                .change(Change::Made { profile, name }),
        )
    }

    fn answer(
        &self,
        (profile, name): Self::Done,
        _made: &calls::Made,
    ) -> Self::Output {
        Made::Made {
            profile,
            label: label(profile, &name),
        }
    }
}

/// `create_profile`, which makes the profile's actor from its
/// character before it answers.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Create(pub Tool<CreateProfile>);

impl Action for Create {
    type Output = <Tool<CreateProfile> as Action>::Output;
    const NAME: &'static str = <Tool<CreateProfile> as Action>::NAME;
}

impl Message for Create {
    type To = <Tool<CreateProfile> as Message>::To;
    type From = <Tool<CreateProfile> as Message>::From;
}

impl Handler<Create> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Create(tool): Create,
    ) -> Result<<Create as Action>::Output> {
        let character =
            given(tool.act.character.as_deref()).map(str::to_owned);
        let made = self.clone().handle(ctx.clone(), tool).await?;
        if let Made::Made { profile, .. } = &made {
            let text = match character {
                Some(text) => text,
                None => {
                    settings::read(&Rows::outside(&ctx)?)
                        .await?
                        .character
                }
            };
            self.peers
                .create(
                    &Profile(*profile),
                    &ProfileInput::Character { text },
                )
                .await?;
        }
        Ok(made)
    }
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
    let digest =
        Sha256::digest(format!("agent/{}/call/{}", agent.0, call.0));
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

/// An agent's call of a change to a profile, kept for a chief.
impl<T: ProfileChange> Act<Hand> for Carried<T> {
    type Output = <T as ToolCall>::Answer;
    type Loaded = Kept;
    /// The wait that keeps it.
    type Done = WaitId;

    async fn load(
        &self,
        _by: &Hand,
        rows: &Rows<'_>,
    ) -> Result<Kept> {
        reserved::keep(
            rows,
            T::TOOL,
            Hand::NUMBER,
            &self.called,
            self,
        )
        .await
    }

    fn decide(
        &self,
        _by: &Hand,
        kept: Kept,
    ) -> Decision<WaitId, Self::Output> {
        match kept {
            Kept::Waits(kept) => Decision::Change(kept),
            Kept::Refused(why) => {
                Decision::Answer(Refusable::refused(why.into()))
            }
        }
    }

    fn answer(
        &self,
        wait: WaitId,
        _made: &calls::Made,
    ) -> Self::Output {
        Refusable::refused(Gate::Reserved { wait }.into())
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
            at: None,
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

macro_rules! asked_of_the_profile {
    ($($change:ty),+) => {$(
        impl ToolAct<Hand> for $change {
            type Run = Carried<Self>;

            fn run(self, called: &Called) -> Carried<Self> {
                Carried {
                    called: called.clone(),
                    act: self,
                }
            }
        }
    )+};
}

asked_of_the_profile!(EditProfile, TagProfile, RevertProfile);
