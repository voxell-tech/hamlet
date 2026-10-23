//! The setting off of the world's magic: runes under a house that
//! rises, embers over one that falls, motes from one that goes over
//! to another, and a light over a thinking agent's house.

use std::f32::consts::TAU;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use hamlet_vfx::magic::{
    Ember, Flight, MagicSettings, Wand, chance, circle, fly, rise,
};
use hamlet_vfx::mark::{MarkMaterial, Marks};
use hamlet_vfx::sparks::{SparkSettings, Sparks, burst};

use super::House;
use super::shift::{Part, Shift};
use super::society::{AgentId, Society};
use super::work::Robot;
use crate::Clock;
use crate::keeper::KeeperState;
use crate::keeper::work::Task;
use crate::palette::Palette;

/// On a house whose coming or going has been given its magic.
#[derive(Component)]
pub(super) struct Charmed;

/// The light over the house of an agent that is thinking.
#[derive(Component)]
pub(super) struct Musing {
    of: AgentId,
    began: f32,
}

/// The room about a thinking agent's light that a pointer can hit,
/// and whether the pointer is on it.
#[derive(Component, Clone, Copy, Debug)]
pub struct Thought {
    pub of: AgentId,
    pub over: bool,
}

impl Musing {
    pub(super) fn of(&self) -> AgentId {
        self.of
    }
}

/// Gives a house that has begun to come or go its magic: runes
/// under one that rises, embers over one that falls.
pub(super) fn charm(
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<MagicSettings>,
    pal: Res<Palette>,
    wand: Res<Wand>,
    sparks: Option<Res<Sparks>>,
    spark_set: Res<SparkSettings>,
    society: Res<Society>,
    houses: Query<(Entity, &House, &Shift), Without<Charmed>>,
    standing: Query<(&House, &GlobalTransform)>,
    mut materials: ResMut<Assets<MarkMaterial>>,
    marks: Res<Marks>,
    mut floor: super::Floor,
) {
    for (house, house_of, shift) in &houses {
        let Some((began, from)) = shift.began() else {
            continue;
        };
        commands.entity(house).try_insert(Charmed);
        // A still shows the moment after, with no magic in the air.
        if clock.frozen {
            continue;
        }
        let ground = from.translation + Vec3::Y * set.lift;
        match shift.part() {
            Part::Rise => {
                circle(
                    &mut commands,
                    &marks,
                    &mut materials,
                    pal.rune * set.rune_light,
                    ground,
                    &floor.under(ground, set.radius * 1.1),
                    began,
                );
            }
            Part::Fall => {
                // A house that is merged away goes over to the one
                // that takes it in, and a circle opens there as it
                // lands.
                let host = society
                    .agent(house_of.agent)
                    .and_then(|gone| gone.into)
                    .and_then(|host| {
                        standing.iter().find(|(h, _)| {
                            h.agent == host && !h.archived
                        })
                    })
                    .map(|(_, at)| at.translation());
                if let Some(host) = host {
                    let to = host + Vec3::Y * set.flight_height;
                    for i in 0..set.motes {
                        let own = i as f32 + chance(began + i as f32);
                        let out = Vec3::new(
                            chance(own) - 0.5,
                            chance(own + 0.3),
                            chance(own + 0.6) - 0.5,
                        ) * set.radius;
                        fly(
                            &mut commands,
                            &wand,
                            Flight {
                                began,
                                from: ground + out,
                                to,
                                own,
                                time: set.flight_time,
                                arc: set.flight_arc,
                            },
                        );
                    }
                    circle(
                        &mut commands,
                        &marks,
                        &mut materials,
                        pal.rune * set.rune_light,
                        host + Vec3::Y * set.lift,
                        &floor.under(host, set.radius * 1.1),
                        began + set.flight_time,
                    );
                    continue;
                }
                // A few plain embers stand in where the world has
                // no particles.
                let burst = burst(
                    &mut commands,
                    sparks.as_deref(),
                    &spark_set,
                    began,
                    ground,
                );
                let plain =
                    if burst.is_some() { 0 } else { set.embers };
                for i in 0..plain {
                    let own = i as f32 + chance(began + i as f32);
                    let around = chance(own) * TAU;
                    let out =
                        chance(own + 0.5).sqrt() * set.radius * 0.7;
                    let from = ground
                        + Vec3::new(around.cos(), 0.0, around.sin())
                            * out;
                    rise(
                        &mut commands,
                        &wand,
                        Ember { began, from, own },
                    );
                }
            }
            Part::Clear => {
                // The house of a town that was set down somewhere
                // else goes over to where it now stands.
                let Some(to) = standing
                    .iter()
                    .find(|(h, at)| {
                        h.agent == house_of.agent
                            && !h.archived
                            && at.translation().xz()
                                != from.translation.xz()
                    })
                    .map(|(_, at)| at.translation())
                else {
                    continue;
                };
                let far = to.distance(ground);
                for i in 0..set.move_motes {
                    let own = i as f32 + chance(began + i as f32);
                    let out = Vec3::new(
                        chance(own) - 0.5,
                        chance(own + 0.3),
                        chance(own + 0.6) - 0.5,
                    ) * set.radius;
                    fly(
                        &mut commands,
                        &wand,
                        Flight {
                            began,
                            from: ground + out,
                            to: to + Vec3::Y * set.flight_height,
                            own,
                            time: set.move_time,
                            arc: set
                                .flight_arc
                                .max(far * set.move_arc),
                        },
                    );
                }
            }
            Part::Rest => {}
        }
    }
}

/// Keeps a small light over the house of every agent that waits on
/// its model, and puts it out when the thought is over.
pub(super) fn muse(
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<MagicSettings>,
    wand: Res<Wand>,
    robots: Query<&Robot>,
    houses: Query<(&House, &GlobalTransform)>,
    thoughts: Query<&Thought>,
    mut lights: Query<(Entity, &Musing, &mut Transform)>,
) {
    let thinks = |robot: &Robot| {
        robot.work.task == Task::Think
            && matches!(robot.state, KeeperState::Working)
    };
    let thinking = |agent: AgentId| {
        robots
            .iter()
            .any(|robot| robot.agent == agent && thinks(robot))
    };
    let home = |agent: AgentId| {
        houses
            .iter()
            .find(|(house, _)| {
                house.agent == agent && !house.archived
            })
            .map(|(_, at)| at.translation())
    };
    for (entity, musing, mut at) in &mut lights {
        let place = home(musing.of).filter(|_| thinking(musing.of));
        let Some(place) = place else {
            commands.entity(entity).try_despawn();
            continue;
        };
        let t = clock.t - musing.began;
        let bob = (t * set.musing_rate).sin() * set.musing_bob;
        let breath = 1.0 + 0.18 * (t * set.musing_rate * 2.3).sin();
        let grown = (t / set.musing_in).clamp(0.0, 1.0);
        // It swells under the pointer, to say it can be clicked.
        let over =
            thoughts.iter().any(|t| t.of == musing.of && t.over);
        let swell = if over { 1.5 } else { 1.0 };
        at.translation = place + Vec3::Y * (set.musing_height + bob);
        at.scale =
            Vec3::splat(set.musing_size * breath * grown * swell);
    }
    if clock.frozen {
        return;
    }
    for robot in &robots {
        if !thinks(robot)
            || lights.iter().any(|(_, m, _)| m.of == robot.agent)
            || home(robot.agent).is_none()
        {
            continue;
        }
        commands.spawn((
            Musing {
                of: robot.agent,
                began: clock.t,
            },
            Mesh3d(wand.mote()),
            MeshMaterial3d(wand.musing()),
            Transform::from_scale(Vec3::ZERO),
            NotShadowCaster,
            children![(
                Thought {
                    of: robot.agent,
                    over: false,
                },
                Mesh3d(wand.mote()),
                Transform::default(),
                Visibility::Hidden,
                Pickable::default(),
            )],
        ));
    }
}
