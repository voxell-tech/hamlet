//! The chief in the realm (docs/chief.md): its place, the Hearth, its
//! mascot, and the Wickstep that carries the mascot to whoever the
//! chief talks to. The chief looks as a chief does who never chose a
//! look, and its place stands where the land puts it.

mod hearth;
mod homing;
pub mod look;
pub mod mascot;
mod place;
mod wickstep;

use glam::Vec3;
use hamlet_engine_sdk::scene::gpu::Shader;

use crate::build::upload::Upload;
use crate::folk::chief::hearth::Hearth;
use crate::folk::chief::look::Look;
pub use crate::folk::chief::place::stand;
use crate::realm::Ctx;
use crate::society::Society;
use crate::tuning::WorldTuning;
use crate::world::land::Land;
use crate::world::palette::Palette;

/// The chief's place and mascot. Nothing when the land had no room
/// for a place.
pub struct Chief {
    hearth: Option<Hearth>,
}

impl Chief {
    /// Gives the chief its ground, before any town is founded. Its
    /// runes are drawn by `marks`.
    pub fn make(
        tuning: &WorldTuning,
        pal: &Palette,
        marks: &Shader,
        land: &mut Land,
        society: &mut Society,
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
            )?,
        })
    }

    /// Builds what is left to build, and moves the mascot a frame
    /// `dt` seconds further.
    pub fn update(
        &mut self,
        ctx: &Ctx,
        dt: f32,
        upload: &mut Upload,
    ) {
        if let Some(hearth) = &mut self.hearth {
            hearth.update(ctx, dt, upload);
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
