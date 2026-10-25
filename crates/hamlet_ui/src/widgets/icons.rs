//! The icons of the panels (docs/icons.md): one family of line
//! drawings, made in code and tinted by a text tone.

use bevy::app::{App, Startup};
use bevy::asset::uuid::Uuid;
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::system::ResMut;
use bevy::image::Image;
use bevy::math::Vec2;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use bevy_fynix::dock::DockIcons;
use bevy_fynix::views::{Icon, icon};

use self::Shape::{
    Arc, Curve, Dot, Fill, Line, Loop, Path, Ring, Round,
};

/// The side of the square every icon is drawn in, in its own units.
const GRID: f32 = 16.0;

/// The sizes an icon is drawn for, in the interface's pixels.
const SIZES: [f32; 4] = [12.0, 14.0, 16.0, 20.0];

/// The pixels of an image to one of the interface's, so that an icon
/// is sharp on a display of twice the density.
const DENSITY: f32 = 2.0;

/// The handles of the icons' images start here.
const FIRST: u128 = 0x6861_6d6c_6574_5f69_636f_6e73_0000_0000;

pub fn plugin(app: &mut App) {
    app.insert_resource(dock()).add_systems(Startup, draw);
}

/// An icon of the panels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Plus,
    Cross,
    Check,
    /// Two checks: allowed from now on.
    CheckAll,
    /// An arrow to the left: back to where this was reached from.
    Back,
    /// A crown.
    Mayor,
    /// A house.
    Hamlet,
    /// A flame.
    Hearth,
    /// A speech balloon.
    Talk,
    /// A speech balloon with a plus in it.
    NewSession,
    /// A written page.
    Notes,
    /// An hourglass.
    History,
    /// A pencil.
    Rename,
    /// Arrows to the four sides.
    Move,
    /// A struck ring: gone, or to be closed down.
    Gone,
    /// A bin.
    Delete,
    /// A square in a ring.
    Stop,
    /// At rest, and the night.
    Moon,
    /// A bolt: at work.
    Bolt,
    /// A bell: something waits for the chief.
    Bell,
    /// A shield: the sandbox.
    Shield,
    /// A chip: the model.
    Chip,
    /// Waves from a point: the connection.
    Signal,
    /// Two coins: what was spent.
    Coins,
    /// A dial with a needle: the budget.
    Meter,
    /// A ticked ballot.
    Ballot,
    Signpost,
    /// A robot's head: an agent.
    Robot,
    /// Two sliders: the settings.
    Sliders,
    /// An arrow down into a tray: out of sight, still running.
    Lower,
    /// An arrow down to a line: the end of the chat.
    ToEnd,
    Power,
    /// An arrow turning back.
    Restore,
    Book,
    /// A sight: the camera put back.
    Aim,
    Clock,
    Sun,
    /// A magnifying glass: the Inspector.
    Lens,
    /// A triangle: go on.
    Play,
    Globe,
    /// A chevron that points up, for a dropdown to turn.
    Chevron,
    /// A chevron to the right: this opens a page of its own.
    Into,
    /// Three dots: the rest of what can be done.
    More,
    /// Three lines: a list folded away.
    Menu,
    Folder,
}

/// A part of an icon, in the units of its square, y downward. Angles
/// are in degrees, clockwise from the right.
enum Shape {
    /// A stroke from `(x, y)` to `(x, y)`.
    Line(f32, f32, f32, f32),
    /// A stroke through points.
    Path(&'static [[f32; 2]]),
    /// A stroke through points and back to the first.
    Loop(&'static [[f32; 2]]),
    /// A stroke from a point, bent toward a second, to a third.
    Curve(f32, f32, f32, f32, f32, f32),
    /// A stroke along a circle of a centre and a radius, between two
    /// angles.
    Arc(f32, f32, f32, f32, f32),
    /// A stroked circle of a centre and a radius.
    Ring(f32, f32, f32),
    /// A stroked rectangle of two corners, rounded by a radius.
    Round(f32, f32, f32, f32, f32),
    /// A filled circle of a centre and a radius.
    Dot(f32, f32, f32),
    /// A filled polygon.
    Fill(&'static [[f32; 2]]),
}

impl Glyph {
    /// Every icon.
    pub const ALL: [Glyph; 45] = [
        Glyph::Plus,
        Glyph::Cross,
        Glyph::Check,
        Glyph::CheckAll,
        Glyph::Back,
        Glyph::Mayor,
        Glyph::Hamlet,
        Glyph::Hearth,
        Glyph::Talk,
        Glyph::NewSession,
        Glyph::Notes,
        Glyph::History,
        Glyph::Rename,
        Glyph::Move,
        Glyph::Gone,
        Glyph::Delete,
        Glyph::Stop,
        Glyph::Moon,
        Glyph::Bolt,
        Glyph::Bell,
        Glyph::Shield,
        Glyph::Chip,
        Glyph::Signal,
        Glyph::Coins,
        Glyph::Meter,
        Glyph::Ballot,
        Glyph::Signpost,
        Glyph::Robot,
        Glyph::Sliders,
        Glyph::Lower,
        Glyph::ToEnd,
        Glyph::Power,
        Glyph::Restore,
        Glyph::Book,
        Glyph::Aim,
        Glyph::Clock,
        Glyph::Sun,
        Glyph::Lens,
        Glyph::Play,
        Glyph::Globe,
        Glyph::Chevron,
        Glyph::Into,
        Glyph::More,
        Glyph::Menu,
        Glyph::Folder,
    ];

    /// Its image for a side of `size`, to be tinted.
    pub fn image(self, size: f32) -> Handle<Image> {
        // The smallest drawing that is not stretched, or the largest.
        let drawn = SIZES
            .iter()
            .position(|drawn| *drawn >= size)
            .unwrap_or(SIZES.len() - 1);
        self.drawing(drawn)
    }

    fn drawing(self, drawn: usize) -> Handle<Image> {
        let at = (self as usize * SIZES.len() + drawn) as u128;
        Handle::from(Uuid::from_u128(FIRST + at))
    }

    fn shapes(self) -> &'static [Shape] {
        match self {
            Glyph::Plus => &[
                Line(8.0, 3.5, 8.0, 12.5),
                Line(3.5, 8.0, 12.5, 8.0),
            ],
            Glyph::Cross => &[
                Line(4.25, 4.25, 11.75, 11.75),
                Line(11.75, 4.25, 4.25, 11.75),
            ],
            Glyph::Check => {
                &[Path(&[[3.0, 8.5], [6.5, 12.0], [13.0, 4.5]])]
            }
            Glyph::CheckAll => &[
                Path(&[[1.5, 8.5], [4.75, 11.75], [10.25, 5.0]]),
                Path(&[[8.0, 10.75], [9.0, 11.75], [14.5, 5.0]]),
            ],
            Glyph::Back => &[
                Line(3.0, 8.0, 13.0, 8.0),
                Path(&[[7.0, 4.0], [3.0, 8.0], [7.0, 12.0]]),
            ],
            Glyph::Mayor => &[Loop(&[
                [3.5, 12.5],
                [2.5, 5.5],
                [5.75, 8.5],
                [8.0, 3.5],
                [10.25, 8.5],
                [13.5, 5.5],
                [12.5, 12.5],
            ])],
            Glyph::Hamlet => &[
                Path(&[[1.75, 8.0], [8.0, 2.25], [14.25, 8.0]]),
                Path(&[
                    [3.75, 7.5],
                    [3.75, 13.5],
                    [12.25, 13.5],
                    [12.25, 7.5],
                ]),
                Line(8.0, 10.25, 8.0, 13.5),
            ],
            Glyph::Hearth => &[
                Curve(9.25, 1.5, 9.75, 5.0, 11.9, 7.75),
                Arc(8.0, 10.0, 4.5, -30.0, 190.0),
                Curve(3.57, 9.22, 3.6, 7.0, 5.25, 5.75),
                Curve(5.25, 5.75, 5.75, 7.5, 6.75, 7.25),
                Curve(6.75, 7.25, 8.25, 5.0, 9.25, 1.5),
            ],
            Glyph::Talk => &[
                Round(2.0, 2.5, 14.0, 11.5, 2.5),
                Path(&[[5.0, 11.5], [5.0, 14.25], [8.0, 11.5]]),
            ],
            Glyph::NewSession => &[
                Round(2.0, 2.5, 14.0, 11.5, 2.5),
                Path(&[[5.0, 11.5], [5.0, 14.25], [8.0, 11.5]]),
                Line(8.0, 4.75, 8.0, 9.25),
                Line(5.75, 7.0, 10.25, 7.0),
            ],
            Glyph::Notes => &[
                Round(3.5, 2.0, 12.5, 14.0, 1.5),
                Line(6.25, 6.0, 9.75, 6.0),
                Line(6.25, 10.0, 9.75, 10.0),
            ],
            Glyph::History => &[
                Line(4.0, 2.5, 12.0, 2.5),
                Line(4.0, 13.5, 12.0, 13.5),
                Path(&[
                    [5.0, 2.5],
                    [5.0, 5.0],
                    [8.0, 8.0],
                    [11.0, 5.0],
                    [11.0, 2.5],
                ]),
                Path(&[
                    [5.0, 13.5],
                    [5.0, 11.0],
                    [8.0, 8.0],
                    [11.0, 11.0],
                    [11.0, 13.5],
                ]),
            ],
            Glyph::Rename => &[
                Loop(&[
                    [10.5, 2.5],
                    [13.5, 5.5],
                    [6.0, 13.0],
                    [2.5, 13.5],
                    [3.0, 10.0],
                ]),
                Line(8.75, 4.25, 11.75, 7.25),
            ],
            Glyph::Move => &[
                Line(8.0, 2.0, 8.0, 14.0),
                Line(2.0, 8.0, 14.0, 8.0),
                Path(&[[6.0, 4.0], [8.0, 2.0], [10.0, 4.0]]),
                Path(&[[6.0, 12.0], [8.0, 14.0], [10.0, 12.0]]),
                Path(&[[4.0, 6.0], [2.0, 8.0], [4.0, 10.0]]),
                Path(&[[12.0, 6.0], [14.0, 8.0], [12.0, 10.0]]),
            ],
            Glyph::Gone => &[
                Ring(8.0, 8.0, 5.75),
                Line(3.95, 12.05, 12.05, 3.95),
            ],
            Glyph::Delete => &[
                Line(3.0, 4.5, 13.0, 4.5),
                Path(&[
                    [6.5, 4.5],
                    [6.5, 2.5],
                    [9.5, 2.5],
                    [9.5, 4.5],
                ]),
                Path(&[
                    [4.25, 4.5],
                    [5.0, 13.5],
                    [11.0, 13.5],
                    [11.75, 4.5],
                ]),
            ],
            Glyph::Stop => &[
                Ring(8.0, 8.0, 6.0),
                Fill(&[
                    [5.75, 5.75],
                    [10.25, 5.75],
                    [10.25, 10.25],
                    [5.75, 10.25],
                ]),
            ],
            Glyph::Moon => &[
                Arc(7.5, 8.5, 5.5, 10.24, 259.76),
                Arc(10.75, 5.25, 4.75, 62.9, 207.1),
            ],
            Glyph::Bolt => &[Loop(&[
                [9.5, 1.75],
                [3.75, 9.0],
                [7.75, 9.0],
                [6.5, 14.25],
                [12.25, 7.0],
                [8.25, 7.0],
            ])],
            Glyph::Bell => &[
                Arc(8.0, 7.0, 3.5, 180.0, 360.0),
                Path(&[
                    [4.5, 7.0],
                    [4.5, 9.5],
                    [3.0, 12.0],
                    [13.0, 12.0],
                    [11.5, 9.5],
                    [11.5, 7.0],
                ]),
                Dot(8.0, 13.75, 1.1),
                Line(8.0, 2.25, 8.0, 3.5),
            ],
            Glyph::Shield => &[
                Path(&[[3.0, 4.0], [8.0, 2.0], [13.0, 4.0]]),
                Curve(3.0, 4.0, 2.5, 11.0, 8.0, 14.25),
                Curve(13.0, 4.0, 13.5, 11.0, 8.0, 14.25),
            ],
            Glyph::Chip => &[
                Round(4.5, 4.5, 11.5, 11.5, 1.0),
                Line(6.5, 2.0, 6.5, 4.5),
                Line(9.5, 2.0, 9.5, 4.5),
                Line(6.5, 11.5, 6.5, 14.0),
                Line(9.5, 11.5, 9.5, 14.0),
                Line(2.0, 6.5, 4.5, 6.5),
                Line(2.0, 9.5, 4.5, 9.5),
                Line(11.5, 6.5, 14.0, 6.5),
                Line(11.5, 9.5, 14.0, 9.5),
            ],
            Glyph::Signal => &[
                Dot(8.0, 11.75, 1.25),
                Arc(8.0, 11.75, 4.25, 225.0, 315.0),
                Arc(8.0, 11.75, 8.0, 225.0, 315.0),
            ],
            Glyph::Coins => &[
                Ring(6.25, 9.5, 4.25),
                Arc(9.75, 6.5, 4.25, 205.0, 434.0),
            ],
            Glyph::Meter => &[
                Arc(8.0, 10.5, 6.0, 180.0, 360.0),
                Line(8.0, 10.5, 11.25, 6.25),
                Dot(8.0, 10.5, 1.2),
            ],
            Glyph::Ballot => &[
                Round(2.5, 2.5, 13.5, 13.5, 2.0),
                Path(&[[5.25, 8.25], [7.25, 10.25], [10.75, 5.75]]),
            ],
            Glyph::Signpost => &[
                Line(8.0, 7.5, 8.0, 14.25),
                Loop(&[
                    [3.0, 2.5],
                    [10.75, 2.5],
                    [13.5, 5.0],
                    [10.75, 7.5],
                    [3.0, 7.5],
                ]),
            ],
            Glyph::Robot => &[
                Round(3.0, 5.5, 13.0, 13.5, 2.5),
                Line(8.0, 3.0, 8.0, 5.5),
                Dot(8.0, 2.5, 1.1),
                Dot(6.0, 9.5, 1.1),
                Dot(10.0, 9.5, 1.1),
            ],
            Glyph::Sliders => &[
                Line(2.5, 5.0, 13.5, 5.0),
                Line(2.5, 11.0, 13.5, 11.0),
                Dot(5.5, 5.0, 2.0),
                Dot(10.5, 11.0, 2.0),
            ],
            Glyph::ToEnd => &[
                Line(8.0, 2.5, 8.0, 10.5),
                Path(&[[4.5, 7.5], [8.0, 11.0], [11.5, 7.5]]),
                Line(3.0, 13.5, 13.0, 13.5),
            ],
            Glyph::Lower => &[
                Line(8.0, 2.5, 8.0, 9.5),
                Path(&[[5.0, 6.5], [8.0, 9.5], [11.0, 6.5]]),
                Path(&[
                    [2.5, 10.5],
                    [2.5, 13.5],
                    [13.5, 13.5],
                    [13.5, 10.5],
                ]),
            ],
            Glyph::Power => &[
                Arc(8.0, 8.5, 5.5, 305.0, 595.0),
                Line(8.0, 2.0, 8.0, 8.0),
            ],
            Glyph::Restore => &[
                Arc(8.0, 9.0, 5.0, 255.0, 535.0),
                Path(&[[9.13, 5.57], [6.71, 4.17], [8.11, 1.75]]),
            ],
            Glyph::Book => &[
                Line(8.0, 4.5, 8.0, 13.5),
                Path(&[
                    [8.0, 4.5],
                    [5.5, 3.0],
                    [2.0, 3.0],
                    [2.0, 12.0],
                    [5.5, 12.0],
                    [8.0, 13.5],
                ]),
                Path(&[
                    [8.0, 4.5],
                    [10.5, 3.0],
                    [14.0, 3.0],
                    [14.0, 12.0],
                    [10.5, 12.0],
                    [8.0, 13.5],
                ]),
            ],
            Glyph::Aim => &[
                Ring(8.0, 8.0, 4.25),
                Dot(8.0, 8.0, 1.1),
                Line(8.0, 1.5, 8.0, 3.75),
                Line(8.0, 12.25, 8.0, 14.5),
                Line(1.5, 8.0, 3.75, 8.0),
                Line(12.25, 8.0, 14.5, 8.0),
            ],
            Glyph::Clock => &[
                Ring(8.0, 8.0, 6.0),
                Path(&[[8.0, 4.75], [8.0, 8.0], [10.5, 9.5]]),
            ],
            Glyph::Sun => &[
                Ring(8.0, 8.0, 2.75),
                Line(13.0, 8.0, 14.75, 8.0),
                Line(11.54, 11.54, 12.77, 12.77),
                Line(8.0, 13.0, 8.0, 14.75),
                Line(4.46, 11.54, 3.23, 12.77),
                Line(3.0, 8.0, 1.25, 8.0),
                Line(4.46, 4.46, 3.23, 3.23),
                Line(8.0, 3.0, 8.0, 1.25),
                Line(11.54, 4.46, 12.77, 3.23),
            ],
            Glyph::Lens => {
                &[Ring(7.0, 7.0, 4.5), Line(10.4, 10.4, 13.75, 13.75)]
            }
            Glyph::Play => {
                &[Loop(&[[5.0, 3.0], [13.0, 8.0], [5.0, 13.0]])]
            }
            Glyph::Globe => &[
                Ring(8.0, 8.0, 6.0),
                Line(2.0, 8.0, 14.0, 8.0),
                Curve(8.0, 2.0, 2.5, 8.0, 8.0, 14.0),
                Curve(8.0, 2.0, 13.5, 8.0, 8.0, 14.0),
            ],
            Glyph::Chevron => {
                &[Path(&[[4.0, 10.0], [8.0, 6.0], [12.0, 10.0]])]
            }
            Glyph::Into => {
                &[Path(&[[6.0, 4.0], [10.0, 8.0], [6.0, 12.0]])]
            }
            Glyph::More => &[
                Dot(3.5, 8.0, 1.25),
                Dot(8.0, 8.0, 1.25),
                Dot(12.5, 8.0, 1.25),
            ],
            Glyph::Menu => &[
                Line(3.0, 4.5, 13.0, 4.5),
                Line(3.0, 8.0, 13.0, 8.0),
                Line(3.0, 11.5, 13.0, 11.5),
            ],
            Glyph::Folder => &[Loop(&[
                [2.0, 3.5],
                [6.25, 3.5],
                [7.75, 5.25],
                [14.0, 5.25],
                [14.0, 12.75],
                [2.0, 12.75],
            ])],
        }
    }
}

/// `which` as an icon with a side of `size`.
pub fn glyph(which: Glyph, size: f32) -> Icon {
    icon(which.image(size)).size(size)
}

/// The cross that closes a tab of the dock and the plus that adds
/// one.
pub fn dock() -> DockIcons {
    DockIcons {
        close: Some(Glyph::Cross.image(12.0)),
        add: Some(Glyph::Plus.image(14.0)),
    }
}

/// The width of a stroke, in the interface's pixels, of an icon with
/// a side of `size`. It grows more slowly than the icon, as the
/// stems of letters do.
fn weight(size: f32) -> f32 {
    (size * 0.1).clamp(1.25, 1.8)
}

fn to_line(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let along = b - a;
    let t = ((p - a).dot(along) / along.length_squared().max(1e-6))
        .clamp(0.0, 1.0);
    p.distance(a + along * t)
}

fn to_path(p: Vec2, points: &[[f32; 2]], closed: bool) -> f32 {
    let ends = points.iter().skip(1).chain(
        closed.then_some(points.first()).into_iter().flatten(),
    );
    points
        .iter()
        .zip(ends)
        .map(|(a, b)| to_line(p, Vec2::from(*a), Vec2::from(*b)))
        .fold(f32::MAX, f32::min)
}

fn to_curve(p: Vec2, a: Vec2, bend: Vec2, b: Vec2) -> f32 {
    const STEPS: usize = 12;
    let at = |step: usize| {
        let t = step as f32 / STEPS as f32;
        a.lerp(bend, t).lerp(bend.lerp(b, t), t)
    };
    (0..STEPS)
        .map(|step| to_line(p, at(step), at(step + 1)))
        .fold(f32::MAX, f32::min)
}

fn to_arc(
    p: Vec2,
    centre: Vec2,
    radius: f32,
    from: f32,
    to: f32,
) -> f32 {
    let out = p - centre;
    let angle = out.y.atan2(out.x).to_degrees();
    if (angle - from).rem_euclid(360.0) <= to - from {
        return (out.length() - radius).abs();
    }
    let end = |angle: f32| {
        centre + Vec2::from_angle(angle.to_radians()) * radius
    };
    p.distance(end(from)).min(p.distance(end(to)))
}

fn to_round(p: Vec2, min: Vec2, max: Vec2, radius: f32) -> f32 {
    let half = (max - min) / 2.0 - Vec2::splat(radius);
    let out = (p - (min + max) / 2.0).abs() - half;
    let signed =
        out.max(Vec2::ZERO).length() + out.x.max(out.y).min(0.0);
    (signed - radius).abs()
}

/// The distance from `p` to the edge of a polygon, negative inside.
fn in_polygon(p: Vec2, points: &[[f32; 2]]) -> f32 {
    let mut inside = false;
    let mut last = Vec2::from(points[points.len() - 1]);
    for point in points {
        let point = Vec2::from(*point);
        if (point.y > p.y) != (last.y > p.y)
            && p.x
                < (last.x - point.x) * (p.y - point.y)
                    / (last.y - point.y)
                    + point.x
        {
            inside = !inside;
        }
        last = point;
    }
    let edge = to_path(p, points, true);
    if inside { -edge } else { edge }
}

/// The share of the pixel at `p` an icon's ink covers, where `stroke`
/// is the width of a stroke and `pixel` the side of a pixel.
fn ink(shapes: &[Shape], p: Vec2, stroke: f32, pixel: f32) -> f32 {
    let mut away = f32::MAX;
    for shape in shapes {
        let half = stroke / 2.0;
        let d = match *shape {
            Line(ax, ay, bx, by) => {
                to_line(p, Vec2::new(ax, ay), Vec2::new(bx, by))
                    - half
            }
            Path(points) => to_path(p, points, false) - half,
            Loop(points) => to_path(p, points, true) - half,
            Curve(ax, ay, cx, cy, bx, by) => {
                to_curve(
                    p,
                    Vec2::new(ax, ay),
                    Vec2::new(cx, cy),
                    Vec2::new(bx, by),
                ) - half
            }
            Arc(x, y, radius, from, to) => {
                to_arc(p, Vec2::new(x, y), radius, from, to) - half
            }
            Ring(x, y, radius) => {
                (p.distance(Vec2::new(x, y)) - radius).abs() - half
            }
            Round(ax, ay, bx, by, radius) => {
                to_round(
                    p,
                    Vec2::new(ax, ay),
                    Vec2::new(bx, by),
                    radius,
                ) - half
            }
            Dot(x, y, radius) => p.distance(Vec2::new(x, y)) - radius,
            Fill(points) => in_polygon(p, points),
        };
        away = away.min(d);
    }
    (0.5 - away / pixel).clamp(0.0, 1.0)
}

/// `which` in white, for a side of `size`.
fn drawing(which: Glyph, size: f32) -> Image {
    let side = (size * DENSITY).round() as usize;
    let pixel = GRID / side as f32;
    let stroke = weight(size) * GRID / size;
    let shapes = which.shapes();
    let mut data = vec![255u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * pixel;
            data[(y * side + x) * 4 + 3] =
                (ink(shapes, p, stroke, pixel) * 255.0).round() as u8;
        }
    }
    Image::new(
        Extent3d {
            width: side as u32,
            height: side as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Draws every icon at every size.
pub fn draw(mut images: ResMut<Assets<Image>>) {
    for which in Glyph::ALL {
        for (drawn, size) in SIZES.into_iter().enumerate() {
            let _ = images
                .insert(&which.drawing(drawn), drawing(which, size));
        }
    }
}
