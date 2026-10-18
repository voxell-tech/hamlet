//! Things that wait for a chief (rule one, H10, H11): approvals,
//! objections, questions and faults, each an entity with a
//! [`Waiting`] until a chief gives its [`Answer`].

mod answer;
pub mod danger;
mod decide;
pub mod grants;
mod instruction;
mod lapse;
mod ledger;
mod plugin;
mod queue;
mod review;
mod typed;

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use hamlet_home::limits::{Limits, Permission, Review};
use hamlet_rules::chief::rulings::Hearing;
use hamlet_rules::hamlet::Hamlet;

pub use self::answer::{Answer, Answered};
use self::answer::{settle, settle_picks};
use self::danger::Words;
use self::decide::{Asked, decide};
use self::grants::Grants;
use self::instruction::{Carrying, Instruction, follow, over};
pub use self::lapse::Veto;
use self::lapse::{departed, forget, lapse, recall, stop};
use self::ledger::Ledger;
pub use self::plugin::{Settled, petition};
use self::queue::Raised;
pub use self::queue::{Asks, Kind, Need, Needs, RaisedBy, Waiting};
pub(crate) use self::typed::strip;
use crate::agent::Event as AgentEvent;
use crate::agents::{self, Citizen, Left};
use crate::core::HomeDir;
use crate::session::{
    Closing, Instructed, Overseen, Reply, Rewound, SessionOutput,
    ToolStarting, TurnDropped, TurnStarted,
};

/// Holds tool calls and instructions for a chief, and takes the
/// chief's answers.
pub struct OversightPlugin;

impl Plugin for OversightPlugin {
    fn build(&self, app: &mut App) {
        let root = app.world().resource::<HomeDir>().0.root.clone();
        app.insert_resource(Overseen)
            .insert_resource(Grants::open(&root))
            .insert_resource(Ledger::open(&root))
            .insert_resource(review::Reviews::new())
            .init_resource::<Rules>()
            .init_resource::<Raised>()
            .add_observer(
                |mut starting: On<ToolStarting>,
                 mut commands: Commands| {
                    let call = starting.event_mut();
                    if call.deny.is_some() {
                        return;
                    }
                    let Some(answer) = call.answer.take() else {
                        return;
                    };
                    let call = ToolStarting {
                        session: call.session,
                        who: call.who.clone(),
                        name: call.name.clone(),
                        args: call.args.clone(),
                        reach: call.reach.clone(),
                        deny: None,
                        leave: call.leave,
                        answer: Some(answer),
                    };
                    commands.queue(move |world: &mut World| {
                        decide(world, call)
                    });
                },
            )
            .add_observer(
                |mut instructed: On<Instructed>,
                 mut commands: Commands| {
                    let told = instructed.event_mut();
                    told.instruction = Some(
                        commands
                            .spawn(Instruction {
                                agent: told.agent.clone(),
                                chief: told.user.clone(),
                                text: told.text.clone(),
                                session: told.session,
                                hearing: Hearing::default(),
                                task: None,
                                reviewed: false,
                            })
                            .id(),
                    );
                },
            )
            .add_observer(
                |started: On<TurnStarted>,
                 mut instructions: Query<&mut Instruction>,
                 mut commands: Commands| {
                    let started = started.event();
                    let mut session = commands.entity(started.session);
                    session.insert(Asked::default());
                    match started.instruction {
                        Some(instruction) => {
                            session.insert(Carrying(instruction))
                        }
                        None => session.remove::<Carrying>(),
                    };
                    if let Some(mut told) = started
                        .instruction
                        .and_then(|i| instructions.get_mut(i).ok())
                    {
                        told.task = Some(started.task.clone());
                    }
                },
            )
            .add_observer(
                |dropped: On<TurnDropped>, mut commands: Commands| {
                    let instruction = dropped.event().instruction;
                    commands.queue(move |world: &mut World| {
                        over(world, instruction)
                    });
                },
            )
            .add_observer(
                |answer: On<Answer>, mut commands: Commands| {
                    let answer = answer.event().clone();
                    commands.queue(move |world: &mut World| {
                        settle(world, answer)
                    });
                },
            )
            .add_observer(
                |answered: On<Answered>, mut commands: Commands| {
                    let answered = answered.event().clone();
                    commands.queue(move |world: &mut World| {
                        settle_picks(world, answered)
                    });
                },
            )
            .add_observer(
                |rewound: On<Rewound>, mut commands: Commands| {
                    let (session, at) =
                        (rewound.event().session, rewound.event().at);
                    commands.queue(move |world: &mut World| {
                        forget(world, session, at)
                    });
                },
            )
            .add_observer(|veto: On<Veto>, mut commands: Commands| {
                let veto = veto.event().clone();
                commands.queue(move |world: &mut World| {
                    stop(world, veto)
                });
            })
            .add_observer(
                |revoke: On<Revoke>,
                 agents: Query<(&Name, &Citizen), Without<Left>>,
                 mut grants: ResMut<Grants>| {
                    let revoke = revoke.event();
                    // The living agent of that name, else whoever
                    // was given it under that name.
                    let agent = agents
                        .iter()
                        .find(|(name, _)| {
                            name.as_str() == revoke.agent
                        })
                        .map(|(_, citizen)| citizen.0)
                        .or_else(|| {
                            grants
                                .held()
                                .iter()
                                .find(|g| g.name == revoke.agent)
                                .map(|g| g.agent)
                        });
                    if let Some(agent) = agent {
                        let _ = grants.revoke(
                            agent,
                            revoke.permission,
                            &revoke.chief,
                        );
                    }
                },
            )
            .add_observer(
                |_: On<Closing>,
                 mut commands: Commands,
                 waiting: Query<Entity, With<Waiting>>| {
                    // Left unsettled in the record, so the chief is
                    // told of them when the app next starts.
                    for entity in &waiting {
                        commands.entity(entity).despawn();
                    }
                },
            )
            .add_observer(
                |out: On<SessionOutput>, mut commands: Commands| {
                    let out = out.event();
                    // The rest, a streamed word among them, is
                    // nothing to follow.
                    if !matches!(
                        out.reply,
                        Reply::Event(
                            AgentEvent::Agent { .. }
                                | AgentEvent::ModelCall(_)
                        ) | Reply::Done(_)
                            | Reply::Error(_)
                    ) {
                        return;
                    }
                    let (session, reply) =
                        (out.session, out.reply.clone());
                    let by =
                        out.key.address().to_string();
                    commands.queue(move |world: &mut World| {
                        follow(world, session, &by, &reply)
                    });
                },
            )
            .add_observer(typed::typed)
            .add_observer(|left: On<Add, Left>, mut commands: Commands| {
                let agent = left.entity;
                commands
                    .queue(move |world: &mut World| lapse(world, agent));
            })
            .add_systems(Startup, (recall, departed))
            .add_systems(Update, review::hear.run_if(review::heard));
    }
}

/// The words the oversight rules look for and how much they quote.
#[derive(Resource, Clone, Debug)]
pub(crate) struct Rules {
    pub words: Words,
    /// The most of the mayor's notes its review of an instruction is
    /// shown, in characters.
    pub review_notes: usize,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            words: Words::default(),
            review_notes: 2000,
        }
    }
}

/// Present when the mayor is never asked about an instruction,
/// whatever the limits say.
#[derive(Resource)]
pub struct Unreviewed;

fn hamlet(world: &World) -> Hamlet {
    Hamlet::new(world.resource::<HomeDir>().0.root.clone())
}

/// The chief's limits as they were last read. Where the file
/// cannot be read, the strictest reading of what this module takes
/// from it.
fn limits(world: &World) -> Limits {
    agents::limits(world).unwrap_or_else(|_| Limits {
        asking: hamlet_home::limits::Asking {
            mayor_review: Review::Never,
            ..Default::default()
        },
        ..Limits::default()
    })
}

/// A chief takes a permission back.
#[derive(Event, Clone, Debug)]
pub struct Revoke {
    pub agent: String,
    pub permission: Permission,
    pub chief: String,
}

#[cfg(test)]
mod tests;
