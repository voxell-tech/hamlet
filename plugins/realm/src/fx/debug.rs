//! The debug overlay, shown and hidden with its key: the room each
//! thing the pointer can hit takes and where the pointer's ray meets
//! them, the line each road follows and the room and door of every
//! plot, drawn over the realm; and on the status line the frame
//! time, what is under the pointer and the part of the day.

use std::f32::consts::TAU;
use std::hash::{DefaultHasher, Hash, Hasher};

use bevy_ecs::system::SystemParam;
use glam::{Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{Material, StandardMaterial};
use hamlet_engine_bevy::ui::{StatusItem, Tone, set_status};

use crate::build::mesh::{Line, Mb};
use crate::build::stock::rgba;
use crate::build::upload::{Draw, Unsent};
use crate::folk::chief::Chief;
use crate::folk::keeper::Robots;
use crate::fx::ticked;
use crate::places::library::{self, Library};
use crate::society::{RoadLevel, Society};
use crate::tuning::WorldTuning;
use crate::view::camera::Camera;
use crate::view::pick::{Picking, Thing};
use crate::world::ground::Ground;
use crate::world::land::Land;
use crate::world::layout::{End, Route, plot};
use crate::world::sky::{DayPart, Sky};
use crate::world::tint;

/// The status item the overlay's words are shown in.
const READOUT: &str = "debug";

/// Height of the view in pixels until it tells its own.
const VIEW_HEIGHT: f32 = 900.0;

/// Seconds the frame time is averaged over.
const AVERAGE: f32 = 0.5;

pub(crate) fn plugin(app: &mut App) {
    if let Ok((tuning, _)) = WorldTuning::read() {
        app.add_shortcut(&tuning.debug.key, "debug");
    }
    app.init_resource::<Overlay>()
        .add_command("debug", "Debug Overlay", toggle)
        .add_menu_entry("View", "debug")
        .add_growing_status_item(READOUT)
        .add_systems(Update, show.run_if(ticked));
}

/// The debug overlay. Only what the realm draws is read.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct DebugSettings {
    pub shown: bool,
    /// The key that shows and hides it.
    pub key: String,
    /// The room of everything the pointer can hit, and where the
    /// pointer's ray meets them.
    pub hit_boxes: bool,
    /// The line each road follows.
    pub routes: bool,
    /// The room and the door of every plot.
    pub plots: bool,
    /// The frame time.
    pub frame_time: bool,
    /// Width of its lines, in pixels.
    pub line_width: f32,
    /// Height of its lines above the ground.
    pub lift: f32,
    /// Radius of a mark at a point.
    pub mark: f32,
    #[serde(with = "tint")]
    pub hit: Vec3,
    #[serde(with = "tint")]
    pub hovered: Vec3,
    #[serde(with = "tint")]
    pub selected: Vec3,
    #[serde(with = "tint")]
    pub route: Vec3,
    #[serde(with = "tint")]
    pub plot: Vec3,
    /// A plot nobody has.
    #[serde(with = "tint")]
    pub unused: Vec3,
}

/// Whether the overlay is shown, and what of it is drawn.
#[derive(Resource, Default)]
struct Overlay {
    /// Shown or hidden by hand; until then as the tuning has it.
    shown: Option<bool>,
    drawn: Option<Drawn>,
    /// The view's height in pixels.
    height: Option<f32>,
    /// Seconds and frames since the frame time was last worked out,
    /// and what it was, in milliseconds.
    frames: (f32, u32),
    ms: f32,
    /// The words last shown.
    said: String,
}

/// What of the overlay is in the scene.
struct Drawn {
    root: Entity,
    lines: Handle<Material>,
    /// The roads and the plots, and what they were drawn for.
    fixed: Entity,
    fixed_for: Option<u64>,
    /// The rooms of the things and the pointer's hits.
    moving: Entity,
}

/// What the overlay shows.
#[derive(SystemParam)]
struct Seen<'w> {
    tuning: Res<'w, WorldTuning>,
    land: Res<'w, Land>,
    society: Res<'w, Society>,
    ground: Res<'w, Ground>,
    robots: Res<'w, Robots>,
    chief: Res<'w, Chief>,
    library: Res<'w, Library>,
    picking: Res<'w, Picking>,
    camera: Res<'w, Camera>,
    sky: Res<'w, Sky>,
    part: Res<'w, DayPart>,
}

/// Shows the overlay, or hides it.
fn toggle(
    mut overlay: ResMut<Overlay>,
    tuning: Option<Res<WorldTuning>>,
) {
    let was = overlay
        .shown
        .or(tuning.map(|tuning| tuning.debug.shown))
        .unwrap_or(false);
    overlay.shown = Some(!was);
}

/// Draws the overlay while it is shown, and takes it away once it is
/// not.
fn show(
    seen: Seen,
    time: Res<Time>,
    mut resized: MessageReader<SceneResize>,
    mut overlay: ResMut<Overlay>,
    mut draw: Draw,
) {
    let view = seen.camera.view();
    for resize in resized.read().filter(|resize| resize.view == view)
    {
        overlay.height = Some(resize.height);
    }
    overlay.frames.0 += time.delta_secs();
    overlay.frames.1 += 1;
    if overlay.frames.0 >= AVERAGE {
        overlay.ms = overlay.frames.0 / overlay.frames.1 as f32 * 1e3;
        overlay.frames = (0.0, 0);
    }
    let set = &seen.tuning.debug;
    if !overlay.shown.unwrap_or(set.shown) {
        if let Some(drawn) = overlay.drawn.take() {
            draw.despawn(drawn.root);
        }
        overlay.say(String::new());
        return;
    }
    let eye = seen.camera.eye(&seen.land);
    let lens = &seen.tuning.camera;
    let height = overlay.height.unwrap_or(VIEW_HEIGHT).max(1.0);
    let wide =
        set.line_width * 2.0 * (lens.fov.to_radians() * 0.5).tan()
            / height;
    let drawn = match overlay.drawn.take() {
        Some(drawn) => drawn,
        None => match Drawn::make(&mut draw) {
            Ok(drawn) => drawn,
            Err(why) => {
                hamlet_engine_bevy::warn(&format!("debug: {why}"));
                overlay.shown = Some(false);
                return;
            }
        },
    };
    let mut drawn = drawn;
    let fixed_for = seen.fixed_for(eye);
    if drawn.fixed_for != Some(fixed_for) && draw.left() {
        let mut pen = Pen::new(eye, wide);
        seen.roads(&mut pen, eye);
        seen.plots(&mut pen, eye);
        if drawn.lay(drawn.fixed, pen, &mut draw) {
            drawn.fixed_for = Some(fixed_for);
        }
    }
    let mut pen = Pen::new(eye, wide);
    let under = seen.hits(&mut pen, eye);
    drawn.lay(drawn.moving, pen, &mut draw);
    overlay.drawn = Some(drawn);
    let mut words = Vec::new();
    if set.frame_time {
        words.push(format!("{:.1} ms", overlay.ms));
    }
    words.push(format!("under the pointer: {under}"));
    words.push(format!(
        "hovered: {:?}   selected: {:?}",
        seen.picking.hovered(),
        seen.picking.selected()
    ));
    let part = *seen.part;
    words.push(format!(
        "sky: {:.1}h {part:?}{}{}",
        seen.sky.hour,
        if part.dark() { ", dark" } else { "" },
        if part.low_sun() { ", low sun" } else { "" },
    ));
    overlay.say(words.join("   "));
}

impl Overlay {
    /// Shows `words` on the status line, none to hide it.
    fn say(&mut self, words: String) {
        if self.said != words {
            set_status(
                READOUT,
                &StatusItem {
                    icon: String::new(),
                    text: words.clone(),
                    tone: Tone::Dim,
                    tip: String::new(),
                },
            );
            self.said = words;
        }
    }
}

impl Drawn {
    fn make(draw: &mut Draw) -> Result<Self, String> {
        let lines = draw.material(StandardMaterial {
            unlit: true,
            double_sided: true,
            fog_enabled: false,
            ..StandardMaterial::colored(rgba(Vec3::ONE))
        })?;
        let root = draw.spawn(None, 0);
        let fixed = draw.spawn(Some(root), 0);
        let moving = draw.spawn(Some(root), 0);
        for entity in [fixed, moving] {
            draw.shadows(entity, false, false);
        }
        Ok(Self {
            root,
            lines,
            fixed,
            fixed_for: None,
            moving,
        })
    }

    /// Has `entity` draw what `pen` drew. False when the frame has no
    /// room left for it.
    fn lay(&self, entity: Entity, pen: Pen, draw: &mut Draw) -> bool {
        match draw.mesh(pen.mb.data()) {
            Ok(mesh) => draw.draw(entity, &mesh, &self.lines),
            Err(Unsent::Full(_)) => return false,
            Err(Unsent::Refused(why)) => {
                hamlet_engine_bevy::warn(&format!("debug: {why}"));
            }
        }
        true
    }
}

impl Seen<'_> {
    /// A number for what the roads and plots are drawn for: the
    /// society, the land, the roads found between towns and, roughly,
    /// where the camera stands.
    fn fixed_for(&self, eye: Vec3) -> u64 {
        let set = &self.tuning.debug;
        let (view, _) = self.camera.views();
        let step = (view.dist * 0.25).max(1.0);
        let mut hasher = DefaultHasher::new();
        (self.society.shape(), self.land.places()).hash(&mut hasher);
        self.ground.lanes().lines().count().hash(&mut hasher);
        (eye / step)
            .round()
            .to_array()
            .map(f32::to_bits)
            .hash(&mut hasher);
        (set.routes, set.plots).hash(&mut hasher);
        hasher.finish()
    }

    /// The line of each road near the camera.
    fn roads(&self, pen: &mut Pen, eye: Vec3) {
        let set = &self.tuning.debug;
        if !set.routes {
            return;
        }
        let (land, society) = (&*self.land, &*self.society);
        let far = self.tuning.detail.town_far;
        let colour = |level: Option<RoadLevel>| match level {
            Some(RoadLevel::Overgrown) => set.plot,
            _ => set.route,
        };
        let lift = Vec3::Y * set.lift;
        let mut draw = |line: &[Vec2], level| {
            let near = line.first().is_some_and(|p| {
                land.on_ground(*p).distance(eye) < far
            });
            if near {
                pen.strip(
                    line.iter().map(|p| land.on_ground(*p) + lift),
                    colour(level),
                );
            }
        };
        for (pair, line) in self.ground.lanes().lines() {
            draw(line, society.road(pair).map(|road| road.level));
        }
        let end = |agent| {
            let agent = society.agent(agent)?;
            Some(End {
                site: society.town(agent.town)?.site,
                plot: agent.plot,
            })
        };
        for road in &society.roads {
            let (Some(a), Some(b)) =
                (end(road.pair.0), end(road.pair.1))
            else {
                continue;
            };
            let at = land.on_ground(land.sites[a.site].at);
            if a.site != b.site || at.distance(eye) > far {
                continue;
            }
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            let line =
                Line::spline(&Route::way(land, a, b, &[]), 0.5)
                    .pts
                    .into_iter()
                    .map(|p| p.xz())
                    .collect::<Vec<_>>();
            draw(&line, Some(road.level));
        }
    }

    /// The room and the door of every plot of the towns near the
    /// camera.
    fn plots(&self, pen: &mut Pen, eye: Vec3) {
        let set = &self.tuning.debug;
        if !set.plots {
            return;
        }
        let (land, society) = (&*self.land, &*self.society);
        let lift = Vec3::Y * set.lift;
        for town in &society.towns {
            let centre = land.on_ground(land.sites[town.site].at);
            if centre.distance(eye) > self.tuning.detail.town_far {
                continue;
            }
            for k in 0..land.set.town.plots {
                let spot = plot(land, town.site, k);
                if !spot.usable {
                    continue;
                }
                let taken = society
                    .citizens(town.id)
                    .any(|agent| agent.plot == k);
                let colour =
                    if taken { set.plot } else { set.unused };
                let at = land.on_ground(spot.at) + lift;
                pen.ring(
                    at,
                    land.set.town.house_radius,
                    FLAT,
                    colour,
                );
                pen.line(
                    at,
                    land.on_ground(spot.door()) + lift,
                    colour,
                );
            }
        }
    }

    /// The room each thing near the camera takes for the pointer, as
    /// its least and most corners.
    fn rooms(&self, eye: Vec3) -> Vec<(Thing, Vec3, Vec3)> {
        let (pick, chief) = (&self.tuning.pick, &self.tuning.chief);
        let detail = &self.tuning.detail;
        let land = &*self.land;
        let upright = |at: Vec3, (radius, height): (f32, f32)| {
            let half = Vec3::new(radius, 0.0, radius);
            (at - half, at + half + Vec3::Y * height)
        };
        let boxed = |at: Vec3, size: Vec3| {
            let half = Vec3::new(size.x, 0.0, size.z) * 0.5;
            (at - half, at + half + Vec3::Y * size.y)
        };
        let mut rooms = Vec::new();
        let mut room = |thing, (lo, hi): (Vec3, Vec3), far: f32| {
            if ((lo + hi) * 0.5).distance(eye) < far {
                rooms.push((thing, lo, hi));
            }
        };
        for agent in
            self.society.agents.iter().filter(|a| !a.archived)
        {
            if let Some(at) = self.robots.at(agent.id) {
                room(
                    Thing::Robot(agent.id),
                    boxed(at, pick.robot_box),
                    detail.robots_far,
                );
            }
            if let Some(town) = self.society.town(agent.town) {
                let at = plot(land, town.site, agent.plot).at;
                room(
                    Thing::House(agent.id),
                    upright(land.on_ground(at), pick.house),
                    detail.town_far,
                );
            }
        }
        for town in &self.society.towns {
            let at = land.on_ground(land.sites[town.site].at);
            room(
                Thing::Common(town.id),
                upright(at, pick.common),
                detail.town_far,
            );
        }
        if let Some(at) = self.chief.mascot() {
            room(
                Thing::Mascot,
                boxed(at, chief.mascot_box),
                detail.robots_far,
            );
        }
        if let Some(at) = self.chief.hearth() {
            room(
                Thing::Hearth,
                upright(at, chief.house),
                detail.town_far,
            );
        }
        if let Some(at) = self.library.at() {
            room(
                Thing::Library,
                upright(at, library::SIZE),
                detail.town_far,
            );
        }
        rooms
    }

    /// Draws the rooms of the things and where the pointer's ray
    /// meets them and the ground, and says what it meets.
    fn hits(&self, pen: &mut Pen, eye: Vec3) -> String {
        let set = &self.tuning.debug;
        if !set.hit_boxes {
            return "nothing that can be hit".to_owned();
        }
        let rooms = self.rooms(eye);
        let (hovered, selected) =
            (self.picking.hovered(), self.picking.selected());
        for (thing, lo, hi) in &rooms {
            let colour = if selected == Some(*thing) {
                set.selected
            } else if hovered == Some(*thing) {
                set.hovered
            } else {
                set.hit
            };
            pen.room(*lo, *hi, colour);
        }
        let Some((from, way)) = self.camera.ray(&self.tuning.camera)
        else {
            return "nothing that can be hit".to_owned();
        };
        let mut met = rooms
            .iter()
            .filter_map(|(thing, lo, hi)| {
                let (depth, normal) = enter(from, way, *lo, *hi)?;
                Some((depth, normal, Some(*thing)))
            })
            .collect::<Vec<_>>();
        if let Some(depth) =
            ground_hit(&self.land, from, way, self.tuning.camera.far)
        {
            let at = from + way * depth;
            met.push((depth, self.land.normal(at.xz()), None));
        }
        met.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (nth, (depth, normal, _)) in met.iter().enumerate() {
            let colour = if nth == 0 { set.hovered } else { set.hit };
            let at = from + way * *depth;
            for axis in [FLAT, (Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z)]
            {
                pen.ring(at, set.mark, axis, colour);
            }
            pen.arrow(at, at + *normal * set.mark * 4.0, colour);
        }
        let named = met
            .iter()
            .filter_map(|(depth, _, thing)| {
                Some(format!("{:?} at {depth:.1}", (*thing)?))
            })
            .collect::<Vec<_>>();
        if named.is_empty() {
            "nothing that can be hit".to_owned()
        } else {
            named.join(", ")
        }
    }
}

/// The plane of a ring lying on the ground, by two of its axes.
const FLAT: (Vec3, Vec3) = (Vec3::X, Vec3::Z);

/// Lines drawn as two thin strips crossed along them, about as wide
/// on the screen near as far.
struct Pen {
    mb: Mb,
    eye: Vec3,
    /// Width of a line one unit from the eye.
    wide: f32,
}

impl Pen {
    fn new(eye: Vec3, wide: f32) -> Self {
        Self {
            mb: Mb::new(),
            eye,
            wide,
        }
    }

    fn line(&mut self, a: Vec3, b: Vec3, colour: Vec3) {
        let Some(along) = (b - a).try_normalize() else {
            return;
        };
        let half =
            self.wide * ((a + b) * 0.5).distance(self.eye) * 0.5;
        let side =
            along.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        for across in [side, along.cross(side)] {
            let off = across * half;
            let normal = across.cross(along);
            let ids = [a - off, a + off, b + off, b - off]
                .map(|p| self.mb.v(p, normal, colour));
            self.mb.quad(ids[0], ids[1], ids[2], ids[3]);
        }
    }

    fn strip(
        &mut self,
        points: impl IntoIterator<Item = Vec3>,
        colour: Vec3,
    ) {
        let mut last = None;
        for p in points {
            if let Some(from) = last {
                self.line(from, p, colour);
            }
            last = Some(p);
        }
    }

    /// A ring about `at` in the plane of `axes`.
    fn ring(
        &mut self,
        at: Vec3,
        radius: f32,
        axes: (Vec3, Vec3),
        colour: Vec3,
    ) {
        let point = |k: usize| {
            let (s, c) = (k as f32 / 24.0 * TAU).sin_cos();
            at + (axes.0 * c + axes.1 * s) * radius
        };
        self.strip((0..=24).map(point), colour);
    }

    /// The edges of the box from `lo` to `hi`.
    fn room(&mut self, lo: Vec3, hi: Vec3, colour: Vec3) {
        let corner = |k: usize| {
            Vec3::new(
                if k & 1 == 0 { lo.x } else { hi.x },
                if k & 2 == 0 { lo.y } else { hi.y },
                if k & 4 == 0 { lo.z } else { hi.z },
            )
        };
        for k in 0..8 {
            for bit in [1, 2, 4] {
                if k & bit == 0 {
                    self.line(corner(k), corner(k | bit), colour);
                }
            }
        }
    }

    fn arrow(&mut self, from: Vec3, to: Vec3, colour: Vec3) {
        self.line(from, to, colour);
        let Some(back) = (from - to).try_normalize() else {
            return;
        };
        let side =
            back.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let head = from.distance(to) * 0.25;
        for turn in [side, -side] {
            self.line(to, to + (back + turn * 0.5) * head, colour);
        }
    }
}

/// How far along `way` from `from` the ray enters the box from `lo`
/// to `hi`, and the normal of the side it enters by. None for a box
/// it misses or begins in.
fn enter(
    from: Vec3,
    way: Vec3,
    lo: Vec3,
    hi: Vec3,
) -> Option<(f32, Vec3)> {
    let step = way.recip();
    let (a, b) = ((lo - from) * step, (hi - from) * step);
    let (near, far) = (a.min(b), a.max(b));
    let (depth, out) = (near.max_element(), far.min_element());
    if !(depth.is_finite() && depth > 0.0 && depth <= out) {
        return None;
    }
    let axis = (0..3)
        .max_by(|i, j| near[*i].total_cmp(&near[*j]))
        .unwrap_or(1);
    let mut normal = Vec3::ZERO;
    normal[axis] = -way[axis].signum();
    Some((depth, normal))
}

/// How far along `way` from `from` the ray meets the ground as it is
/// drawn, within `far`.
fn ground_hit(
    land: &Land,
    from: Vec3,
    way: Vec3,
    far: f32,
) -> Option<f32> {
    let over = |t: f32| {
        let p = from + way * t;
        p.y - land.surface(p.xz())
    };
    let (mut last, mut t) = (0.0, 0.0);
    for _ in 0..256 {
        let above = over(t);
        if above <= 0.0 {
            let (mut dry, mut wet) = (last, t);
            for _ in 0..12 {
                let mid = (dry + wet) * 0.5;
                if over(mid) > 0.0 {
                    dry = mid;
                } else {
                    wet = mid;
                }
            }
            return Some(wet);
        }
        last = t;
        t += (above * 0.5).max(0.05);
        if t > far {
            return None;
        }
    }
    None
}
