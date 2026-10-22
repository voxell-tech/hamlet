//! The camera of the Realm panel: framing the hamlets, and moved by
//! hand. The left button slides the ground, the right button turns
//! and tilts, and the wheel brings it nearer.

use glam::{Vec2, Vec3};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::MouseButton;
use hamlet_engine_bevy::ui::PointerMode;

use crate::build::frame::Frame;
use crate::build::upload::Draw;
use crate::society::Society;
use crate::view::pick::Thing;
use crate::world::ground::{DetailSettings, Sight};
use crate::world::land::Land;
use crate::world::sky::{CameraSettings, Look};

/// The key of the camera's entity, which names its view.
pub const VIEW: u32 = 1;

/// The size of the view, in the interface's pixels, until the engine
/// tells it.
const SIZE: Vec2 = Vec2::new(1280.0, 800.0);

/// The camera moved by hand, on top of the framing of the hamlets.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Orbit {
    /// Offset of the focus point along the ground.
    pub pan: Vec2,
    /// Multiplies the view's distance.
    pub zoom: f32,
    /// Turn around the focus point, in radians.
    pub yaw: f32,
    /// Added to the view's pitch, in radians.
    pub pitch: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// The reach of a hand on the camera. With nothing in it, the
/// camera's own settings hold.
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Leash {
    /// The least and the most of [`Orbit::zoom`].
    pub zoom: Option<(f32, f32)>,
    /// The places worth looking at. The focus stays within
    /// [`CameraSettings::pan`] of them and of the straight ways
    /// between them.
    pub anchors: Vec<Vec2>,
}

impl Leash {
    /// `pan`, brought back to where the focus may be. `centre` is
    /// the point it is measured from.
    pub fn held(
        &self,
        camera: &CameraSettings,
        centre: Vec2,
        pan: Vec2,
    ) -> Vec2 {
        let focus = centre + pan;
        // The nearest point of any place, or of the way from one
        // to another.
        let ways =
            self.anchors.iter().enumerate().flat_map(|(i, a)| {
                self.anchors[i..].iter().map(move |b| {
                    let along = *b - *a;
                    let share = (focus - *a).dot(along)
                        / along.length_squared().max(1e-6);
                    *a + along * share.clamp(0.0, 1.0)
                })
            });
        let nearest = ways.min_by(|a, b| {
            a.distance_squared(focus)
                .total_cmp(&b.distance_squared(focus))
        });
        match nearest {
            Some(anchor) => {
                anchor + (focus - anchor).clamp_length_max(camera.pan)
                    - centre
            }
            None => pan.clamp_length_max(camera.pan),
        }
    }

    fn zoom(&self, camera: &CameraSettings) -> (f32, f32) {
        self.zoom.unwrap_or(camera.zoom)
    }
}

/// The camera's view.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct View {
    pub focus: Vec2,
    pub dist: f32,
    pub yaw: f32,
    /// Degrees below the horizon.
    pub pitch: f32,
}

impl View {
    /// The height the camera looks at: the lie of the land about
    /// its focus, taken wide so that the camera crosses a mountain
    /// without following every rock of it.
    fn level(&self, land: &Land) -> f32 {
        const SPOKES: usize = 8;
        let reach = self.dist * 0.4;
        let mut sum = land.height(self.focus);
        for ring in [0.5, 1.0] {
            for spoke in 0..SPOKES {
                let turn = (spoke as f32 + ring) / SPOKES as f32
                    * std::f32::consts::TAU;
                sum += land.height(
                    self.focus
                        + Vec2::from_angle(turn) * reach * ring,
                );
            }
        }
        sum / (1 + 2 * SPOKES) as f32
    }

    /// The camera's place for this view, over the point it looks at
    /// at height `level`.
    fn eye(&self, level: f32) -> (Vec3, Vec3) {
        let focus =
            Vec3::new(self.focus.x, level + 0.5, self.focus.y);
        let pitch = self.pitch.to_radians();
        let back = Vec3::new(
            self.yaw.sin() * pitch.cos(),
            pitch.sin(),
            self.yaw.cos() * pitch.cos(),
        );
        (focus + self.dist * back, focus)
    }

    /// The sight of the ground a camera aimed this way has.
    pub fn sight(
        &self,
        land: &Land,
        settings: &CameraSettings,
        detail: &DetailSettings,
    ) -> Sight {
        let (eye, _) = self.eye(self.level(land));
        let whole =
            self.dist * settings.fog_end.0 + settings.fog_end.1;
        Sight {
            eye,
            centre: self.focus,
            reach: whole.min(settings.far) * detail.ground_reach,
        }
    }
}

/// The way a drag on the realm moves the camera.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Steer {
    #[default]
    Still,
    /// The ground slides under the pointer.
    Slide,
    /// The view turns and tilts.
    Turn,
}

/// The thing the camera stays on.
#[derive(Resource, Default)]
pub struct Following(pub Option<Thing>);

/// The camera, where it is asked to be and where it is shown.
#[derive(Resource)]
pub struct Camera {
    at: Entity,
    /// The orbit asked for, and the one shown, which follows it.
    orbit: Orbit,
    glide: Orbit,
    /// The point on the ground the orbit's pan is measured from.
    centre: Vec2,
    leash: Leash,
    /// The hamlets framed last: how many, their middle and the
    /// distance to see them all.
    framed: Option<(usize, Vec2, f32)>,
    /// The height it looks at, eased over the land.
    level: Option<f32>,
    /// The view it shows, and the one it is going to.
    view: View,
    ahead: Option<View>,
    steer: Steer,
    /// The pixels the pointer moved since a button last went down.
    pub travel: f32,
    /// The view's size, and the pointer over it, in the interface's
    /// pixels.
    size: Vec2,
    pointer: Option<Vec2>,
    /// Where the camera was last put.
    shown: Frame,
    /// The place followed, until the ground is slid by hand.
    pub following: Option<Vec2>,
    /// The look and distance the lens was last set for.
    lit: Option<(Look, f32)>,
}

impl Camera {
    pub fn make(
        settings: &CameraSettings,
        look: &Look,
        draw: &mut Draw,
    ) -> Self {
        let at = draw.spawn(None, VIEW);
        draw.commands.entity(at).insert(SceneCamera::viewport(
            settings.lens(look, settings.distance),
        ));
        Self {
            at,
            orbit: Orbit::default(),
            glide: Orbit::default(),
            centre: Vec2::ZERO,
            leash: Leash::default(),
            framed: None,
            level: None,
            view: View {
                focus: Vec2::ZERO,
                dist: settings.distance,
                yaw: 0.0,
                pitch: settings.pitch,
            },
            ahead: None,
            steer: Steer::Still,
            travel: 0.0,
            size: SIZE,
            pointer: None,
            shown: Frame::IDENTITY,
            following: None,
            lit: None,
        }
    }

    /// The entity of its view.
    pub fn view(&self) -> Entity {
        self.at
    }

    /// The pointer of a drag that moves it, once the pointer moved
    /// too far for a click.
    pub fn dragging(
        &self,
        camera: &CameraSettings,
    ) -> Option<PointerMode> {
        if self.travel <= camera.click_slop {
            return None;
        }
        match self.steer {
            Steer::Slide => Some(PointerMode::Slide),
            Steer::Turn => Some(PointerMode::Turn),
            Steer::Still => None,
        }
    }

    /// The view shown, and the one it is going to while it glides.
    pub fn views(&self) -> (View, Option<View>) {
        (self.view, self.ahead)
    }

    /// Where the camera stands.
    pub fn eye(&self, land: &Land) -> Vec3 {
        let level =
            self.level.unwrap_or_else(|| self.view.level(land));
        self.view.eye(level).0
    }

    /// Number of places, the towns of the hamlets and the chief's
    /// Hearth among them, the middle of them, and the distance the
    /// camera stands back at to see them all.
    fn framing(
        camera: &CameraSettings,
        land: &Land,
        society: &Society,
    ) -> Option<(usize, Vec2, f32)> {
        let sites = || {
            society
                .places()
                .filter_map(|place| society.stands(land, place))
                .map(|(at, _)| at)
        };
        let count = sites().count();
        if count == 0 {
            return None;
        }
        let middle = sites().sum::<Vec2>() / count as f32;
        let spread =
            sites().map(|at| at.distance(middle)).fold(0.0, f32::max);
        let base = (camera.distance
            + spread * camera.spread_distance)
            .min(camera.reach);
        Some((count, middle, base))
    }

    /// Frames the places as they stand, and keeps a hand from taking
    /// the camera away from every place. The same places elsewhere
    /// leave the view where it is.
    pub fn frame(
        &mut self,
        camera: &CameraSettings,
        land: &Land,
        society: &Society,
    ) {
        let Some((count, middle, base)) =
            Self::framing(camera, land, society)
        else {
            return;
        };
        self.leash = Leash {
            zoom: Some((
                camera.distance * camera.zoom.0 / base,
                camera.reach / base,
            )),
            anchors: society
                .places()
                .filter_map(|place| society.stands(land, place))
                .map(|(at, _)| at)
                .collect(),
        };
        if let Some((before, was, far)) = self.framed
            && before == count
            && (was, far) != (middle, base)
        {
            for held in [&mut self.orbit, &mut self.glide] {
                held.pan += was - middle;
                held.zoom *= far / base;
            }
        }
        self.framed = Some((count, middle, base));
        self.centre = middle;
    }

    /// The view of `orbit` over the hamlets as they are framed.
    fn seen(
        &self,
        camera: &CameraSettings,
        orbit: Orbit,
    ) -> Option<View> {
        let (_, middle, base) = self.framed?;
        Some(View {
            focus: middle + orbit.pan,
            dist: base * orbit.zoom,
            yaw: orbit.yaw,
            pitch: (camera.pitch + orbit.pitch.to_degrees())
                .clamp(camera.pitch_limits.0, camera.pitch_limits.1),
        })
    }

    /// Brings `at` to the middle of the view and comes in close, and
    /// stays on it if `stay`.
    pub fn focus_on(
        &mut self,
        camera: &CameraSettings,
        at: Vec2,
        stay: bool,
    ) {
        let pan =
            self.leash.held(camera, self.centre, at - self.centre);
        // As close as F comes with one hamlet, however far apart the
        // hamlets lie.
        let close = camera.focus_zoom
            * self
                .leash
                .zoom
                .map_or(1.0, |(least, _)| least / camera.zoom.0);
        self.orbit.pan = pan;
        self.orbit.zoom = self.orbit.zoom.min(close);
        self.following = stay.then_some(at);
    }

    /// Goes back to the framing of every hamlet.
    pub fn reset(&mut self) {
        self.orbit = Orbit::default();
        self.following = None;
    }

    /// Answers what the person did over the view.
    pub fn input(
        &mut self,
        camera: &CameraSettings,
        input: &SceneInput,
    ) {
        match input {
            SceneInput::Press(press) if press.view == self.at => {
                let steer = match press.button {
                    MouseButton::Primary => Steer::Slide,
                    MouseButton::Secondary => Steer::Turn,
                    _ => return,
                };
                if press.down {
                    self.travel = 0.0;
                    self.steer = steer;
                } else if self.steer == steer {
                    // Letting go of the other button leaves the drag
                    // as it is.
                    self.steer = Steer::Still;
                }
            }
            SceneInput::Pointer(pointer)
                if pointer.view == self.at =>
            {
                self.pointer = Some(Vec2::new(pointer.x, pointer.y));
                let delta = Vec2::new(pointer.dx, pointer.dy);
                self.travel += delta.length();
                match self.steer {
                    Steer::Slide => self.slide(camera, delta),
                    Steer::Turn => {
                        self.orbit.yaw -= delta.x * camera.turn_rate;
                        self.orbit.pitch = (self.orbit.pitch
                            + delta.y * camera.tilt_rate)
                            .clamp(camera.tilt.0, camera.tilt.1);
                    }
                    Steer::Still => {}
                }
            }
            SceneInput::Wheel(wheel) if wheel.view == self.at => {
                let (least, most) = self.leash.zoom(camera);
                self.orbit.zoom = (self.orbit.zoom
                    * (-wheel.y * camera.scroll_rate).exp())
                .clamp(least, most.max(least));
            }
            SceneInput::Resize(resize) if resize.view == self.at => {
                self.size = Vec2::new(resize.width, resize.height)
                    .max(Vec2::ONE);
            }
            _ => {}
        }
    }

    /// Slides the ground under the middle of the view as far as the
    /// pointer moved.
    fn slide(&mut self, camera: &CameraSettings, delta: Vec2) {
        // Sliding the ground by hand lets go of what was followed.
        self.following = None;
        // A pixel is this much of the ground across the view, and
        // more along it, where the ground lies tilted away.
        let half = (camera.fov.to_radians() * 0.5).tan();
        let far = self.view.dist;
        let down = self.view.pitch.to_radians();
        let pixel = 2.0 * far * half / self.size.y;
        let (across, along) = (pixel, pixel / down.sin().max(0.2));
        let slide = Vec2::new(-delta.x * across, -delta.y * along);
        let turned = Vec2::from_angle(-self.orbit.yaw).rotate(slide);
        self.orbit.pan = self.leash.held(
            camera,
            self.centre,
            self.orbit.pan + turned,
        );
    }

    /// Eases the orbit shown toward the one asked for and points the
    /// camera, with its fog fitted to its distance. True while it
    /// still moves.
    pub fn tick(
        &mut self,
        camera: &CameraSettings,
        land: &Land,
        look: &Look,
        dt: f32,
        draw: &mut Draw,
    ) -> bool {
        if let Some(at) = self.following {
            self.orbit.pan = self.leash.held(
                camera,
                self.centre,
                at - self.centre,
            );
        }
        self.glide = self.glided(camera, dt);
        let Some(next) = self.seen(camera, self.glide) else {
            return false;
        };
        self.ahead = (self.orbit != self.glide)
            .then(|| self.seen(camera, self.orbit))
            .flatten();
        self.view = next;
        let (eye, focus) = self.view.eye(self.view.level(land));
        // The height it looks at is eased to, so that it rides over
        // rough ground. A first look and a leap go there at once.
        let dist = self.view.dist;
        let eased = match self
            .level
            .filter(|was| (was - focus.y).abs() < dist * 0.5)
        {
            Some(was) => {
                let k = 1.0 - (-camera.level * dt).exp();
                was + (focus.y - was) * k
            }
            None => focus.y,
        };
        let settled = (eased - focus.y).abs() < 0.002;
        let eased = if settled { focus.y } else { eased };
        self.level = Some(eased);
        let lift = Vec3::Y * (eased - focus.y);
        let shown = Frame::looking_at(eye + lift, focus + lift);
        if shown != self.shown {
            draw.place(self.at, shown);
            self.shown = shown;
        }
        let lit = (look.clone(), dist);
        if self.lit.as_ref() != Some(&lit) {
            draw.commands.entity(self.at).insert(
                SceneCamera::viewport(camera.lens(look, dist)),
            );
            self.lit = Some(lit);
        }
        !settled || self.orbit != self.glide
    }

    /// The ray from the eye through the pointer, while it is over
    /// the view: where it starts and its way.
    pub fn ray(
        &self,
        camera: &CameraSettings,
    ) -> Option<(Vec3, Vec3)> {
        let at = self.pointer?;
        let half = (camera.fov.to_radians() * 0.5).tan();
        let ndc = Vec2::new(
            at.x / self.size.x * 2.0 - 1.0,
            1.0 - at.y / self.size.y * 2.0,
        );
        let aspect = self.size.x / self.size.y;
        let way =
            Vec3::new(ndc.x * half * aspect, ndc.y * half, -1.0);
        Some((self.shown.at, (self.shown.turn * way).normalize()))
    }

    /// The orbit shown, `dt` further toward the one asked for.
    fn glided(&self, camera: &CameraSettings, dt: f32) -> Orbit {
        let (from, to) = (self.glide, self.orbit);
        if from == to {
            return to;
        }
        let k = 1.0 - (-camera.glide * dt).exp();
        // A hand on the ground is followed closely, but not in steps.
        let rate = if self.steer == Steer::Slide {
            camera.slide
        } else {
            camera.glide
        };
        let slid = 1.0 - (-rate * dt).exp();
        // Zoom is eased by its ratio, so it feels the same far and
        // near.
        let zoom = (from.zoom.ln()
            + (to.zoom.ln() - from.zoom.ln()) * k)
            .exp();
        let next = Orbit {
            pan: from.pan.lerp(to.pan, slid),
            zoom,
            yaw: from.yaw + (to.yaw - from.yaw) * k,
            pitch: from.pitch + (to.pitch - from.pitch) * k,
        };
        let near = next.pan.distance(to.pan) < 0.01
            && (next.zoom / to.zoom - 1.0).abs() < 0.001
            && (next.yaw - to.yaw).abs() < 0.0005
            && (next.pitch - to.pitch).abs() < 0.0005;
        if near { to } else { next }
    }
}
