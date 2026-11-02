//! Splines: cubic Beziers made from a path's points.

use bevy_math::Vec3;
use hamlet_graph::PathPoint;

use crate::curve::{Bezier, Curve, Segment};

/// The least a knot of the centripetal parameter may be apart, so
/// points on top of each other do not divide by zero.
const LEAST_KNOT: f32 = 1e-4;

/// How a spline follows its points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplineKind {
    /// Through every point, as a centripetal Catmull-Rom spline.
    Through,
    /// By the tangent handles of each point.
    Handles,
}

impl SplineKind {
    /// The names of the kinds, as a choice input takes them.
    pub const CHOICES: &'static [&'static str] =
        &["through", "handles"];

    pub fn from_choice(choice: &str) -> Option<Self> {
        match choice {
            "through" => Some(Self::Through),
            "handles" => Some(Self::Handles),
            _ => None,
        }
    }
}

/// The control points of each Bezier of the spline through `points`,
/// one for each pair of points in order, and one from the last to
/// the first when `closed`.
pub fn beziers(
    points: &[PathPoint],
    kind: SplineKind,
    closed: bool,
) -> Vec<[Vec3; 4]> {
    let count = points.len();
    if count < 2 {
        return Vec::new();
    }
    let segments = if closed { count } else { count - 1 };
    (0..segments)
        .map(|at| match kind {
            SplineKind::Handles => {
                let (a, b) = (&points[at], &points[(at + 1) % count]);
                [a.at, a.at + a.after, b.at + b.before, b.at]
            }
            SplineKind::Through => through(points, at, closed),
        })
        .collect()
}

/// The Bezier from point `at` to the next, through them as a
/// centripetal Catmull-Rom spline goes. An open spline's ends use
/// a point mirrored across them.
fn through(
    points: &[PathPoint],
    at: usize,
    closed: bool,
) -> [Vec3; 4] {
    let count = points.len() as isize;
    let point = |index: isize| {
        let last =
            |back: isize| points[(count - 1 - back) as usize].at;
        if closed {
            points[index.rem_euclid(count) as usize].at
        } else if index < 0 {
            points[0].at * 2.0 - points[1].at
        } else if index >= count {
            last(0) * 2.0 - last(1)
        } else {
            points[index as usize].at
        }
    };
    let at = at as isize;
    let [p0, p1, p2, p3] = [-1, 0, 1, 2].map(|step| point(at + step));
    let knot = |from: Vec3, to: Vec3| {
        from.distance(to).sqrt().max(LEAST_KNOT)
    };
    let t1 = knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let tangent_in =
        (p1 - p0) / t1 - (p2 - p0) / t2 + (p2 - p1) / (t2 - t1);
    let tangent_out = (p2 - p1) / (t2 - t1) - (p3 - p1) / (t3 - t1)
        + (p3 - p2) / (t3 - t2);
    let third = (t2 - t1) / 3.0;
    [p1, p1 + tangent_in * third, p2 - tangent_out * third, p2]
}

/// The spline through `points` as a curve.
pub fn curve(
    points: &[PathPoint],
    kind: SplineKind,
    closed: bool,
) -> Curve {
    let segments = beziers(points, kind, closed)
        .into_iter()
        .map(|control| Segment::Bezier(Bezier::new(control)))
        .collect();
    Curve::new(segments, closed)
}

/// `points` with one more at the parameter `t` of the Bezier from
/// point `segment` to the next. By handles the spline keeps its
/// shape.
pub fn inserted(
    points: &[PathPoint],
    kind: SplineKind,
    closed: bool,
    segment: usize,
    t: f32,
) -> Vec<PathPoint> {
    let Some(&control) = beziers(points, kind, closed).get(segment)
    else {
        return points.to_vec();
    };
    let (first, second) = Bezier::new(control).split(t);
    let middle = first[3];
    let mut made = points.to_vec();
    if kind == SplineKind::Handles {
        let next = (segment + 1) % points.len();
        made[segment].after = first[1] - first[0];
        made[next].before = second[2] - second[3];
    }
    made.insert(
        segment + 1,
        PathPoint::new(middle, first[2] - middle, second[1] - middle),
    );
    made
}
