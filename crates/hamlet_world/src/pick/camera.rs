//! The camera moved by hand: sliding, turning, zooming, and staying
//! on a thing.

use bevy::input::mouse::MouseScrollUnit;
use bevy::prelude::*;

use super::{Selected, Taken, Thing, Travel, WorldSurface, select};
use crate::play::{Play, Played};
use crate::sky::CameraSettings;

/// The camera moved by hand, on top of the stage's view.
#[derive(Resource, Clone, Copy, PartialEq)]
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
        Orbit {
            pan: Vec2::ZERO,
            zoom: 1.0,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// The orbit as the camera shows it, which follows [`Orbit`]
/// smoothly.
#[derive(Resource, Clone, Copy, PartialEq, Default)]
pub struct Glide(pub Orbit);

/// The point on the ground the orbit's pan is measured from.
#[derive(Resource, Clone, Copy, PartialEq, Default)]
pub struct Centre(pub Vec2);

/// The camera's stand as it was last aimed: its distance from the
/// point it looks at and its angle below the horizon, in radians.
/// Nothing until it has been aimed.
#[derive(Resource, Clone, Copy, PartialEq, Default)]
pub struct Stance(pub Option<(f32, f32)>);

/// The reach of a hand on the camera. With nothing in it, the
/// camera's own settings hold.
#[derive(Resource, Clone, PartialEq, Default, Debug)]
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

/// A thing the camera stays on, until the ground is slid by hand.
#[derive(Resource, Default, PartialEq)]
pub struct Following(pub Option<Thing>);

/// The systems that move the camera by hand, ahead of whatever aims
/// it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Steering;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Orbit>()
        .init_resource::<Glide>()
        .init_resource::<Centre>()
        .init_resource::<Stance>()
        .init_resource::<Following>()
        .init_resource::<Leash>()
        .init_resource::<Steer>()
        .add_observer(drag)
        .add_observer(drag_end)
        .add_observer(scroll)
        .add_systems(
            Update,
            (focus_keys, follow, glide).chain().in_set(Steering),
        );
}

/// The place of `thing` on the ground.
fn place(
    thing: Thing,
    things: &Query<(&Thing, &GlobalTransform)>,
) -> Option<Vec2> {
    things
        .iter()
        .find(|(of, _)| **of == thing)
        .map(|(_, at)| at.translation().xz())
}

/// Brings `thing` to the middle of the view and comes in close, and
/// stays on it if `stay`.
pub fn focus_on(world: &mut World, thing: Thing, stay: bool) {
    let at = world
        .query::<(&Thing, &GlobalTransform)>()
        .iter(world)
        .find(|(of, _)| **of == thing)
        .map(|(_, at)| at.translation().xz());
    let Some(at) = at else {
        return;
    };
    if let Some(mut played) =
        world.get_resource_mut::<Messages<Played>>()
    {
        played.write(Played {
            what: Play::Focus,
            at: Vec3::new(at.x, 0.0, at.y),
        });
    }
    let centre = world.resource::<Centre>().0;
    let set = world.resource::<CameraSettings>();
    let leash = world.resource::<Leash>();
    let pan = leash.held(set, centre, at - centre);
    // As close as F comes with one hamlet, however far apart the
    // hamlets lie.
    let close = set.focus_zoom
        * leash.zoom.map_or(1.0, |(least, _)| least / set.zoom.0);
    let mut orbit = world.resource_mut::<Orbit>();
    orbit.pan = pan;
    orbit.zoom = orbit.zoom.min(close);
    world.resource_mut::<Following>().0 = stay.then_some(thing);
}

/// F brings the selected thing to the middle and comes in close,
/// Shift and F stays on it, and Escape lets it go. Nothing happens
/// while something is being typed.
fn focus_keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    typing: Option<Res<bevy::input_focus::InputFocus>>,
    selected: Res<Selected>,
    taken: Res<Taken>,
    mut commands: Commands,
    mut following: ResMut<Following>,
) {
    let Some(keys) = keys else {
        return;
    };
    if typing.is_some_and(|focus| focus.get().is_some()) {
        return;
    }
    // Escape lets go of what is followed, and then of what is
    // selected.
    if keys.just_pressed(KeyCode::Escape)
        && !taken.escape
        && !taken.over
    {
        if following.0.is_some() {
            following.0 = None;
        } else if selected.0.is_some() {
            commands.queue(|world: &mut World| select(world, None));
        }
    }
    let Some(thing) =
        selected.0.filter(|_| keys.just_pressed(KeyCode::KeyF))
    else {
        return;
    };
    let stay =
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    commands.queue(move |world: &mut World| {
        focus_on(world, thing, stay);
    });
}

/// Keeps the view on the thing that is followed.
fn follow(
    following: Res<Following>,
    centre: Res<Centre>,
    camera: Res<CameraSettings>,
    leash: Res<Leash>,
    things: Query<(&Thing, &GlobalTransform)>,
    mut orbit: ResMut<Orbit>,
) {
    let Some(at) =
        following.0.and_then(|thing| place(thing, &things))
    else {
        return;
    };
    let pan = leash.held(&camera, centre.0, at - centre.0);
    if orbit.pan != pan {
        orbit.pan = pan;
    }
}

/// Eases the orbit that is shown toward the one that was asked for.
fn glide(
    time: Res<Time>,
    camera: Res<CameraSettings>,
    orbit: Res<Orbit>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut shown: ResMut<Glide>,
    mut redraw: MessageWriter<bevy::window::RequestRedraw>,
) {
    if shown.0 == *orbit {
        return;
    }
    let k = 1.0 - (-camera.glide * time.delta_secs()).exp();
    // A hand on the ground is followed closely, but not in steps.
    let held = buttons.pressed(MouseButton::Left);
    let rate = if held { camera.slide } else { camera.glide };
    let slid = 1.0 - (-rate * time.delta_secs()).exp();
    let from = shown.0;
    // Zoom is eased by its ratio, so it feels the same far and near.
    let zoom = (from.zoom.ln()
        + (orbit.zoom.ln() - from.zoom.ln()) * k)
        .exp();
    let next = Orbit {
        pan: from.pan.lerp(orbit.pan, slid),
        zoom,
        yaw: from.yaw + (orbit.yaw - from.yaw) * k,
        pitch: from.pitch + (orbit.pitch - from.pitch) * k,
    };
    let near = next.pan.distance(orbit.pan) < 0.01
        && (next.zoom / orbit.zoom - 1.0).abs() < 0.001
        && (next.yaw - orbit.yaw).abs() < 0.0005
        && (next.pitch - orbit.pitch).abs() < 0.0005;
    shown.0 = if near { *orbit } else { next };
    redraw.write(bevy::window::RequestRedraw);
}

/// The way a drag on the world moves the camera.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Steer {
    #[default]
    Still,
    /// The ground slides under the pointer.
    Slide,
    /// The view turns and tilts.
    Turn,
}

/// The left button slides the ground, the right button turns and
/// tilts.
pub(super) fn drag(
    drag: On<Pointer<Drag>>,
    surfaces: Query<&ComputedNode, With<WorldSurface>>,
    camera: Res<CameraSettings>,
    stance: Res<Stance>,
    taken: Res<Taken>,
    leash: Res<Leash>,
    centre: Res<Centre>,
    mut travel: ResMut<Travel>,
    mut orbit: ResMut<Orbit>,
    mut following: ResMut<Following>,
    mut steer: ResMut<Steer>,
) {
    let Ok(node) = surfaces.get(drag.event_target()) else {
        return;
    };
    travel.0 += drag.delta.length();
    match drag.button {
        PointerButton::Primary if taken.drag => {}
        PointerButton::Primary => {
            steer.set_if_neq(Steer::Slide);
            // Sliding the ground by hand lets go of what was
            // followed.
            if following.0.is_some() {
                following.0 = None;
            }
            let height = (node.size().y
                * node.inverse_scale_factor())
            .max(1.0);
            // The ground under the middle of the view moves as far
            // as the pointer does. A pixel is this much of the
            // ground across the view, and more along it, where the
            // ground lies tilted away.
            let half = (camera.fov.to_radians() * 0.5).tan();
            let (across, along) = match stance.0 {
                Some((far, down)) => {
                    let pixel = 2.0 * far * half / height;
                    (pixel, pixel / down.sin().max(0.2))
                }
                None => {
                    let pixel = camera.span() * orbit.zoom / height;
                    (pixel, pixel * camera.slide_tilt)
                }
            };
            let slide = Vec2::new(
                -drag.delta.x * across,
                -drag.delta.y * along,
            );
            let turned = Vec2::from_angle(-orbit.yaw).rotate(slide);
            orbit.pan =
                leash.held(&camera, centre.0, orbit.pan + turned);
        }
        PointerButton::Secondary => {
            steer.set_if_neq(Steer::Turn);
            orbit.yaw -= drag.delta.x * camera.turn_rate;
            orbit.pitch = (orbit.pitch
                + drag.delta.y * camera.tilt_rate)
                .clamp(camera.tilt.0, camera.tilt.1);
        }
        PointerButton::Middle => {}
    }
}

fn drag_end(_: On<Pointer<DragEnd>>, mut steer: ResMut<Steer>) {
    steer.set_if_neq(Steer::Still);
}

fn scroll(
    scroll: On<Pointer<Scroll>>,
    surfaces: Query<(), With<WorldSurface>>,
    camera: Res<CameraSettings>,
    leash: Res<Leash>,
    mut orbit: ResMut<Orbit>,
) {
    if !surfaces.contains(scroll.event_target()) {
        return;
    }
    let (least, most) = leash.zoom(&camera);
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.y,
        MouseScrollUnit::Pixel => scroll.y / camera.scroll_line,
    };
    orbit.zoom = (orbit.zoom * (-lines * camera.scroll_rate).exp())
        .clamp(least, most.max(least));
}
