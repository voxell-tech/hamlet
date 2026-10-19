//! The realm as a plugin reads it and points at it: the read model of
//! `hamlet_realm_api::survey`, kept from the hamlets, the agents and
//! their robots, the selection and the sky, and a thing pointed at by
//! its name.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use hamlet_core::agents::{
    Busy, Citizen, Left, LivesIn, Mayor, Settlement,
};
use hamlet_realm_api::pick::{Focus, Gesture, Named, PointAt};
use hamlet_realm_api::survey::{
    Spotted, Survey, SurveyedAgent, SurveyedHamlet,
};
use hamlet_rules::hamlet::HOME;

use crate::WorldSystems;
use crate::generator::{Common, Robot};
use crate::hamlet::{Housed, Sited};
use crate::pick::{Selected, Thing, select};
use crate::sky::Sky;

pub fn plugin(app: &mut App) {
    app.init_resource::<Survey>()
        .add_observer(point_at)
        .add_systems(Update, keep.after(WorldSystems));
}

type Agents<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
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
    (
        Entity,
        &'static Name,
        &'static Settlement,
        Option<&'static Sited>,
    ),
>;

/// Where the things a plugin names are drawn: an agent's robot by
/// the agent's name, a hamlet's common by its id.
#[derive(SystemParam)]
pub struct Whereabouts<'w, 's> {
    agents: Query<
        'w,
        's,
        (&'static Name, &'static Housed),
        (With<Citizen>, Without<Left>),
    >,
    robots: Query<
        'w,
        's,
        (Entity, &'static Robot, &'static GlobalTransform),
    >,
    hamlets: Query<'w, 's, (&'static Settlement, &'static Sited)>,
    commons: Query<
        'w,
        's,
        (Entity, &'static Common, &'static GlobalTransform),
    >,
}

impl Whereabouts<'_, '_> {
    /// The robot of the agent `name`, and where it was last drawn.
    pub fn agent(
        &self,
        name: &str,
    ) -> Option<(Entity, &GlobalTransform)> {
        let (_, housed) = self
            .agents
            .iter()
            .find(|(named, _)| named.as_str() == name)?;
        self.robots
            .iter()
            .find(|(_, robot, _)| robot.agent == housed.0)
            .map(|(robot, _, at)| (robot, at))
    }

    /// The common of the hamlet `id`, and where it was last drawn.
    pub fn hamlet(
        &self,
        id: &str,
    ) -> Option<(Entity, &GlobalTransform)> {
        let (_, sited) = self
            .hamlets
            .iter()
            .find(|(settlement, _)| settlement.name == id)?;
        self.commons
            .iter()
            .find(|(_, common, _)| common.town == sited.0)
            .map(|(common, _, at)| (common, at))
    }
}

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
            .map(|(_, name, settlement, sited)| SurveyedHamlet {
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
            .map(|(_, name, lives, housed, busy, _)| SurveyedAgent {
                name: name.to_string(),
                hamlet: hamlets
                    .get(lives.0)
                    .map(|(.., settlement, _)| {
                        settlement.name.clone()
                    })
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

/// Whether `lives` is the first hamlet, whose mayor is the keeper.
fn first(hamlets: &Hamlets, lives: &LivesIn) -> bool {
    hamlets
        .get(lives.0)
        .is_ok_and(|(.., settlement, _)| settlement.name == HOME)
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
                agents.iter().find(|(_, _, lives, .., mayor)| {
                    *mayor && first(hamlets, lives)
                });
            ("mayor", mayor.map(|(_, name, ..)| name.to_string())?)
        }
        Thing::Resident(agent) => ("agent", name(agent)?),
        Thing::ResidentHouse(agent) => ("house", name(agent)?),
        Thing::Ruin(agent) => ("ruin", name(agent)?),
        Thing::Common(hamlet)
        | Thing::Clockwork(hamlet)
        | Thing::Workbench(hamlet) => {
            ("hamlet", hamlets.get(hamlet).ok()?.2.name.clone())
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

/// Does to the thing a plugin named what the chief's click on it
/// does.
fn point_at(
    point: On<PointAt>,
    agents: Agents,
    hamlets: Hamlets,
    mut commands: Commands,
) {
    let thing = match &point.named {
        Named::Agent(name) => agents
            .iter()
            .find(|(_, named, ..)| named.as_str() == name)
            .map(|(agent, _, lives, .., mayor)| {
                match mayor && first(&hamlets, lives) {
                    true => Thing::Keeper,
                    false => Thing::Resident(agent),
                }
            }),
        Named::Hamlet(id) => hamlets
            .iter()
            .find(|(.., settlement, _)| settlement.name == *id)
            .map(|(hamlet, ..)| Thing::Common(hamlet)),
    };
    let Some(thing) = thing else {
        return;
    };
    match point.gesture {
        Gesture::Focus | Gesture::Follow => commands.trigger(Focus {
            thing,
            stay: point.gesture == Gesture::Follow,
        }),
        Gesture::Select => {
            commands.queue(move |world: &mut World| {
                select(world, Some(thing))
            });
        }
    }
}
