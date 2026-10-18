//! A chief's hand on the world: robots poked, patted, lifted and
//! cheered, letters handed over, a thought nudged.

mod given;
mod lift;
mod notice;
mod pool;
#[cfg(test)]
mod tests;
mod touch;
mod toys;

use std::f32::consts::{PI, TAU};
use std::path::PathBuf;

use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::FileDragAndDrop;
use hamlet_core::agents::Mayor;

pub use self::given::{gifted, greet};
use self::lift::Fall;
pub use self::pool::Readout;
pub use self::touch::Proud;
use self::touch::{Rest, Tap};
use crate::WorldSystems;
use crate::generator::{Passage, Robot, Thought};
use crate::hamlet::Housed;
use crate::keeper::Act;
use crate::moving::Carrying;
use crate::pick::{self, Hovered, Reach, Taken, Thing, Travel};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<PlaySettings>()
        .init_resource::<Hand>()
        .init_resource::<Reach>()
        .init_resource::<Taken>()
        .init_resource::<Travel>()
        .init_resource::<HoverMap>()
        .add_message::<Wish>()
        .add_message::<Tap>()
        .add_message::<FileDragAndDrop>()
        .add_observer(touch::tap)
        .add_observer(touch::tell)
        .add_observer(touch::nudge)
        .add_observer(lift::grab)
        .add_observer(lift::let_go)
        .add_observer(lift::drag_end)
        .add_observer(touch::finished)
        .add_systems(Startup, (toys::prepare, pool::gauge::prepare))
        .add_systems(
            Update,
            (
                (meet, claims, touch::room),
                (
                    touch::pokes,
                    touch::pats,
                    lift::carry,
                    lift::fall,
                    given::letters,
                ),
                (
                    notice::watch,
                    notice::join,
                    notice::face,
                    touch::fade,
                )
                    .chain(),
                (pool::meters, toys::play),
            )
                .chain()
                .after(pick::aim)
                .before(WorldSystems),
        )
        .add_systems(
            PostUpdate,
            pool::readouts.before(bevy::ui::UiSystems::Layout),
        );
}

/// A chief's hand on the world: poking, patting and lifting the
/// robots, and what it gives them.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaySettings {
    /// Seconds within which a second click on a robot is a poke.
    pub poke_gap: f32,
    /// Number of pokes, within a number of seconds, that are one too
    /// many.
    pub annoy_pokes: usize,
    pub annoy_window: f32,
    /// Seconds the pointer rests on a robot before that is a pat,
    /// how many pixels it may stray meanwhile, and the seconds
    /// before resting pats the same robot again.
    pub pat_hold: f32,
    pub pat_still: f32,
    pub pat_rest: f32,
    /// A stroke: the pixels the pointer covers over a robot, how
    /// many times it turns back, within how many seconds, and the
    /// seconds before a stroke pats the same robot again.
    pub stroke_length: f32,
    pub stroke_turns: u32,
    pub stroke_time: f32,
    pub stroke_rest: f32,
    /// Height a robot is held over the ground, the rate it follows
    /// the hand at, and its lean behind the hand, in radians for
    /// each unit a second.
    pub lift_height: f32,
    pub lift_follow: f32,
    pub lift_lean: f32,
    /// Acceleration of a robot that is let go.
    pub fall: f32,
    /// Distance from another robot at which one stands to greet it.
    pub greet_gap: f32,
    /// Ground a robot covers per second on its way home, the most
    /// seconds the way may take, and the fastest it goes however
    /// far it was carried.
    pub scurry: f32,
    pub scurry_longest: f32,
    pub scurry_fastest: f32,
    /// Nearness of the pointer at which a robot looks at it, the
    /// most its head turns and lifts, and the rate.
    pub look_near: f32,
    pub look_turn: f32,
    pub look_lift: f32,
    pub look_rate: f32,
    /// Nearness of the pointer for a wave, the chance of one each
    /// time, and the seconds before the same robot waves again.
    pub wave_near: f32,
    pub wave_chance: f32,
    pub wave_rest: f32,
    /// Seconds after its work ends that a click cheers a robot, how
    /// near its neighbours stand to join in, and the seconds one
    /// further away waits for each unit of distance.
    pub cheer_window: f32,
    pub cheer_reach: f32,
    pub cheer_lag: f32,
    /// Seconds a nudged thought stays in its balloon, and how much
    /// larger than the light the room is that a pointer can hit.
    pub nudge_pin: f32,
    pub nudge_room: f32,
    /// Seconds a dropped file waits for the pointer to show where it
    /// fell.
    pub letter_wait: f32,
    /// The place of a gift, from its robot's place at rest: to its
    /// side and out from the house.
    pub gift_at: Vec2,
    /// The place of a hamlet's meter on the common, in the town's
    /// frame, and its size.
    pub meter_at: Vec2,
    pub meter_size: f32,
    /// Brightness of a meter's light, the blinks a second of an
    /// empty one, and the camera distance beyond which the rest is
    /// not written over it.
    pub meter_glow: f32,
    /// The light a hamlet's meter throws on what is about it after
    /// dark, and how far.
    pub meter_lumens: f32,
    pub meter_reach: f32,
    pub meter_blink: f32,
    pub meter_read_far: f32,
    /// A heart or a spark over a robot: how high over the ground it
    /// starts, how far it rises, its size and the seconds it lasts.
    pub heart_height: f32,
    pub heart_rise: f32,
    pub heart_size: f32,
    pub heart_time: f32,
    /// Height a gift or a letter falls from, the seconds it
    /// takes, and the seconds a gift stays in the yard.
    pub drop_height: f32,
    pub drop_time: f32,
    pub gift_stays: f32,
}

/// Something a chief asked of the core by hand.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum Wish {
    /// A road between two agents.
    Road { a: Entity, b: Entity },
    /// The file or folder at `path`, handed to an agent.
    Letter { to: Entity, path: PathBuf },
}

/// A robot's handling of late. Times are the scene's clock.
#[derive(Component, Debug)]
pub struct Mood {
    pokes: Vec<f32>,
    sulks_until: f32,
    patted: f32,
    waved: f32,
    /// True when the pointer was near at the last look, and the
    /// count of times it has come near.
    near: bool,
    passes: u32,
    look: Vec2,
    /// The way it turns to while a passage plays.
    face: Option<f32>,
    /// A motion it takes up at a clock time.
    joins: Option<(f32, Act)>,
    fall: Option<Fall>,
}

impl Default for Mood {
    fn default() -> Self {
        let long_ago = -1000.0;
        Self {
            pokes: Vec::new(),
            sulks_until: long_ago,
            patted: long_ago,
            waved: long_ago,
            near: false,
            passes: 0,
            look: Vec2::ZERO,
            face: None,
            joins: None,
            fall: None,
        }
    }
}

impl Mood {
    pub(crate) fn sulks(&self, now: f32) -> bool {
        now < self.sulks_until
    }
}

/// The robot the hand holds while the button is down.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    robot: Entity,
    /// It is off the ground.
    lifted: bool,
}

/// The chief's hand.
#[derive(Resource, Default)]
pub struct Hand {
    held: Option<Held>,
    /// The button was let go since the hand was last looked at.
    released: bool,
    last_tap: Option<(Entity, f32)>,
    rest: Option<Rest>,
    /// Files dropped on the window, each with the seconds it still
    /// waits to learn where it fell.
    letters: Vec<(PathBuf, f32)>,
    /// The robot under the pointer that could be lifted.
    within: Option<Entity>,
}

/// The hand's hold on the thing under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// It could take it.
    Ready,
    /// It carries a robot.
    Robot,
}

impl Hand {
    /// The robot held off the ground.
    pub fn lifted(&self) -> Option<Entity> {
        self.held.filter(|held| held.lifted).map(|held| held.robot)
    }

    pub fn grip(&self) -> Option<Grip> {
        match self.held {
            Some(Held { lifted: true, .. }) => Some(Grip::Robot),
            Some(_) => Some(Grip::Ready),
            None => self.within.map(|_| Grip::Ready),
        }
    }
}

/// The angle from `a` to `b`, the short way round.
fn bend(a: f32, b: f32) -> f32 {
    (b - a + PI).rem_euclid(TAU) - PI
}

/// The way from `from` to `to` over the ground, as a turn about Y.
fn toward(from: Vec3, to: Vec3) -> f32 {
    (to.x - from.x).atan2(to.z - from.z)
}

fn yaw_of(rotation: Quat) -> f32 {
    rotation.to_euler(EulerRot::YXZ).0
}

/// True when a robot can be given a motion to play now: about its
/// house, and in nothing it must finish.
fn free(robot: &Robot, passage: Option<&Passage>) -> bool {
    robot.at_home()
        && passage.is_none_or(|passage| {
            matches!(
                passage.act(),
                Act::Wave
                    | Act::Wiggle
                    | Act::Startle
                    | Act::Land
                    | Act::Greet
                    | Act::Receive
                    | Act::Celebrate
            )
        })
}

/// The robot a thing is, if it is one.
fn robot_of(
    thing: Thing,
    boxes: &Query<(Entity, &Thing, &ChildOf)>,
    robots: &Query<&Robot>,
) -> Option<Entity> {
    if !matches!(thing, Thing::Keeper | Thing::Resident(_)) {
        return None;
    }
    boxes
        .iter()
        .find(|(_, of, parent)| {
            **of == thing && robots.contains(parent.parent())
        })
        .map(|(_, _, parent)| parent.parent())
}

/// Gives every new robot a mood.
fn meet(
    mut commands: Commands,
    robots: Query<Entity, (With<Robot>, Without<Mood>)>,
) {
    for robot in &robots {
        commands.entity(robot).insert(Mood::default());
    }
}

/// The robot `thing` is, if it can be lifted now.
fn liftable(
    thing: Option<Thing>,
    boxes: &Query<(Entity, &Thing, &ChildOf)>,
    robots: &Query<&Robot>,
    passages: &Query<&Passage>,
) -> Option<Entity> {
    thing
        .and_then(|thing| robot_of(thing, boxes, robots))
        .filter(|robot| {
            robots
                .get(*robot)
                .is_ok_and(|of| free(of, passages.get(*robot).ok()))
        })
}

/// Notes what the pointer is on that the hand could take or that is
/// not a thing of the world, and takes the click of the latter from
/// the selection.
fn claims(
    over: Res<HoverMap>,
    mut thoughts: Query<(Entity, &mut Thought)>,
    hovered: Res<Hovered>,
    boxes: Query<(Entity, &Thing, &ChildOf)>,
    robots: Query<&Robot>,
    passages: Query<&Passage>,
    carrying: Option<Res<Carrying>>,
    mut hand: ResMut<Hand>,
    mut taken: ResMut<Taken>,
) {
    // Read from the hover itself: a pointer taken off the world
    // while it is on one of these leaves no event behind.
    let on = |entity: Entity| {
        over.values().any(|hits| hits.contains_key(&entity))
    };
    for (entity, mut thought) in &mut thoughts {
        let on = on(entity);
        if thought.over != on {
            thought.over = on;
        }
    }
    let within = liftable(hovered.0, &boxes, &robots, &passages);
    if hand.within != within {
        hand.within = within;
    }
    // A place in hand takes every click.
    let carrying = carrying.is_some_and(|c| c.0.is_some());
    let click =
        carrying || thoughts.iter().any(|(_, thought)| thought.over);
    if taken.click != click {
        taken.click = click;
    }
}

/// The agent a thing stands for, with its robot if it is in sight.
fn met(
    thing: Thing,
    boxes: &Query<(Entity, &Thing, &ChildOf)>,
    robots: &Query<&Robot>,
    agents: &Query<(Entity, &Housed, Has<Mayor>)>,
) -> Option<(Entity, Option<Entity>)> {
    let robot_for = |agent: Entity| {
        let (_, housed, _) = agents.get(agent).ok()?;
        boxes.iter().map(|(_, _, parent)| parent.parent()).find(
            |robot| {
                robots.get(*robot).is_ok_and(|r| r.agent == housed.0)
            },
        )
    };
    match thing {
        Thing::Resident(agent) | Thing::ResidentHouse(agent) => {
            Some((agent, robot_for(agent)))
        }
        Thing::Keeper | Thing::Cottage => {
            let robot = robot_of(Thing::Keeper, boxes, robots)?;
            let id = robots.get(robot).ok()?.agent;
            let (agent, ..) = agents
                .iter()
                .find(|(_, housed, _)| housed.0 == id)?;
            Some((agent, Some(robot)))
        }
        _ => None,
    }
}
