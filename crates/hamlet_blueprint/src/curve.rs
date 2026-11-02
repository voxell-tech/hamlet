//! Exact curves: chains of lines, circular arcs and cubic Beziers,
//! measured and walked by arc length.

use core::f32::consts::{PI, TAU};

use bevy_math::Vec3;

/// The equal steps of a Bezier's parameter its arc length is tabled
/// at. The chords of that many steps are within a micrometre of a
/// metre-sized curve's length.
const TABLE: usize = 256;

/// The most points one segment is drawn with.
const MOST_PIECES: usize = 1024;

/// A straight run, a stretch of a circle or a cubic Bezier.
#[derive(Clone, PartialEq, Debug)]
pub enum Segment {
    Line { from: Vec3, to: Vec3 },
    Arc(Arc),
    Bezier(Bezier),
}

impl Segment {
    pub fn length(&self) -> f32 {
        match self {
            Segment::Line { from, to } => from.distance(*to),
            Segment::Arc(arc) => arc.length(),
            Segment::Bezier(bezier) => bezier.length(),
        }
    }

    /// The point `distance` along it, from 0 to its length, and the
    /// unit direction it goes in there.
    pub fn at(&self, distance: f32) -> (Vec3, Vec3) {
        match self {
            Segment::Line { from, to } => {
                let direction = (*to - *from).normalize_or_zero();
                (*from + direction * distance, direction)
            }
            Segment::Arc(arc) => {
                let angle = if arc.radius > 0.0 {
                    arc.start
                        + arc.sweep.signum() * distance / arc.radius
                } else {
                    arc.start
                };
                (arc.point(angle), arc.direction(angle))
            }
            Segment::Bezier(bezier) => {
                let t = bezier.parameter(distance);
                (bezier.point(t), bezier.direction(t))
            }
        }
    }

    pub fn start(&self) -> Vec3 {
        self.at(0.0).0
    }

    pub fn end(&self) -> Vec3 {
        match self {
            Segment::Line { to, .. } => *to,
            Segment::Arc(arc) => arc.point(arc.start + arc.sweep),
            Segment::Bezier(bezier) => bezier.points[3],
        }
    }

    /// Pushes the points of a polyline within `tolerance` of it, from
    /// its start up to but not including its end.
    fn sample(&self, tolerance: f32, points: &mut Vec<Vec3>) {
        let (pieces, point): (usize, Box<dyn Fn(f32) -> Vec3>) =
            match self {
                Segment::Line { from, .. } => {
                    points.push(*from);
                    return;
                }
                Segment::Arc(arc) => {
                    let arc = *arc;
                    (
                        arc.pieces(tolerance),
                        Box::new(move |along| {
                            arc.point(arc.start + arc.sweep * along)
                        }),
                    )
                }
                Segment::Bezier(bezier) => {
                    let bezier = bezier.clone();
                    (
                        bezier.pieces(tolerance),
                        Box::new(move |along| bezier.point(along)),
                    )
                }
            };
        points.extend(
            (0..pieces)
                .map(|piece| point(piece as f32 / pieces as f32)),
        );
    }
}

/// A stretch of a circle: the points `centre + radius * (cos a * zero
/// + sin a * (normal x zero))` for the angle `a` from `start` to
/// `start + sweep`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Arc {
    pub centre: Vec3,
    pub radius: f32,
    /// The unit vector the plane of the circle faces along.
    pub normal: Vec3,
    /// The unit vector from the centre to the point at angle zero.
    pub zero: Vec3,
    /// Radians.
    pub start: f32,
    /// Radians, negative to go the other way round.
    pub sweep: f32,
}

impl Arc {
    pub fn length(&self) -> f32 {
        self.radius * self.sweep.abs()
    }

    /// Whether it goes once round.
    pub fn is_full(&self) -> bool {
        self.sweep.abs() >= TAU
    }

    pub fn point(&self, angle: f32) -> Vec3 {
        let quarter = self.normal.cross(self.zero);
        self.centre
            + self.radius
                * (self.zero * libm::cosf(angle)
                    + quarter * libm::sinf(angle))
    }

    fn direction(&self, angle: f32) -> Vec3 {
        let quarter = self.normal.cross(self.zero);
        (quarter * libm::cosf(angle) - self.zero * libm::sinf(angle))
            * self.sweep.signum()
    }

    fn pieces(&self, tolerance: f32) -> usize {
        let step = if tolerance < self.radius {
            2.0 * libm::acosf(1.0 - tolerance / self.radius)
        } else {
            PI
        };
        ((self.sweep.abs() / step.max(1e-3)).ceil() as usize)
            .clamp(1, MOST_PIECES)
    }
}

/// A cubic Bezier from its first to its last control point, with the
/// length up to each of [`TABLE`] equal steps of its parameter.
#[derive(Clone, PartialEq, Debug)]
pub struct Bezier {
    points: [Vec3; 4],
    lengths: Vec<f32>,
}

impl Bezier {
    pub fn new(points: [Vec3; 4]) -> Self {
        let mut bezier = Self {
            points,
            lengths: Vec::new(),
        };
        let mut length = 0.0;
        let mut last = points[0];
        let mut lengths = vec![0.0];
        for step in 1..=TABLE {
            let now = bezier.point(step as f32 / TABLE as f32);
            length += last.distance(now);
            lengths.push(length);
            last = now;
        }
        bezier.lengths = lengths;
        bezier
    }

    pub fn points(&self) -> [Vec3; 4] {
        self.points
    }

    pub fn length(&self) -> f32 {
        self.lengths.last().copied().unwrap_or_default()
    }

    /// The point at the parameter `t`, from 0 to 1.
    pub fn point(&self, t: f32) -> Vec3 {
        let [a, b, c, d] = self.points;
        let s = 1.0 - t;
        a * (s * s * s)
            + b * (3.0 * s * s * t)
            + c * (3.0 * s * t * t)
            + d * (t * t * t)
    }

    /// The unit direction at the parameter `t`, along the chord when
    /// the curve stops there.
    fn direction(&self, t: f32) -> Vec3 {
        let [a, b, c, d] = self.points;
        let s = 1.0 - t;
        let slope = (b - a) * (s * s)
            + (c - b) * (2.0 * s * t)
            + (d - c) * (t * t);
        slope
            .try_normalize()
            .unwrap_or_else(|| (d - a).normalize_or_zero())
    }

    /// The parameter `distance` of arc length along it, found in the
    /// table and carried between its two nearest steps.
    fn parameter(&self, distance: f32) -> f32 {
        let distance = distance.clamp(0.0, self.length());
        let step = self
            .lengths
            .partition_point(|length| *length <= distance)
            .saturating_sub(1)
            .min(TABLE - 1);
        let (low, high) =
            (self.lengths[step], self.lengths[step + 1]);
        let along = if high > low {
            (distance - low) / (high - low)
        } else {
            0.0
        };
        (step as f32 + along) / TABLE as f32
    }

    /// The two Beziers it divides into at the parameter `t`.
    pub fn split(&self, t: f32) -> ([Vec3; 4], [Vec3; 4]) {
        let [a, b, c, d] = self.points;
        let (ab, bc, cd) = (a.lerp(b, t), b.lerp(c, t), c.lerp(d, t));
        let (left, right) = (ab.lerp(bc, t), bc.lerp(cd, t));
        let middle = left.lerp(right, t);
        ([a, ab, left, middle], [middle, right, cd, d])
    }

    /// How many straight pieces keep a polyline within `tolerance`
    /// of it.
    fn pieces(&self, tolerance: f32) -> usize {
        let [a, b, c, d] = self.points;
        let bend = (a - 2.0 * b + c)
            .length()
            .max((b - 2.0 * c + d).length());
        ((0.75 * bend / tolerance.max(1e-6)).sqrt().ceil() as usize)
            .clamp(1, MOST_PIECES)
    }
}

/// Where two segments of a curve meet, or where it starts or ends.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Corner {
    pub at: Vec3,
    /// The unit direction the next segment goes in, or the last one
    /// came in at the end of an open curve.
    pub direction: Vec3,
    /// The length of the curve before it.
    pub distance: f32,
}

/// Segments end to end, open, or closed when the last ends where the
/// first starts and the curve wraps there.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Curve {
    segments: Vec<Segment>,
    closed: bool,
}

impl Curve {
    pub fn new(segments: Vec<Segment>, closed: bool) -> Self {
        Self { segments, closed }
    }

    pub fn line(from: Vec3, to: Vec3) -> Self {
        Self::new(vec![Segment::Line { from, to }], false)
    }

    /// Lines through `points` in order, the last joined to the first
    /// when `closed` and there are three or more.
    pub fn polyline(points: &[Vec3], closed: bool) -> Self {
        let closed = closed && points.len() > 2;
        let edges = if closed {
            points.len()
        } else {
            points.len().saturating_sub(1)
        };
        let segments = (0..edges)
            .map(|at| Segment::Line {
                from: points[at],
                to: points[(at + 1) % points.len()],
            })
            .collect();
        Self::new(segments, closed)
    }

    /// The curve along `arc`, closed when it goes once round, which
    /// it then does exactly.
    pub fn from_arc(mut arc: Arc) -> Self {
        let closed = arc.is_full();
        if closed {
            arc.sweep = TAU * arc.sweep.signum();
        }
        Self::new(vec![Segment::Arc(arc)], closed)
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn length(&self) -> f32 {
        self.segments.iter().map(Segment::length).sum()
    }

    /// The point `distance` along the curve and the direction it goes
    /// in there. A point on a join is on the segment that starts
    /// there. A closed curve wraps; an open one has nothing past its
    /// end.
    pub fn at(&self, distance: f32) -> Option<(Vec3, Vec3)> {
        let length = self.length();
        if length <= 0.0 {
            return None;
        }
        let mut left = if self.closed {
            distance.rem_euclid(length)
        } else if (0.0..=length).contains(&distance) {
            distance
        } else {
            return None;
        };
        for segment in &self.segments {
            let own = segment.length();
            if left < own {
                return Some(segment.at(left));
            }
            left -= own;
        }
        self.segments.last().map(|last| last.at(last.length()))
    }

    /// The start of each segment, and the end of an open curve.
    pub fn corners(&self) -> Vec<Corner> {
        let mut distance = 0.0;
        let mut corners = Vec::with_capacity(self.segments.len() + 1);
        for segment in &self.segments {
            let (at, direction) = segment.at(0.0);
            corners.push(Corner {
                at,
                direction,
                distance,
            });
            distance += segment.length();
        }
        if !self.closed
            && let Some(last) = self.segments.last()
        {
            let (at, direction) = last.at(last.length());
            corners.push(Corner {
                at,
                direction,
                distance,
            });
        }
        corners
    }

    /// The points of [`Self::corners`].
    pub fn points(&self) -> Vec<Vec3> {
        self.corners().iter().map(|corner| corner.at).collect()
    }

    /// A polyline within `tolerance` of the curve, ending where it
    /// began when the curve is closed.
    pub fn sampled(&self, tolerance: f32) -> Vec<Vec3> {
        let mut points = Vec::new();
        for segment in &self.segments {
            segment.sample(tolerance, &mut points);
        }
        if let Some(last) = self.segments.last() {
            points.push(last.end());
        }
        points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_is_walked_by_arc_length() {
        // Control points bunched at one end: the parameter runs fast
        // there, the length does not.
        let bezier = Bezier::new([
            Vec3::ZERO,
            Vec3::new(0.1, 0.0, 0.0),
            Vec3::new(0.2, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
        ]);
        assert!((bezier.length() - 3.0).abs() < 1e-4);
        let curve = Curve::new(vec![Segment::Bezier(bezier)], false);
        let (at, _) = curve.at(1.5).unwrap();
        assert!((at.x - 1.5).abs() < 1e-3);
    }
}
