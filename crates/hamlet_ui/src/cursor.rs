//! The pointer (docs/cursors.md): the chief's taper, whose flame is
//! the point, and the chief's glove for what is held.

mod art;
#[cfg(test)]
mod tests;

use bevy::app::{App, PostUpdate, Startup, Update};
use bevy::asset::{Assets, Handle};
use bevy::ecs::entity::Entity;
use bevy::ecs::query::{With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::input::ButtonInput;
use bevy::input::mouse::MouseButton;
use bevy::time::{Real, Time};
use bevy::window::{
    CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow,
    SystemCursorIcon,
};
use bevy_fynix::EntityCursor;
use hamlet_world::hand::{Grip, Hand};
use hamlet_world::pick::{Hovered, Pressable, Steer, Taken, Thing};

pub use self::art::{Inks, face, frames, hot, side};
use crate::theme::{
    BLACK, BRASS_1, BRASS_2, BRASS_4, COAL, EMBER, FLAME, LAMP, RUNE,
};
use crate::welcome::Committing;

/// The share of a cursor that is left while the hand carries a thing
/// of the world, so that the thing is seen through it.
const FAINT: f32 = 0.4;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, draw)
        .add_systems(Update, lend)
        .add_systems(PostUpdate, dress);
}

/// The line a cursor that resizes lies along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// West to east.
    Across,
    /// North to south.
    Upright,
    /// North-west to south-east.
    Falling,
    /// South-west to north-east.
    Rising,
}

/// The deed the pointer says can be done where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Point,
    /// Over what can be pressed.
    Press,
    /// Over a thing of the world that can be selected.
    Pick,
    /// Over what can be taken hold of.
    Grab,
    /// Holding something.
    Carry,
    /// Sliding the ground.
    Slide,
    /// Turning the view.
    Turn,
    /// Over text that can be typed in.
    Write,
    Resize(Axis),
    /// The deed asked for is not allowed here.
    Refuse,
    /// The app is about to be busy.
    Wait,
}

/// All that the cursor is chosen from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Facts {
    pub busy: bool,
    pub grip: Option<Grip>,
    pub steer: Steer,
    /// The cursor the panels ask for.
    pub asked: Option<SystemCursorIcon>,
    /// The thing of the world under the pointer.
    pub over: Option<Thing>,
    /// The pointer is on something of the world that takes its
    /// click.
    pub claimed: bool,
}

impl Mode {
    pub fn all() -> Vec<Mode> {
        let mut all = vec![
            Mode::Point,
            Mode::Press,
            Mode::Pick,
            Mode::Grab,
            Mode::Carry,
            Mode::Slide,
            Mode::Turn,
            Mode::Write,
        ];
        all.extend(
            [
                Axis::Across,
                Axis::Upright,
                Axis::Falling,
                Axis::Rising,
            ]
            .map(Mode::Resize),
        );
        all.extend([Mode::Refuse, Mode::Wait]);
        all
    }

    /// The mode a cursor of the system stands for. None for the
    /// plain arrow and for what means nothing here.
    pub(crate) fn of_system(icon: SystemCursorIcon) -> Option<Mode> {
        use SystemCursorIcon as S;
        Some(match icon {
            S::Pointer => Mode::Press,
            S::Cell | S::Crosshair => Mode::Pick,
            S::Grab => Mode::Grab,
            S::Grabbing => Mode::Carry,
            S::Move | S::AllScroll => Mode::Slide,
            S::Text | S::VerticalText => Mode::Write,
            S::EResize | S::WResize | S::EwResize | S::ColResize => {
                Mode::Resize(Axis::Across)
            }
            S::NResize | S::SResize | S::NsResize | S::RowResize => {
                Mode::Resize(Axis::Upright)
            }
            S::NwResize | S::SeResize | S::NwseResize => {
                Mode::Resize(Axis::Falling)
            }
            S::NeResize | S::SwResize | S::NeswResize => {
                Mode::Resize(Axis::Rising)
            }
            S::NoDrop | S::NotAllowed => Mode::Refuse,
            S::Wait | S::Progress => Mode::Wait,
            _ => return None,
        })
    }

    /// The mode for `facts`: the app's waiting first, then the hand's
    /// hold on the world, then the cursor the panels ask for, then
    /// the thing under the pointer in the world.
    pub fn choose(facts: &Facts) -> Mode {
        if facts.busy {
            return Mode::Wait;
        }
        if facts.grip == Some(Grip::Robot) {
            return Mode::Carry;
        }
        match facts.steer {
            Steer::Slide => return Mode::Slide,
            Steer::Turn => return Mode::Turn,
            Steer::Still => {}
        }
        if let Some(mode) = facts.asked.and_then(Mode::of_system) {
            return mode;
        }
        if facts.grip.is_some() {
            Mode::Grab
        } else if facts.claimed {
            Mode::Press
        } else if facts.over.is_some() {
            Mode::Pick
        } else {
            Mode::Point
        }
    }
}

/// The inks of the panels' own theme.
pub fn inks() -> Inks {
    Inks {
        flame: FLAME,
        lamp: LAMP,
        ember: EMBER,
        wax: BRASS_1,
        shade: BRASS_2,
        brass: BRASS_4,
        rune: RUNE,
        coal: COAL,
        edge: BLACK,
    }
}

/// The pictures of every mode, and the same again made faint.
#[derive(Resource)]
pub(crate) struct Cursors(Vec<(Mode, [Vec<Handle<Image>>; 2])>);

impl Cursors {
    /// The picture of `mode` at `seconds`, the faint one if `faint`.
    fn at(
        &self,
        mode: Mode,
        seconds: f32,
        faint: bool,
    ) -> Option<&Handle<Image>> {
        let (_, both) = self.0.iter().find(|(of, _)| *of == mode)?;
        let faces = &both[faint as usize];
        // Ten pictures a second.
        faces.get((seconds * 10.0) as usize % faces.len())
    }
}

/// Leaves [`FAINT`] of every pixel of a picture.
fn fade(image: &mut Image) {
    for pixel in image.data.iter_mut().flatten().skip(3).step_by(4) {
        *pixel = (*pixel as f32 * FAINT) as u8;
    }
}

/// Draws every cursor.
pub fn draw(world: &mut World) {
    let inks = inks();
    let mut images = world.resource_mut::<Assets<Image>>();
    let cursors = Mode::all()
        .into_iter()
        .map(|mode| {
            let mut faces = |faint: bool| {
                (0..frames(mode))
                    .map(|frame| {
                        let mut image = face(mode, frame, &inks);
                        if faint {
                            fade(&mut image);
                        }
                        images.add(image)
                    })
                    .collect::<Vec<_>>()
            };
            (mode, [faces(false), faces(true)])
        })
        .collect::<Vec<_>>();
    world.insert_resource(Cursors(cursors));
}

/// Has every node over the world that takes a click ask for the
/// pressing cursor, as a button of the panels does.
pub(crate) fn lend(
    mut commands: Commands,
    nodes: Query<Entity, (With<Pressable>, Without<EntityCursor>)>,
) {
    for node in &nodes {
        commands
            .entity(node)
            .try_insert(EntityCursor(SystemCursorIcon::Pointer));
    }
}

/// Shows the cursor of the mode that holds. Fynix puts a cursor of
/// the system on the window each frame it wants one, and that is read
/// here as the panels' asking.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dress(
    cursors: Res<Cursors>,
    time: Res<Time<Real>>,
    pending: Option<Res<Committing>>,
    hand: Option<Res<Hand>>,
    steer: Option<Res<Steer>>,
    hovered: Option<Res<Hovered>>,
    taken: Option<Res<Taken>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<
        (Entity, Option<&CursorIcon>),
        With<PrimaryWindow>,
    >,
    mut commands: Commands,
) {
    // A drag that never ended does not hold the cursor.
    let down = |button: MouseButton| {
        buttons.as_ref().is_none_or(|all| all.pressed(button))
    };
    let steer = match steer.map(|steer| *steer) {
        Some(Steer::Slide) if down(MouseButton::Left) => Steer::Slide,
        Some(Steer::Turn) if down(MouseButton::Right) => Steer::Turn,
        _ => Steer::Still,
    };
    for (window, shown) in &windows {
        let asked = match shown {
            Some(CursorIcon::System(icon)) => Some(*icon),
            _ => None,
        };
        let grip = hand.as_ref().and_then(|hand| hand.grip());
        // A robot in the hand is seen through it.
        let faint = grip == Some(Grip::Robot);
        let mode = Mode::choose(&Facts {
            busy: pending.as_ref().is_some_and(|p| p.waits()),
            grip,
            steer,
            asked,
            over: hovered.as_ref().and_then(|hovered| hovered.0),
            claimed: taken.as_ref().is_some_and(|taken| taken.click),
        });
        let Some(image) =
            cursors.at(mode, time.elapsed_secs(), faint)
        else {
            continue;
        };
        let wanted = CursorIcon::Custom(CustomCursor::Image(
            CustomCursorImage {
                handle: image.clone(),
                hotspot: hot(mode),
                ..CustomCursorImage::default()
            },
        ));
        if shown != Some(&wanted) {
            commands.entity(window).try_insert(wanted);
        }
    }
}
