//! The camera, the two lights and the look of the hour: the same in
//! every world.

use std::f32::consts::PI;

use bevy::anti_alias::smaa::Smaa;
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{
    Atmosphere, CascadeShadowConfig, DirectionalLightShadowMap,
    FogVolume, ShadowFilteringMethod, VolumetricFog, VolumetricLight,
};
use bevy::pbr::{AtmosphereSettings, ScreenSpaceAmbientOcclusion};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal};
use chrono::Timelike;
use hamlet_assets::tint;
use hamlet_vfx::inlay::{self, EMBER, InlayPlugin};

use super::palette::Palette;
use super::{Clock, Stage, Step, Stock};

/// The camera, its lights and the look of the hour. Add it after
/// `settings::plugin` and `palette::plugin`.
pub fn plugin(app: &mut App) {
    app.add_plugins(InlayPlugin)
        .init_resource::<Looks>()
        .init_resource::<CameraSettings>()
        .init_resource::<Quality>()
        .init_resource::<SkySettings>()
        .init_resource::<FogSettings>()
        .init_resource::<Hour>()
        .init_resource::<North>()
        .init_state::<DayPart>()
        .add_computed_state::<Dark>()
        .add_computed_state::<LowSun>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (
                tell.in_set(Step::Clock),
                (
                    (light, light_windows)
                        .run_if(resource_changed::<Sky>),
                    dim_fill.after(light).run_if(
                        |sky: Res<Sky>,
                         fog: Res<FogSettings>,
                         quality: Res<Quality>| {
                            sky.is_changed()
                                || fog.is_changed()
                                || quality.is_changed()
                        },
                    ),
                    (occlude, sky_dome)
                        .run_if(resource_changed::<SkySettings>),
                    spend.run_if(resource_changed::<Quality>),
                    (raise_fog, fit_fog)
                        .chain()
                        .run_if(
                            |fog: Res<FogSettings>, quality: Res<Quality>| {
                                fog.is_changed() || quality.is_changed()
                            },
                        ),
                    fit_fog.run_if(resource_changed::<Sky>),
                    lay_fog,
                )
                    .in_set(Step::Scene),
            ),
        );
    let world = app.world_mut();
    let sky = Sky::at(
        world.resource::<Looks>(),
        world.resource::<SkySettings>(),
        world.resource::<SkySettings>().held(false),
        North(0.0),
    );
    world.insert_resource(sky);
    let size = world.resource::<CameraSettings>().shadow_map;
    world.insert_resource(DirectionalLightShadowMap { size });
}

/// One time of day: lights, sky and how strongly the fittings burn.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    #[serde(with = "tint")]
    pub sun: Color,
    pub sun_lux: f32,
    /// The direction the key light travels. A moving sun goes round
    /// through the day's, which is where it stands highest, and the
    /// moon goes opposite it.
    pub sun_dir: Vec3,
    #[serde(with = "tint")]
    pub fill: Color,
    pub fill_lux: f32,
    #[serde(with = "tint")]
    pub ambient: Color,
    pub ambient_nits: f32,
    #[serde(with = "tint")]
    pub haze: Color,
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
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Looks {
    pub day: Look,
    pub night: Look,
    /// The share of a groove's bed that is the light strip.
    pub strip_share: f32,
    /// Seconds for the trace behind a mote to fade to half.
    pub trace_half_life: f32,
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    Color::from(Oklaba::from(a).mix(&Oklaba::from(b), k))
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

/// The sky over a day: when the sun rises and sets, the look of dawn
/// and dusk, and which of the engine's lighting effects are on.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
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
    /// Darkening where surfaces meet. It takes the place of MSAA,
    /// which the engine cannot combine with it.
    pub occlusion: bool,
    /// The engine's scattering sky in place of the plain haze.
    pub atmosphere: bool,
}

impl SkySettings {
    /// The hour shown while the time of day is held to day or to
    /// night: the middle of each.
    pub fn held(&self, night: bool) -> f32 {
        let noon = (self.sunrise + self.sunset) * 0.5;
        if night { (noon + 12.0) % 24.0 } else { noon }
    }

    pub fn is_night(&self, hour: f32) -> bool {
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
        if self.is_night(hour) {
            let since = (hour - self.sunset).rem_euclid(24.0);
            (since / (24.0 - day).max(1e-3), false)
        } else {
            ((hour - self.sunrise) / day.max(1e-3), true)
        }
    }
}

/// The volumetric fog over the land, which the key light throws
/// shafts through. Not the distance fog that hides the edge of what
/// is drawn. The chief may change it, so it is kept apart from
/// [`SkySettings`], which the land is built from.
#[derive(
    Resource,
    Clone,
    PartialEq,
    Debug,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct FogSettings {
    /// False draws none.
    pub on: bool,
    /// How far one sees through the fog: what lies this far away is
    /// nearly hidden. It fills the air, with no top.
    pub visibility: f32,
    /// How brightly its shafts take the key light under a low sun,
    /// and the share of that they keep under the highest sun. It
    /// does not thicken the fog.
    pub strength: f32,
    pub high_sun: f32,
    /// How much of the light it throws on forward rather than every
    /// way, 0 to 1: the glow around the sun.
    pub glow: f32,
    /// How much light it scatters and how much it takes, 0 to 1.
    pub scattering: f32,
    pub absorption: f32,
    /// How strongly it takes the ambient light, which lifts it where
    /// no shaft falls.
    pub ambient: f32,
    /// The steps taken through it at high quality.
    pub steps: u32,
}

impl FogSettings {
    /// The engine's density for the visibility: light that far
    /// through it keeps a twentieth of itself.
    pub fn density(&self) -> f32 {
        let taken = (self.absorption + self.scattering).max(1e-3);
        20f32.ln() / (self.visibility.max(1.0) * taken)
    }
}

/// The camera: its lens, where it starts and how far a hand may move
/// it.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
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
    /// The sun's shadows split at and end at no nearer than the
    /// distance times these. They start at the camera.
    pub shadow_range: (f32, f32),
    /// Beyond this distance nothing is blurred.
    pub blur_within: f32,
    /// The f-stop at a distance of one unit. It narrows with
    /// distance.
    pub blur_stop: f32,
    pub blur_stop_min: f32,
    pub blur_circle: f32,
}

impl CameraSettings {
    /// World units across the view's height at the starting distance.
    pub fn span(&self) -> f32 {
        2.0 * self.distance * (self.fov.to_radians() * 0.5).tan()
    }
}

/// The world's spending on looking good.
#[derive(
    Resource,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// No particles, no bloom, no shafts of light, plain edges and
    /// small shadows.
    Low,
    /// Fewer particles, middling shadows and half the steps through
    /// the volumetric fog.
    Medium,
    #[default]
    High,
}

impl Quality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
    }

    /// The share of the full number of particles it draws.
    pub fn density(self) -> f32 {
        match self {
            Self::Low => 0.0,
            Self::Medium => 0.4,
            Self::High => 1.0,
        }
    }

    /// The share of the full shadow map's side it uses.
    pub fn shadows(self) -> f32 {
        match self {
            Self::Low => 0.25,
            Self::Medium => 0.5,
            Self::High => 1.0,
        }
    }

    /// The share of the full steps through the volumetric fog it
    /// takes. None
    /// draws no shafts of light.
    pub fn shafts(self) -> f32 {
        match self {
            Self::Low => 0.0,
            Self::Medium => 0.5,
            Self::High => 1.0,
        }
    }
}

/// The camera the world is seen through. The app points it at a
/// panel.
#[derive(Component)]
pub struct WorldCamera;

/// The key light: the sun by day, the moon by night.
#[derive(Component)]
pub struct Sun;

#[derive(Component)]
pub(crate) struct FillLight;

/// The volume of fog that shafts of light show in, and how strongly
/// it takes the key light before its width is made up for.
#[derive(Component, Default)]
pub(crate) struct WorldFog {
    light: f32,
}

/// The part of the day.
#[derive(
    States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default,
)]
pub enum DayPart {
    Dawn,
    #[default]
    Day,
    Dusk,
    Night,
}

/// The hours lanterns are lit: every part of the day but
/// [`DayPart::Day`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Dark;

impl ComputedStates for Dark {
    type SourceStates = DayPart;

    fn compute(part: DayPart) -> Option<Self> {
        (part != DayPart::Day).then_some(Dark)
    }
}

/// The hours the sun is low: [`DayPart::Dawn`] and
/// [`DayPart::Dusk`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LowSun;

impl ComputedStates for LowSun {
    type SourceStates = DayPart;

    fn compute(part: DayPart) -> Option<Self> {
        matches!(part, DayPart::Dawn | DayPart::Dusk)
            .then_some(LowSun)
    }
}

/// The hour to show, for whoever holds the clock. `None` follows the
/// wall clock.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct Hour(pub Option<f32>);

/// Degrees the sun's path is turned about the upright, clockwise seen
/// from above: where the sun rises and sets.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct North(pub f32);

/// The sky as it is now.
#[derive(Resource, Clone, PartialEq, Debug)]
pub struct Sky {
    /// Hour of the day, 0 to 24.
    pub hour: f32,
    pub look: Look,
    /// The direction the fill light travels. It follows the sun
    /// under the land too, so it never jumps.
    pub fill_dir: Vec3,
}

impl Default for Sky {
    fn default() -> Self {
        let set = SkySettings::default();
        Self::at(&Looks::default(), &set, set.held(false), North(0.0))
    }
}

impl Sky {
    pub fn at(
        looks: &Looks,
        set: &SkySettings,
        hour: f32,
        north: North,
    ) -> Self {
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
        let turn = Quat::from_rotation_y(-north.0.to_radians());
        let (dir, height) = key_light(looks, set, hour);
        look.sun_dir = turn * dir;
        let k = (height / set.horizon.max(1e-3)).clamp(0.0, 1.0);
        look.sun_lux *= k * k * (3.0 - 2.0 * k);
        let sun = -sun_at(looks, set, hour);
        let fill_dir =
            turn * Vec3::new(-sun.x, -0.5, -sun.z * 0.4 - 0.5);
        Self {
            hour,
            look,
            fill_dir,
        }
    }

    pub fn part(set: &SkySettings, hour: f32) -> DayPart {
        let half = set.twilight * 0.5;
        if !set.moving {
            if set.is_night(hour) {
                DayPart::Night
            } else {
                DayPart::Day
            }
        } else if (hour - set.sunrise).abs() < half {
            DayPart::Dawn
        } else if (hour - set.sunset).abs() < half {
            DayPart::Dusk
        } else if set.is_night(hour) {
            DayPart::Night
        } else {
            DayPart::Day
        }
    }
}

/// The place the sun stands at an hour, as a direction from the land.
/// It goes round one circle through the day's highest place: over
/// the land by day, under it by night, without a jump.
fn sun_at(looks: &Looks, set: &SkySettings, hour: f32) -> Vec3 {
    let (along, sun) = set.arc(hour);
    let top = -looks.day.sun_dir.normalize();
    let rising = Vec3::Y.cross(top).normalize_or(Vec3::X);
    // Half a turn over the day and half under the night.
    let turn = (along + if sun { 0.0 } else { 1.0 }) * PI;
    let (s, c) = turn.sin_cos();
    c * rising + s * top
}

/// The direction the key light travels at an hour, and the sine of
/// its height. It is the sun while the sun is up and the moon,
/// opposite it, while it is down. They change at the horizon, where
/// both give no light.
fn key_light(
    looks: &Looks,
    set: &SkySettings,
    hour: f32,
) -> (Vec3, f32) {
    let sun = sun_at(looks, set, hour);
    let mut to = if sun.y >= 0.0 { sun } else { -sun };
    let height = to.y;
    to.y = to.y.max(set.lowest);
    (-to.normalize(), height)
}

fn wall_hour() -> f32 {
    let now = chrono::Local::now();
    now.hour() as f32 + now.minute() as f32 / 60.0
}

/// Works out the hour and its look, and moves [`DayPart`] with it.
pub(crate) fn tell(
    clock: Res<Clock>,
    stage: Res<Stage>,
    looks: Res<Looks>,
    set: Res<SkySettings>,
    (held, north): (Res<Hour>, Res<North>),
    part: Res<State<DayPart>>,
    mut next: ResMut<NextState<DayPart>>,
    mut sky: ResMut<Sky>,
) {
    // A still never reads the wall clock.
    let hour = held
        .0
        .or_else(|| (!clock.frozen).then(wall_hour))
        // The stage has the last word on whether it is night.
        .filter(|&h| set.moving && set.is_night(h) == stage.night)
        .unwrap_or_else(|| set.held(stage.night));
    if sky.hour != hour
        || looks.is_changed()
        || set.is_changed()
        || north.is_changed()
    {
        let now = Sky::at(&looks, &set, hour, *north);
        if *sky != now {
            *sky = now;
        }
    }
    let want = Sky::part(&set, hour);
    if *part.get() != want {
        next.set(want);
    }
}

pub fn spawn(mut commands: Commands, settings: Res<CameraSettings>) {
    commands.spawn((
        WorldCamera,
        Camera3d::default(),
        // The marks of light on the ground are decals, which read
        // the depth of what they fall on.
        DepthPrepass,
        Projection::Perspective(PerspectiveProjection {
            fov: settings.fov.to_radians(),
            near: settings.near,
            far: settings.far,
            ..default()
        }),
        Msaa::Sample4,
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        ShadowFilteringMethod::Gaussian,
        DistanceFog::default(),
        ColorGrading::default(),
        Transform::default(),
    ));
    commands.spawn((
        Sun,
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default(),
    ));
    commands.spawn((
        FillLight,
        DirectionalLight {
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default(),
    ));
}

/// Sets the lights, the sky and the grading for the hour.
pub(crate) fn light(
    sky: Res<Sky>,
    mut ambient: ResMut<GlobalAmbientLight>,
    camera: Single<
        (&mut Camera, &mut DistanceFog, &mut ColorGrading),
        With<WorldCamera>,
    >,
    mut sun: Single<
        (&mut DirectionalLight, &mut Transform),
        (With<Sun>, Without<FillLight>),
    >,
    mut fill: Single<
        (&mut DirectionalLight, &mut Transform),
        (With<FillLight>, Without<Sun>),
    >,
) {
    let look = &sky.look;
    let (mut camera, mut fog, mut grading) = camera.into_inner();
    // Its own clear colour, so a window that also shows panels keeps
    // theirs.
    camera.clear_color = ClearColorConfig::Custom(look.haze);
    fog.color = look.haze;
    grading.global = ColorGradingGlobal {
        post_saturation: look.saturation,
        exposure: look.exposure,
        ..default()
    };
    ambient.color = look.ambient;
    ambient.brightness = look.ambient_nits;

    sun.0.color = look.sun;
    sun.0.illuminance = look.sun_lux;
    *sun.1 = Transform::default().looking_to(look.sun_dir, Vec3::Y);
    fill.0.color = look.fill;
    fill.0.illuminance = look.fill_lux;
    *fill.1 = Transform::default().looking_to(sky.fill_dir, Vec3::Y);
}

/// Lights every window pane after dark.
pub(crate) fn light_windows(
    sky: Res<Sky>,
    pal: Res<Palette>,
    stock: Res<Stock>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let candle = sky.look.candle;
    let glow = inlay::candle() * EMBER * candle;
    if let Some(mut pane) = materials.get_mut(&stock.windows) {
        pane.emissive = LinearRgba::rgb(glow.x, glow.y, glow.z);
        pane.base_color = if candle > 0.0 {
            pal.pane_lit
        } else {
            pal.pane_dark
        };
    }
}

/// Fits the camera's bloom and the size of the shadows to the
/// quality the world is set to.
pub(crate) fn spend(
    mut commands: Commands,
    quality: Res<Quality>,
    set: Res<CameraSettings>,
    camera: Single<Entity, With<WorldCamera>>,
    mut shadows: ResMut<DirectionalLightShadowMap>,
) {
    let side = (set.shadow_map as f32 * quality.shadows()) as usize;
    if shadows.size != side {
        shadows.size = side;
    }
    let mut camera = commands.entity(*camera);
    if *quality == Quality::Low {
        camera.remove::<Bloom>();
    } else {
        camera.insert(Bloom::NATURAL);
    }
}

/// Puts the darkening where surfaces meet on the camera or takes it
/// off.
pub(crate) fn occlude(
    mut commands: Commands,
    set: Res<SkySettings>,
    camera: Single<Entity, With<WorldCamera>>,
) {
    let mut camera = commands.entity(*camera);
    if set.occlusion {
        camera.insert((
            Msaa::Off,
            ScreenSpaceAmbientOcclusion::default(),
            Smaa::default(),
        ));
    } else {
        camera
            .remove::<(ScreenSpaceAmbientOcclusion, Smaa)>()
            .insert(Msaa::Sample4);
    }
}

/// Puts the engine's sky behind the world or takes it away.
pub(crate) fn sky_dome(
    mut commands: Commands,
    set: Res<SkySettings>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    camera: Single<Entity, With<WorldCamera>>,
    domes: Query<(), With<Atmosphere>>,
) {
    if !set.atmosphere {
        // The sky itself stays: the renderer panics if its medium
        // goes while a frame still holds it.
        commands.entity(*camera).try_remove::<AtmosphereSettings>();
        return;
    }
    if domes.is_empty() {
        let medium = media.add(ScatteringMedium::earth(256, 256));
        commands.spawn(Atmosphere::earth(medium));
    }
    commands
        .entity(*camera)
        .try_insert(AtmosphereSettings::default());
}

/// How low the key light stands, 0 high in the sky to 1 at its
/// lowest.
fn lowness(look: &Look, sky_set: &SkySettings) -> f32 {
    let height = -look.sun_dir.normalize_or(Vec3::NEG_Y).y;
    let k = ((height - sky_set.lowest) / (1.0 - sky_set.lowest))
        .clamp(0.0, 1.0);
    1.0 - k * k * (3.0 - 2.0 * k)
}

/// The steps taken through the volumetric fog, none when it is not
/// drawn.
fn fog_steps(set: &FogSettings, quality: Quality) -> u32 {
    if set.on {
        (set.steps as f32 * quality.shafts()).round() as u32
    } else {
        0
    }
}

/// Lets the key light throw shafts through the volumetric fog, as
/// many steps through it as the quality allows, or takes the fog
/// away.
pub(crate) fn raise_fog(
    mut commands: Commands,
    set: Res<FogSettings>,
    quality: Res<Quality>,
    camera: Single<Entity, With<WorldCamera>>,
    sun: Single<Entity, With<Sun>>,
    fog: Query<Entity, With<WorldFog>>,
) {
    let steps = fog_steps(&set, *quality);
    if steps == 0 {
        for fog in &fog {
            commands.entity(fog).despawn();
        }
        commands.entity(*camera).try_remove::<VolumetricFog>();
        commands.entity(*sun).try_remove::<VolumetricLight>();
        return;
    }
    commands.entity(*camera).try_insert(VolumetricFog {
        step_count: steps,
        ..default()
    });
    commands.entity(*sun).try_insert(VolumetricLight);
    if fog.is_empty() {
        commands.spawn((WorldFog::default(), FogVolume::default()));
    }
}

/// How far the volumetric fog reaches below sea level and above the
/// camera.
const FOG_MARGIN: f32 = 50.0;

/// Dims the fill light as the key light sinks while the volumetric
/// fog is drawn, and puts it out once it gives no light. The engine
/// orders the directional lights with the volumetric ones last,
/// though its fog looks for them first, so with the fill light drawn
/// the key light's shafts never show. Faded rather than put out at
/// once, the change is never seen.
pub(crate) fn dim_fill(
    sky: Res<Sky>,
    sky_set: Res<SkySettings>,
    set: Res<FogSettings>,
    quality: Res<Quality>,
    mut fill: Single<
        (&mut DirectionalLight, &mut Visibility),
        With<FillLight>,
    >,
) {
    let left = if fog_steps(&set, *quality) > 0 {
        1.0 - lowness(&sky.look, &sky_set)
    } else {
        1.0
    };
    fill.0.illuminance = sky.look.fill_lux * left;
    let shown = if left > 1e-3 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *fill.1 != shown {
        *fill.1 = shown;
    }
}

/// Fits the volumetric fog to its settings and to the hour: the
/// lower the sun, the more strongly the fog takes its light.
pub(crate) fn fit_fog(
    sky: Res<Sky>,
    set: Res<FogSettings>,
    sky_set: Res<SkySettings>,
    mut fog: Query<&mut VolumetricFog, With<WorldCamera>>,
    mut volumes: Query<(&mut WorldFog, &mut FogVolume)>,
) {
    let look = &sky.look;
    let low = lowness(look, &sky_set);
    let share = set.high_sun + (1.0 - set.high_sun) * low;
    for mut fog in &mut fog {
        fog.ambient_color = look.ambient;
        fog.ambient_intensity = set.ambient;
    }
    for (mut fog, mut volume) in &mut volumes {
        fog.light = set.strength * share;
        // `lay_fog` sets its light, once it knows how wide it is.
        *volume = FogVolume {
            density_factor: set.density(),
            absorption: set.absorption,
            scattering: set.scattering,
            scattering_asymmetry: set.glow,
            light_intensity: volume.light_intensity,
            ..default()
        };
    }
}

/// Keeps the volumetric fog about the camera as far as the sun's
/// shadows reach, and lit as strongly however wide it is. Past the
/// shadows the engine would light all of it, a bright wall with a
/// hard edge.
pub(crate) fn lay_fog(
    camera: Single<
        &Transform,
        (With<WorldCamera>, Without<WorldFog>),
    >,
    shadows: Query<&CascadeShadowConfig, With<Sun>>,
    mut fog: Query<(&WorldFog, &mut Transform, &mut FogVolume)>,
) {
    // The shadows end this far from the camera; see
    // `generator::view`, which sets them to the ground in view.
    let Some(&reach) =
        shadows.iter().next().and_then(|c| c.bounds.last())
    else {
        return;
    };
    let across = reach * 2.0;
    // From under the lowest ground to over the camera, so the camera
    // is always inside and the fog shows no top and no sides.
    let (foot, top) =
        (-FOG_MARGIN, camera.translation.y + FOG_MARGIN);
    let size = Vec3::new(across, top - foot, across);
    for (fog, mut at, mut volume) in &mut fog {
        *at = Transform::from_xyz(
            camera.translation.x,
            (foot + top) * 0.5,
            camera.translation.z,
        )
        .with_scale(size);
        // The engine dims the light in a volume as if it crossed the
        // whole of it, the radius of a ball about it, and this one is
        // as wide as the view. Making up for that lights it as a thin
        // layer of fog is lit.
        let crossed = volume.density_factor
            * size.length()
            * 0.5
            * (volume.absorption + volume.scattering);
        let light = fog.light * crossed.min(80.0).exp();
        if volume.light_intensity != light {
            volume.light_intensity = light;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minute by minute round the day, the key light, as its
    /// direction times its strength, and the fill's direction only
    /// ever move a little: the sun never jumps, nor does the moon
    /// that takes its place.
    #[test]
    fn the_light_goes_round_the_day_without_a_jump() {
        let (looks, set) = (Looks::default(), SkySettings::default());
        let north = North(30.0);
        let light = |sky: &Sky| sky.look.sun_dir * sky.look.sun_lux;
        let mut was = Sky::at(&looks, &set, 0.0, north);
        for minute in 1..=24 * 60 {
            let now =
                Sky::at(&looks, &set, minute as f32 / 60.0, north);
            let step = light(&now).distance(light(&was));
            // The day's light is thousands of lux: a jump in its
            // direction would move it by that much.
            assert!(step < 500.0, "light jumped {step} at {minute}");
            let turn = now.fill_dir.angle_between(was.fill_dir);
            assert!(turn < 0.05, "fill jumped {turn} at {minute}");
            was = now;
        }
    }
}
