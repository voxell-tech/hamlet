//! The realm as a plugin reads it: the read model of
//! `hamlet_realm_api::survey`, kept from the hamlets, the agents and
//! their robots, the selection and the sky.

use bevy::prelude::*;
use hamlet_core::agents::{
    Busy, Citizen, Left, LivesIn, Mayor, Settlement,
};
use hamlet_realm_api::survey::{
    Spotted, Survey, SurveyedAgent, SurveyedHamlet,
};
use hamlet_rules::hamlet::HOME;

use crate::WorldSystems;
use crate::generator::{Common, Robot};
use crate::hamlet::{Housed, Sited};
use crate::pick::{Selected, Thing};
use crate::sky::Sky;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Survey>()
        .add_systems(Update, keep.after(WorldSystems));
}

type Agents<'w, 's> = Query<
    'w,
    's,
    (
        &'static Name,
        &'static LivesIn,
        Option<&'static Housed>,
        Has<Busy>,
        Has<Mayor>,
    ),
    (With<Citizen>, Without<Left>),
>;

type Hamlets<'w, 's> = Query<
    'w,
    's,
    (&'static Name, &'static Settlement, Option<&'static Sited>),
>;

/// Writes the survey when it differs from the realm as it is.
#[allow(clippy::too_many_arguments)]
fn keep(
    agents: Agents,
    hamlets: Hamlets,
    robots: Query<(&Robot, &GlobalTransform)>,
    commons: Query<(&Common, &GlobalTransform)>,
    names: Query<&Name>,
    selected: Res<Selected>,
    sky: Res<Sky>,
    mut survey: ResMut<Survey>,
) {
    let mut now = Survey {
        hamlets: hamlets
            .iter()
            .map(|(name, settlement, sited)| SurveyedHamlet {
                id: settlement.name.clone(),
                name: name.to_string(),
                at: sited.and_then(|sited| {
                    commons
                        .iter()
                        .find(|(common, _)| common.town == sited.0)
                        .map(|(_, at)| rounded(at.translation()))
                }),
            })
            .collect(),
        agents: agents
            .iter()
            .map(|(name, lives, housed, busy, _)| SurveyedAgent {
                name: name.to_string(),
                hamlet: hamlets
                    .get(lives.0)
                    .map(|(_, settlement, _)| settlement.name.clone())
                    .unwrap_or_default(),
                at: housed.and_then(|housed| {
                    robots
                        .iter()
                        .find(|(robot, _)| robot.agent == housed.0)
                        .map(|(_, at)| rounded(at.translation()))
                }),
                busy,
            })
            .collect(),
        selected: selected.0.and_then(|thing| {
            spotted(thing, &agents, &hamlets, &names)
        }),
        hour: (sky.hour * 60.0).round() / 60.0,
    };
    now.hamlets.sort_by(|a, b| a.id.cmp(&b.id));
    now.agents.sort_by(|a, b| {
        (&a.hamlet, &a.name).cmp(&(&b.hamlet, &b.name))
    });
    if *survey != now {
        *survey = now;
    }
}

/// `at`, to a tenth of a metre.
fn rounded(at: Vec3) -> Vec3 {
    (at * 10.0).round() / 10.0
}

/// The selected `thing` by its kind and the name a plugin knows it
/// by.
fn spotted(
    thing: Thing,
    agents: &Agents,
    hamlets: &Hamlets,
    names: &Query<&Name>,
) -> Option<Spotted> {
    let name = |entity: Entity| {
        names.get(entity).ok().map(ToString::to_string)
    };
    let (kind, name) = match thing {
        Thing::Keeper | Thing::Cottage => {
            let mayor =
                agents.iter().find(|(_, lives, .., mayor)| {
                    *mayor
                        && hamlets.get(lives.0).is_ok_and(
                            |(_, home, _)| home.name == HOME,
                        )
                });
            ("mayor", mayor.map(|(name, ..)| name.to_string())?)
        }
        Thing::Resident(agent) => ("agent", name(agent)?),
        Thing::ResidentHouse(agent) => ("house", name(agent)?),
        Thing::Ruin(agent) => ("ruin", name(agent)?),
        Thing::Common(hamlet)
        | Thing::Clockwork(hamlet)
        | Thing::Workbench(hamlet) => {
            ("hamlet", hamlets.get(hamlet).ok()?.1.name.clone())
        }
        Thing::Chief(chief) => ("chief", name(chief)?),
        Thing::ChiefPlace(chief) => ("hearth", name(chief)?),
        Thing::Library => ("library", "The Library".into()),
    };
    Some(Spotted {
        kind: kind.into(),
        name,
    })
}
