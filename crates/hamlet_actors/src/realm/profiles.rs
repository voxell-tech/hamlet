//! The `profile` table: each profile the realm hires from, with its
//! kind and its name, and the society tools that make and read one.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use hamlet_actor::{Action, BindParam, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::{Agent, Profile};
use hamlet_messages::profile::{ProfileInput, Read, ReadAnswer};
use hamlet_messages::realm::profiles::{
    CreateProfile, CreateRefusal, Made, ProfileRefusal, ReadProfile,
    ReadProfileRefusal,
};
use hamlet_messages::realm::{
    AgentRefusal, ProfileId, Refusable, Tool,
};
use hamlet_messages::sender::Hand;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::callers::{ByHand, Relay};
use super::change::{self, Affected, Rows, Writer};
use super::chiefs::hamlet;
use super::steps::Cx;
use super::{Realm, calls, settings};

/// The character the Hand's profile starts from.
const HAND_CHARACTER: &str =
    include_str!("../../../hamlet_server/assets/hand.md");

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    /// The Hand's own.
    Hand,
    /// A blank hire's own, made for it alone (H38).
    Blank,
    /// One any hire may be made of.
    Shared,
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

/// How a chief and an agent are shown a profile:
/// `scholar#<id> (<name>)`.
pub(super) fn label(profile: ProfileId, name: &str) -> String {
    format!("scholar#{} ({name})", profile.0)
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
            kind: Kind::Blank,
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

/// The character given to `create_profile`; empty gives none.
fn given(character: Option<&str>) -> Option<&str> {
    character.filter(|text| !text.is_empty())
}

impl Act<Agent> for CreateProfile {
    type Output = Made<AgentRefusal<CreateRefusal>>;
    type Loaded = ProfileId;
    /// The profile and its name.
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
                    path: asked.path.clone(),
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
