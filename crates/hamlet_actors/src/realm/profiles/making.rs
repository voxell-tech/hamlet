//! Making a profile: the society tool an agent calls and the chief's
//! request, the profile's house or the harbor it waits at, and its
//! actor started from its character.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::{Agent, Profile};
use hamlet_messages::profile::ProfileInput;
use hamlet_messages::realm::profiles::{
    CreateProfile, CreateRefusal, Made,
};
use hamlet_messages::realm::{
    AgentRefusal, Asked, ProfileId, Refusable as _, Tool,
};
use hamlet_messages::sender::Client;
use hamlet_protocol::client::HouseAt;
use serde::{Deserialize, Serialize};

use super::super::act::{Act, Decided, Decision};
use super::super::change::Rows;
use super::super::chiefs::hamlet;
use super::super::newcomers::Asking;
use super::super::{Realm, calls, record, settings};
use super::{Arrival, Change, free_spot, houses, label, next};

fn given(character: Option<&str>) -> Option<&str> {
    character.filter(|text| !text.is_empty())
}

/// The profile `create` makes for `by` as `making` names it,
/// arriving as `arrival`, or why it is refused.
fn made(
    create: &CreateProfile,
    making: Making,
    arrival: Arrival,
    by: record::By,
) -> Result<Decided<(ProfileId, String)>, CreateRefusal> {
    let Making { profile, house } = making;
    let Ok(name) = hamlet::checked_name(&create.name) else {
        return Err(CreateRefusal::BadName {
            name: create.name.trim().to_owned(),
        });
    };
    if given(create.character.as_deref()).is_some_and(|text| {
        text.chars().count() > settings::CHARACTER_MOST
    }) {
        return Err(CreateRefusal::LongCharacter);
    }
    let name = name.to_owned();
    Ok(Decided::new((profile, name.clone())).change(Change::Made {
        profile,
        name,
        arrival,
        house,
        by,
    }))
}

/// The id a new profile takes, and the city's next free spot.
pub(in crate::realm) struct Making {
    profile: ProfileId,
    house: HouseAt,
}

impl Making {
    async fn read(rows: &Rows<'_>) -> Result<Self> {
        Ok(Self {
            profile: next(rows).await?,
            house: free_spot(&houses(rows, None).await?)?,
        })
    }
}

/// A profile an agent other than the Hand makes waits at the harbor
/// for a chief or the Hand to let it in, unless the realm lets
/// newcomers in.
impl Act<Agent> for CreateProfile {
    type Output = Made<AgentRefusal<CreateRefusal>>;
    type Loaded = (Making, Asking);
    type Done = (ProfileId, String);

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        let making = Making::read(rows).await?;
        let settings = settings::read(rows).await?;
        let character = given(self.character.as_deref())
            .unwrap_or(&settings.character);
        let asking = Asking::read(
            rows,
            by.0,
            settings.newcomers,
            making.profile,
            character,
        )
        .await?;
        Ok((making, asking))
    }

    fn decide(
        &self,
        by: &Agent,
        (making, asking): Self::Loaded,
    ) -> Decision<Self::Done, Self::Output> {
        let arrival = asking.arrival();
        match made(self, making, arrival, record::By::Agent(by.0)) {
            Ok(decided) => {
                let (profile, name) = &decided.done;
                let asked = asking.changes(
                    by.0,
                    *profile,
                    label(*profile, name),
                );
                Decision::Change(decided.changes(asked))
            }
            Err(why) => Decision::Answer(Made::refused(
                AgentRefusal::Act(why),
            )),
        }
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

impl Act<Client> for CreateProfile {
    type Output = Made<CreateRefusal>;
    type Loaded = Making;
    type Done = (ProfileId, String);

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Making> {
        Making::read(rows).await
    }

    fn decide(
        &self,
        _by: &Client,
        making: Making,
    ) -> Decision<Self::Done, Self::Output> {
        match made(self, making, Arrival::Housed, record::By::Chief) {
            Ok(decided) => Decision::Change(decided),
            Err(why) => Decision::Answer(Made::refused(why)),
        }
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
pub(in crate::realm) struct Create(pub Tool<CreateProfile>);

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
        let character = tool.act.character.clone();
        let _making = self.making.read().await;
        let made = self.clone().handle(ctx.clone(), tool).await?;
        if let Made::Made { profile, .. } = &made {
            self.start_profile(&ctx, *profile, character).await?;
        }
        Ok(made)
    }
}

/// `chief_create_profile`, which makes the profile's actor from its
/// character before it answers.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::realm) struct ChiefCreate(pub Asked<CreateProfile>);

impl Action for ChiefCreate {
    type Output = <Asked<CreateProfile> as Action>::Output;
    const NAME: &'static str = <Asked<CreateProfile> as Action>::NAME;
}

impl Message for ChiefCreate {
    type To = <Asked<CreateProfile> as Message>::To;
    type From = <Asked<CreateProfile> as Message>::From;
}

impl Handler<ChiefCreate> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        ChiefCreate(asked): ChiefCreate,
    ) -> Result<<ChiefCreate as Action>::Output> {
        let character = asked.act.character.clone();
        let _making = self.making.read().await;
        let made = self.clone().handle(ctx.clone(), asked).await?;
        if let Made::Made { profile, .. } = &made {
            self.start_profile(&ctx, *profile, character).await?;
        }
        Ok(made)
    }
}

impl Realm {
    /// Makes the actor of the new `profile` from `character`, or from
    /// the realm's character when none is given.
    async fn start_profile(
        &self,
        ctx: &Ctx<Self>,
        profile: ProfileId,
        character: Option<String>,
    ) -> Result<()> {
        let text = match given(character.as_deref()) {
            Some(text) => text.to_owned(),
            None => {
                settings::read(&Rows::outside(ctx)?).await?.character
            }
        };
        self.peers
            .create(
                &Profile(profile),
                &ProfileInput::Character { text },
            )
            .await
    }
}
