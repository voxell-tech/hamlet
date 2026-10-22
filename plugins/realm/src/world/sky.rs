//! The look of the hour: the two lights, the haze the realm fades
//! into and the camera's light, following the chief's clock.

use std::time::{SystemTime, UNIX_EPOCH};

use glam::{Mat3, Vec3};
use hamlet_engine_sdk::scene::{
    Bloom, CameraSettings as Lens, DirectionalLight, Entity, Fog,
    FogFalloff, Light, Projection, Tonemapping,
};

use crate::build::frame::Frame;
use crate::world::tint;

/// The exposure the realm is drawn at, in EV100, before the look's
/// own.
const EXPOSURE: f32 = 9.7;

/// One time of day: lights, sky and how strongly the fittings burn.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    #[serde(with = "tint")]
    pub sun: Vec3,
    pub sun_lux: f32,
    /// The direction the key light travels.
    pub sun_dir: Vec3,
    #[serde(with = "tint")]
    pub fill: Vec3,
    pub fill_lux: f32,
    #[serde(with = "tint")]
    pub ambient: Vec3,
    pub ambient_nits: f32,
    #[serde(with = "tint")]
    pub haze: Vec3,
    pub saturation: f32,
    pub exposure: f32,
    /// Lumens of a lantern's spill at full strength.
    pub lantern_lumens: f32,
    /// Emissive strength of candle fittings: lamps at rest and the
    /// windows.
    pub candle: f32,
    /// Strength of the soft wash beside a lit groove.
    pub wash: f32,
    /// Multiplies the groove's light. Above 1 by day, where it
    /// competes with sunlit ground.
    pub inlay_gain: f32,
}

/// The look of day and of night.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Looks {
    pub day: Look,
    pub night: Look,
    /// The share of a groove's bed that is the light strip.
    pub strip_share: f32,
    /// Seconds for the trace behind a mote to fade to half.
    pub trace_half_life: f32,
}

impl Look {
    /// This look, `k` of the way to `to`. The key light's direction
    /// stays its own.
    pub fn toward(&self, to: &Look, k: f32) -> Look {
        let f = |a: f32, b: f32| a + (b - a) * k;
        Look {
            sun: mix(self.sun, to.sun, k),
            sun_lux: f(self.sun_lux, to.sun_lux),
            sun_dir: self.sun_dir,
            fill: mix(self.fill, to.fill, k),
            fill_lux: f(self.fill_lux, to.fill_lux),
            ambient: mix(self.ambient, to.ambient, k),
            ambient_nits: f(self.ambient_nits, to.ambient_nits),
            haze: mix(self.haze, to.haze, k),
            saturation: f(self.saturation, to.saturation),
            exposure: f(self.exposure, to.exposure),
            lantern_lumens: f(self.lantern_lumens, to.lantern_lumens),
            candle: f(self.candle, to.candle),
            wash: f(self.wash, to.wash),
            inlay_gain: f(self.inlay_gain, to.inlay_gain),
        }
    }
}

/// Linear `a`, `k` of the way to `b` through Oklab.
fn mix(a: Vec3, b: Vec3, k: f32) -> Vec3 {
    let lab = |c: Vec3| {
        let lms = Mat3::from_cols(
            Vec3::new(0.412_221_46, 0.211_903_5, 0.088_302_46),
            Vec3::new(0.536_332_55, 0.680_699_5, 0.281_718_85),
            Vec3::new(0.051_445_995, 0.107_396_96, 0.629_978_7),
        ) * c;
        let root = lms.map(f32::cbrt);
        Mat3::from_cols(
            Vec3::new(0.210_454_26, 1.977_998_5, 0.025_904_037),
            Vec3::new(0.793_617_8, -2.428_592_2, 0.782_771_77),
            Vec3::new(-0.004_072_047, 0.450_593_7, -0.808_675_77),
        ) * root
    };
    let back = |lab: Vec3| {
        let root = Mat3::from_cols(
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.396_337_78, -0.105_561_346, -0.089_484_18),
            Vec3::new(0.215_803_76, -0.063_854_17, -1.291_485_5),
        ) * lab;
        let lms = root * root * root;
        Mat3::from_cols(
            Vec3::new(4.076_741_7, -1.268_438, -0.004_196_086_3),
            Vec3::new(-3.307_711_6, 2.609_757_4, -0.703_418_6),
            Vec3::new(0.230_969_94, -0.341_319_38, 1.707_614_7),
        ) * lms
    };
    back(lab(a).lerp(lab(b), k))
}

/// The sky over a day: when the sun rises and sets, the look of dawn
/// and dusk, and which of the engine's lighting effects are on.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkySettings {
    /// True when the sun moves with the hour. Without it there are
    /// only the two looks of [`Looks`].
    pub moving: bool,
    /// Hours of the day the sun rises and sets at.
    pub sunrise: f32,
    pub sunset: f32,
    /// Hours a dawn or a dusk lasts.
    pub twilight: f32,
    /// The look at the middle of a dawn or a dusk. Its sun is the
    /// low sun.
    pub glow: Look,
    /// Share of a dawn or a dusk that has the glow's look: below 1
    /// it spreads wider.
    pub glow_spread: f32,
    /// The key light is whole at this height and fades to nothing at
    /// the horizon, as the sine of its height.
    pub horizon: f32,
    /// The lowest the key light is drawn at, as the sine of its
    /// height, so that shadows have an end.
    pub lowest: f32,
    /// Lumens of the light a door lantern and a common's lantern
    /// throw after dark. Zero leaves them out.
    pub door_lumens: f32,
    /// Reach of a door lantern's light.
    pub door_reach: f32,
    /// Shafts of light through the haze at dawn and dusk.
    pub shafts: bool,
    /// Density of the haze the shafts show in, how strongly it
    /// takes the light, and the steps taken through it.
    pub shaft_density: f32,
    pub shaft_strength: f32,
    pub shaft_steps: u32,
    /// Darkening where surfaces meet.
    pub occlusion: bool,
    /// The engine's scattering sky in place of the plain haze.
    pub atmosphere: bool,
}

impl SkySettings {
    pub fn night(&self, hour: f32) -> bool {
        !(self.sunrise..self.sunset).contains(&hour)
    }

    /// Share of the day's look an hour has, 0 to 1.
    pub fn daylight(&self, hour: f32) -> f32 {
        let half = self.twilight * 0.5;
        let step = |at: f32| {
            let k = ((hour - at + half) / self.twilight.max(1e-3))
                .clamp(0.0, 1.0);
            k * k * (3.0 - 2.0 * k)
        };
        step(self.sunrise) * (1.0 - step(self.sunset))
    }

    /// Place of the key light along its arc, 0 at its rising and 1 at
    /// its setting, and true when it is the sun.
    pub fn arc(&self, hour: f32) -> (f32, bool) {
        let day = self.sunset - self.sunrise;
        if self.night(hour) {
            let since = (hour - self.sunset).rem_euclid(24.0);
            (since / (24.0 - day).max(1e-3), false)
        } else {
            ((hour - self.sunrise) / day.max(1e-3), true)
        }
    }

    /// The middle of the day, or of the night.
    fn middle(&self, night: bool) -> f32 {
        let noon = (self.sunrise + self.sunset) * 0.5;
        if night { (noon + 12.0) % 24.0 } else { noon }
    }

    /// The hour shown while the sun does not move: the middle of the
    /// day or of the night `hour` is in.
    fn held(&self, hour: f32) -> f32 {
        self.middle(self.night(hour))
    }
}

/// The hour the sky shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Hour {
    /// The chief's clock.
    #[default]
    Clock,
    /// The middle of the day.
    Day,
    /// The middle of the night.
    Night,
}

impl Hour {
    /// The hour shown, on a clock `offset` seconds east of UTC.
    pub fn now(self, set: &SkySettings, offset: i32) -> f32 {
        match self {
            Self::Clock => Sky::hour_at(offset),
            Self::Day => set.middle(false),
            Self::Night => set.middle(true),
        }
    }
}

/// The camera: its lens, where it starts and how far a hand may move
/// it.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraSettings {
    /// Vertical field of view, in degrees.
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    /// Side of the sun's shadow map, in texels.
    pub shadow_map: usize,
    /// Distance from the focus before anyone moves it.
    pub distance: f32,
    /// Distance added for each unit the hamlets lie from their
    /// middle, so that all of them are in view.
    pub spread_distance: f32,
    /// Degrees below the horizon before anyone moves it.
    pub pitch: f32,
    /// Least and most degrees below the horizon.
    pub pitch_limits: (f32, f32),
    /// Least and most of the starting distance.
    pub zoom: (f32, f32),
    /// The furthest the camera stands from what it looks at, however
    /// far apart the hamlets lie.
    pub reach: f32,
    /// The furthest the focus may slide from the nearest thing
    /// worth looking at.
    pub pan: f32,
    /// Radians turned per pixel dragged.
    pub turn_rate: f32,
    /// Radians tilted per pixel dragged.
    pub tilt_rate: f32,
    /// Least and most a hand may tilt it, in radians.
    pub tilt: (f32, f32),
    /// Factor by which the ground slides faster up the view than
    /// across it, to make up for the camera's tilt.
    pub slide_tilt: f32,
    /// Rate of the camera's glide to the place it was asked to be,
    /// per second, and the closeness F comes in to, as a share of
    /// the starting distance.
    pub glide: f32,
    pub focus_zoom: f32,
    /// Rate at which the camera's height follows the land it
    /// crosses, per second.
    pub level: f32,
    /// Rate at which it keeps up with a hand sliding the ground, per
    /// second.
    pub slide: f32,
    /// Share zoomed per scrolled line.
    pub scroll_rate: f32,
    /// Scrolled pixels that count as one line.
    pub scroll_line: f32,
    /// A press that moved further than this many pixels is a drag,
    /// not a click.
    pub click_slop: f32,
    /// Fog begins at the distance times this.
    pub fog_start: f32,
    /// Fog is whole at the distance times the first, plus the
    /// second.
    pub fog_end: (f32, f32),
    /// The sun's shadows reach from, split at and end at the
    /// distance times these.
    pub shadow_range: (f32, f32, f32),
    /// Beyond this distance nothing is blurred.
    pub blur_within: f32,
    /// The f-stop at a distance of one unit. It narrows with
    /// distance.
    pub blur_stop: f32,
    pub blur_stop_min: f32,
    pub blur_circle: f32,
}

impl CameraSettings {
    /// The lens for the look `look` with the camera `dist` from what
    /// it looks at.
    pub fn lens(&self, look: &Look, dist: f32) -> Lens {
        let haze = (look.haze.x, look.haze.y, look.haze.z, 1.0);
        Lens {
            projection: Projection::Perspective((
                self.fov.to_radians(),
                self.near,
                self.far,
            )),
            clear: Some(haze),
            hdr: true,
            tonemapping: Tonemapping::Neutral,
            exposure: EXPOSURE - look.exposure,
            fog: Some(Fog {
                color: haze,
                falloff: FogFalloff::Linear((
                    dist * self.fog_start,
                    dist * self.fog_end.0 + self.fog_end.1,
                )),
            }),
            bloom: Some(Bloom {
                intensity: 0.15,
                low_frequency_boost: 0.7,
                threshold: 0.0,
            }),
            ambient: (
                (look.ambient.x, look.ambient.y, look.ambient.z, 1.0),
                look.ambient_nits,
            ),
            msaa: 4,
        }
    }
}

/// The sky as it is now.
#[derive(Clone, PartialEq, Debug)]
pub struct Sky {
    /// Hour of the day, 0 to 24.
    pub hour: f32,
    pub look: Look,
}

impl Sky {
    pub fn at(looks: &Looks, set: &SkySettings, hour: f32) -> Self {
        let hour = if set.moving { hour } else { set.held(hour) };
        let day = set.daylight(hour);
        // Away from dawn and dusk the looks are shown as designed.
        let mut look = if day >= 1.0 {
            looks.day.clone()
        } else if day <= 0.0 {
            looks.night.clone()
        } else {
            let glow =
                (4.0 * day * (1.0 - day)).powf(set.glow_spread);
            looks
                .night
                .toward(&looks.day, day)
                .toward(&set.glow, glow)
        };
        let (dir, height) = key_light(looks, set, hour);
        look.sun_dir = dir;
        let k = (height / set.horizon.max(1e-3)).clamp(0.0, 1.0);
        look.sun_lux *= k * k * (3.0 - 2.0 * k);
        Self { hour, look }
    }

    /// The hour on a clock `offset` seconds east of UTC.
    pub fn hour_at(offset: i32) -> f32 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64());
        let local = (now + f64::from(offset)).rem_euclid(86_400.0);
        (local / 3_600.0) as f32
    }
}

/// The direction the key light travels at an hour, and the sine of
/// its height. It rises and sets along a circle through the place
/// its look gives it, which is where it stands highest.
fn key_light(
    looks: &Looks,
    set: &SkySettings,
    hour: f32,
) -> (Vec3, f32) {
    let (along, sun) = set.arc(hour);
    let highest = if sun {
        looks.day.sun_dir
    } else {
        looks.night.sun_dir
    };
    let top = -highest.normalize();
    if (along - 0.5).abs() < 1e-4 {
        return (highest, top.y);
    }
    let rising = Vec3::Y.cross(top).normalize_or(Vec3::X);
    let (s, c) = (along * std::f32::consts::PI).sin_cos();
    let mut to = c * rising + s * top;
    let height = to.y;
    to.y = to.y.max(set.lowest);
    (-to.normalize(), height)
}

/// The key light, the sun by day and the moon by night, and the fill
/// light opposite it.
pub struct Lights {
    sun: Entity,
    fill: Entity,
}

impl Lights {
    pub fn make(sun: Entity, fill: Entity) -> Self {
        Self { sun, fill }
    }

    /// Lights the realm as `look` has it.
    pub fn show(&self, look: &Look) {
        let light = |color: Vec3, lux: f32, shadows: bool| {
            Some(Light::Directional(DirectionalLight {
                color: (color.x, color.y, color.z, 1.0),
                illuminance: lux,
                shadows,
            }))
        };
        let _ =
            self.sun.set_light(light(look.sun, look.sun_lux, true));
        self.sun.set_transform(
            Frame::looking_to(Vec3::ZERO, look.sun_dir).scene(),
        );
        let _ = self.fill.set_light(light(
            look.fill,
            look.fill_lux,
            false,
        ));
        let dir = Vec3::new(
            -look.sun_dir.x,
            -0.5,
            -look.sun_dir.z * 0.4 - 0.5,
        );
        self.fill.set_transform(
            Frame::looking_to(Vec3::ZERO, dir).scene(),
        );
    }
}
