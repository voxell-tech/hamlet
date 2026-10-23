//! The chief in the realm (docs/chief.md): its place, the Hearth, its
//! mascot, and the Wickstep that carries the mascot to whoever the
//! chief talks to. The chief looks as a chief does who never chose a
//! look, and its place stands where the land puts it.

mod company;
mod hearth;
mod homing;
pub mod look;
pub mod mascot;
pub mod place;
mod wickstep;

use glam::{Vec2, Vec3};
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::Shader;
use hamlet_engine_bevy::{AgentsEventReceived, Replied, Reply};

use crate::build::upload::Draw;
use crate::folk::chief::company::whereabouts;
pub use crate::folk::chief::company::{Keeping, follow_company};
use crate::folk::chief::hearth::Hearth;
pub use crate::folk::chief::homing::Walking;
use crate::folk::chief::look::Look;
use crate::folk::chief::wickstep::With;
pub use crate::folk::chief::wickstep::{Company, chance};
use crate::folk::keeper::{Act, Passage, Robots};
use crate::folk::sounds::FolkCue;
use crate::places::library::Library;
use crate::places::moving::SetDown;
use crate::realm::{Ctx, Shared};
use crate::society::{AgentId, Place, Society};
use crate::tuning::WorldTuning;
use crate::view::camera::Camera;
use crate::view::pick::{Picking, Thing};
use crate::world::land::Land;
use crate::world::palette::Palette;

/// The chief's place and mascot. Nothing when the land had no room
/// for a place.
#[derive(Resource)]
pub struct Chief {
    hearth: Option<Hearth>,
}

impl Chief {
    /// Gives the chief its ground, before any town is founded. Its
    /// runes are drawn by `marks`.
    pub fn make(
        tuning: &WorldTuning,
        pal: &Palette,
        marks: &Handle<Shader>,
        land: &mut Land,
        society: &mut Society,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        Ok(Self {
            hearth: Hearth::settle(
                &tuning.chief,
                pal,
                Look::default(),
                tuning.play.scurry_fastest,
                marks,
                land,
                society,
                draw,
            )?,
        })
    }

    /// Builds what is left to build, and moves the mascot a frame
    /// `dt` seconds further in the company `with`, beside the camera
    /// turned `yaw`, waving while `hovered`. The agent it came to
    /// keep company with.
    pub fn update(
        &mut self,
        ctx: &Ctx,
        with: Option<With>,
        (yaw, hovered): (f32, bool),
        dt: f32,
        cue: &mut impl FnMut(FolkCue),
        draw: &mut Draw,
    ) -> Option<AgentId> {
        self.hearth.as_mut().and_then(|hearth| {
            hearth.update(ctx, with, (yaw, hovered), dt, cue, draw)
        })
    }

    /// Stands the chief's place on `at`, where the chief set it down.
    pub fn move_to(
        &mut self,
        pal: &Palette,
        at: Vec2,
        land: &mut Land,
        society: &mut Society,
        draw: &mut Draw,
    ) {
        if let Some(hearth) = &mut self.hearth {
            hearth.move_to(pal, at, land, society, draw);
        }
    }

    /// Where the mascot stands.
    pub fn mascot(&self) -> Option<Vec3> {
        self.hearth.as_ref().map(Hearth::mascot)
    }

    /// Where the Hearth stands.
    pub fn hearth(&self) -> Option<Vec3> {
        self.hearth.as_ref().map(Hearth::house)
    }
}

/// What the chief's place is stood anew with.
type Grounds<'w> =
    (Res<'w, Palette>, ResMut<'w, Land>, ResMut<'w, Society>);

/// Builds what is left of the chief's place, and moves the mascot a
/// frame further.
pub fn move_chief(
    mut chief: ResMut<Chief>,
    mut world: ParamSet<(Shared, Grounds)>,
    keeping: Res<Keeping>,
    (camera, picking, library): (
        Res<Camera>,
        Res<Picking>,
        Res<Library>,
    ),
    mut robots: ResMut<Robots>,
    mut cues: MessageWriter<FolkCue>,
    mut replies: MessageReader<Replied>,
    mut events: MessageReader<AgentsEventReceived>,
    mut set_downs: MessageReader<SetDown>,
    mut draw: Draw,
) {
    // The spot the chief's record has the Hearth at, or where the
    // chief set it down, which the home keeps.
    let mut spot = None;
    for replied in replies.read() {
        if let Ok(Reply::Agents(AgentsReply::Chief(record))) =
            &replied.reply
        {
            spot = record.hearth.or(spot);
        }
    }
    for event in events.read() {
        if let AgentsEvent::Chief(record) = &event.event {
            spot = record.hearth.or(spot);
        }
    }
    let mut spot = spot.map(|(x, z)| Vec2::new(x, z));
    for set in set_downs.read() {
        if set.place == Place::Chief {
            spot = Some(set.to);
            draw.commands.agents(&AgentsRequest::SetHearth((
                set.to.x, set.to.y,
            )));
        }
    }
    if let Some(at) = spot
        && chief
            .hearth
            .as_ref()
            .is_some_and(|hearth| hearth.at().distance(at) > 0.01)
    {
        let (pal, mut land, mut society) = world.p1();
        chief.move_to(&pal, at, &mut land, &mut society, &mut draw);
    }
    let shared = world.p0();
    let floor = shared.floor();
    let ctx = shared.ctx(&floor);
    let with = keeping.with.and_then(|company| {
        whereabouts(
            company,
            ctx.clock,
            ctx.tuning.chief.stroll,
            (ctx.land, ctx.society),
            &robots,
            &library,
        )
    });
    let yaw = camera.views().0.yaw;
    let hovered = picking.hovered() == Some(Thing::Mascot);
    let came = chief.update(
        &ctx,
        with,
        (yaw, hovered),
        shared.clock.dt,
        &mut |cue| {
            cues.write(cue);
        },
        &mut draw,
    );
    // Its agent waves as it comes, unless it is in the middle of a
    // motion of its own.
    if let Some(robot) =
        came.and_then(|agent| robots.robot_mut(agent))
        && robot.passage.is_none()
        && robot.at_home()
    {
        robot.passage = Some(Passage::new(Act::Wave, ctx.clock));
    }
}

/// A chief's place, its mascot and the Wickstep that carries it.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefSettings {
    /// The mascot's size, over the size it is built at.
    pub size: f32,
    /// Seconds a Wickstep takes to go out, stays dark, and takes to
    /// come in.
    pub out_time: f32,
    pub dark_time: f32,
    pub in_time: f32,
    /// The rune circle of a Wickstep: its reach, how far over the
    /// ground it lies and how bright it is.
    pub rune_radius: f32,
    pub rune_lift: f32,
    pub rune_light: f32,
    /// The motes of a Wickstep: how many at full quality, the
    /// seconds each lasts, how high they rise, how far they spread
    /// and their size.
    pub motes: u32,
    pub mote_time: f32,
    pub mote_rise: f32,
    pub mote_spread: f32,
    pub mote_size: f32,
    /// The wick, and the motes with it: how bright it is drawn, the
    /// light it throws and how far, and how many times brighter
    /// it flares on arriving.
    pub wick_light: f32,
    pub lamp: f32,
    pub lamp_range: f32,
    pub flare: f32,
    /// Brightness of the orb on a chief's roof and of the waystone's
    /// ring, and of the lantern over the gate after dark.
    pub orb_light: f32,
    pub waystone_light: f32,
    pub lantern_light: f32,
    /// Beside an agent: the mascot's offset to its side and toward
    /// the camera, the stray of the agent before it is followed on
    /// foot, its distance before the mascot hurries, and the
    /// farthest the mascot goes on foot. Past that it goes by a
    /// Wickstep.
    pub beside: f32,
    pub before: f32,
    pub slack: f32,
    pub restep: f32,
    pub afoot: f32,
    /// Distance toward the camera from its agent of the point the
    /// mascot faces, so that its face shows.
    pub cheat: f32,
    /// By a hamlet's common: how far from its middle the mascot
    /// stands, and the seconds before it walks on to another spot.
    pub stroll: (f32, f32),
    /// Speeds of the mascot's walk and turn, and the seconds it
    /// takes to ease from one motion into the next. After an agent
    /// that gets ahead it walks faster, up to a robot's fastest.
    pub walk_speed: f32,
    pub turn_rate: f32,
    pub blend: f32,
    /// The least and the most seconds it stays at one spot of its
    /// grounds.
    pub linger: (f32, f32),
    /// The radius of the wall about a chief's grounds.
    pub grounds: f32,
    /// The box a pointer can hit for the mascot, and the radius and
    /// height of the room the house takes.
    pub mascot_box: Vec3,
    pub house: (f32, f32),
}
