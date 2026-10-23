//! A chief's hand on the realm: robots poked, patted, lifted and
//! cheered, and two robots introduced, which builds a road between
//! their agents.

mod lift;
mod notice;
mod touch;
mod toys;

use std::f32::consts::{PI, TAU};

use bevy_ecs::system::SystemParam;
use glam::{EulerRot, Quat, Vec2, Vec3};
use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{MouseButton, ScenePointer};

use crate::bridge::Bridge;
use crate::build::upload::Draw;
use crate::folk::hand::lift::Fall;
pub use crate::folk::hand::toys::Toys;
use crate::folk::keeper::{Act, Robot, Robots};
use crate::folk::sounds::{FolkCue, FolkSound};
use crate::places::moving::Carrying;
use crate::realm::{Clock, Now};
use crate::society::{AgentId, Society};
use crate::tuning::WorldTuning;
use crate::view::camera::Camera;
use crate::view::pick::{Picking, Thing};
use crate::world::land::Land;
use crate::world::palette::Palette;

/// A chief's hand on the realm: poking, patting and lifting the
/// robots. Only what the realm plays is read.
#[derive(Clone, Debug, serde::Deserialize)]
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
    /// A heart or a spark over a robot: how high over the ground it
    /// starts, how far it rises, its size and the seconds it lasts.
    pub heart_height: f32,
    pub heart_rise: f32,
    pub heart_size: f32,
    pub heart_time: f32,
}

/// A robot's handling of late. Times are the realm's clock.
#[derive(Debug)]
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
    pub(crate) fall: Option<Fall>,
    /// Clock time until which a click cheers it, after its work
    /// ended.
    proud_until: Option<f32>,
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
            proud_until: None,
        }
    }
}

impl Mood {
    pub fn sulks(&self, now: f32) -> bool {
        now < self.sulks_until
    }

    /// Follows its agent's state: for a while after its work ends,
    /// a click on it cheers it.
    pub fn follow(
        &mut self,
        working: bool,
        was_working: bool,
        now: f32,
        cheer_window: f32,
    ) {
        if working {
            self.proud_until = None;
        } else if was_working {
            self.proud_until = Some(now + cheer_window);
        }
    }
}

/// The robot the hand holds while the button is down.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    agent: AgentId,
    /// It is off the ground.
    lifted: bool,
}

/// The pointer resting on a robot, or stroking it.
#[derive(Clone, Copy, Debug)]
struct Rest {
    agent: AgentId,
    /// The spot it settled at, and the time it did.
    anchor: Vec2,
    since: f32,
    last: Vec2,
    /// The stroke so far: when it began, the pixels covered, the
    /// way it last went and how often it turned back.
    began: f32,
    path: f32,
    heading: f32,
    turns: u32,
}

/// The chief's hand.
#[derive(Resource, Default)]
pub struct Hand {
    held: Option<Held>,
    /// The button was let go since the hand was last looked at.
    released: bool,
    /// The left button is down.
    down: bool,
    /// Pixels the pointer moved since the left button went down.
    travel: f32,
    /// The pointer over the view, in its pixels.
    pointer: Option<Vec2>,
    /// The robots clicked since the hand was last looked at.
    taps: Vec<AgentId>,
    last_tap: Option<(AgentId, f32)>,
    rest: Option<Rest>,
    /// The robot under the pointer that could be lifted.
    within: Option<AgentId>,
}

impl Hand {
    /// The agent of the robot held off the ground.
    pub fn lifted(&self) -> Option<AgentId> {
        self.held.filter(|held| held.lifted).map(|held| held.agent)
    }

    /// True while the hand could take the robot under the pointer, or
    /// holds one not yet lifted.
    pub fn ready(&self) -> bool {
        self.held.is_some() || self.within.is_some()
    }

    /// Follows what the person did over the view, with the robot of
    /// `over` under the pointer, and gives back what the camera and
    /// the picking are to see of it: a robot in the hand holds the
    /// ground still, and the release that sets a lifted robot down
    /// is the hand's alone.
    pub fn input(
        &mut self,
        input: &SceneInput,
        over: Option<AgentId>,
        click_slop: f32,
    ) -> Option<SceneInput> {
        match input {
            SceneInput::Pointer(pointer) => {
                self.pointer = Some(Vec2::new(pointer.x, pointer.y));
                if self.down {
                    self.travel +=
                        Vec2::new(pointer.dx, pointer.dy).length();
                }
                if self.held.is_some() {
                    return Some(SceneInput::Pointer(ScenePointer {
                        dx: 0.0,
                        dy: 0.0,
                        ..*pointer
                    }));
                }
            }
            SceneInput::Press(press)
                if press.button == MouseButton::Primary =>
            {
                if press.down {
                    self.down = true;
                    self.travel = 0.0;
                    // A press in the frame of a release: what is held
                    // is let go first.
                    if self.held.is_none() {
                        self.released = false;
                        self.held = self.within.map(|agent| Held {
                            agent,
                            lifted: false,
                        });
                    }
                } else {
                    self.down = false;
                    if self.travel <= click_slop
                        && let Some(agent) = over
                    {
                        self.taps.push(agent);
                    }
                    if let Some(held) = self.held {
                        self.released = true;
                        if held.lifted {
                            return None;
                        }
                    }
                }
            }
            _ => {}
        }
        Some(*input)
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

/// The point where the ray from `from` along `way` meets the level
/// `height`, in front of it.
fn on_level(ray: Option<(Vec3, Vec3)>, height: f32) -> Option<Vec3> {
    let (from, way) = ray?;
    let far = (height - from.y) / way.y;
    (far.is_finite() && far > 0.0).then(|| from + way * far)
}

/// The agent a thing stands for: its robot or its house.
fn agent_of(thing: Option<Thing>) -> Option<AgentId> {
    match thing? {
        Thing::Robot(agent) | Thing::House(agent) => Some(agent),
        _ => None,
    }
}

/// What a frame of the hand reads.
#[derive(SystemParam)]
pub struct Touching<'w> {
    tuning: Res<'w, WorldTuning>,
    clock: Res<'w, Clock>,
    now: Res<'w, Now>,
    land: Res<'w, Land>,
    society: Res<'w, Society>,
    bridge: Res<'w, Bridge>,
    camera: Res<'w, Camera>,
    picking: Res<'w, Picking>,
    carrying: Res<'w, Carrying>,
    pal: Res<'w, Palette>,
}

/// What a frame of the hand is played with.
pub struct HandFrame<'a> {
    set: &'a PlaySettings,
    now: f32,
    dt: f32,
    /// Where the camera stands, and the ray through the pointer.
    eye: Vec3,
    ray: Option<(Vec3, Vec3)>,
    /// The pixels the pointer may move while a button is down and
    /// still click.
    click_slop: f32,
    land: &'a Land,
}

/// Plays a frame of the chief's hand: what it clicked, rested on,
/// lifted and let go, and the robots that notice it.
pub fn play_hand(
    touching: Touching,
    mut hand: ResMut<Hand>,
    mut robots: ResMut<Robots>,
    mut toys: ResMut<Toys>,
    pickable: Query<(Entity, &SceneKey), With<Pickable>>,
    mut cues: MessageWriter<FolkCue>,
    mut draw: Draw,
) {
    let tuning = &*touching.tuning;
    let frame = HandFrame {
        set: &tuning.play,
        now: touching.clock.now,
        dt: touching.clock.dt,
        eye: touching.now.eye,
        ray: touching.camera.ray(&tuning.camera),
        click_slop: tuning.camera.click_slop,
        land: &touching.land,
    };
    let mut cue = |sound: FolkSound, at: Vec3, turn: u32| {
        cues.write(FolkCue { sound, at, turn });
    };
    let hovered = touching.picking.hovered();
    // A town in hand takes every click.
    let within = match hovered {
        Some(Thing::Robot(agent))
            if touching.carrying.carry.is_none() =>
        {
            Some(agent).filter(|agent| {
                robots.robot(*agent).is_some_and(Robot::free)
            })
        }
        _ => None,
    };
    if hand.within != within {
        hand.within = within;
    }
    toys.hand_over(&touching.pal, &mut draw);
    touch::pokes(&frame, &mut hand, &mut robots, &mut toys, &mut cue);
    touch::pats(
        &frame,
        hovered,
        &mut hand,
        &mut robots,
        &mut toys,
        &mut cue,
    );
    let mut watched = Vec::new();
    let wish = lift::carry(
        &frame,
        tuning.pick.robot_box.y,
        hovered,
        &touching.society,
        &mut hand,
        &mut robots,
        &mut watched,
        &mut cue,
    );
    lift::fall(&frame, &mut robots, &mut watched, &mut cue);
    notice::watch(&frame, &hand, &mut robots, &mut cue);
    notice::join(&frame, &mut robots);
    notice::face(&frame, &tuning.motion, &hand, &mut robots);
    toys.play(&frame, &mut draw);
    for (agent, on) in watched {
        let key = Thing::Robot(agent).key();
        for (entity, _) in
            pickable.iter().filter(|(_, of)| of.0 == key)
        {
            draw.commands.entity(entity).try_insert(Pickable {
                hover: on,
                click: on,
            });
        }
    }
    if let Some((a, b)) = wish {
        introduce(&touching.bridge, a, b, &mut draw.commands);
    }
}

/// Builds a road between the agents `a` and `b`, at the chief's
/// word to the hamlet of `a`.
fn introduce(
    bridge: &Bridge,
    a: AgentId,
    b: AgentId,
    commands: &mut Commands,
) {
    let (Some((a, hamlet)), Some((b, _))) =
        (bridge.named(a), bridge.named(b))
    else {
        return;
    };
    commands.agents(&AgentsRequest::Decree {
        hamlet: hamlet.clone(),
        text: format!("/road {a} {b}"),
    });
}
