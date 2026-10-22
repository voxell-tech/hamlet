//! The pointers' pictures: each a few shapes with a dark edge around
//! them, in a square of [`SIDE`] pixels.

use std::f32::consts::{FRAC_1_SQRT_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::color::{Alpha, Color, ColorToPacked, Mix, Srgba};
use bevy::image::Image;
use bevy::math::Vec2;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};

use crate::pointer::{PointerMode, ResizeAxis};
use crate::theme::Colors;

/// The side of a pointer, in pixels. The system takes no scale, so a
/// larger picture would be a larger pointer, not a sharper one.
const SIDE: usize = 32;

/// The colours the pointers are painted in.
#[derive(Clone, Copy)]
pub(crate) struct Inks {
    /// The heart of the flame, its body and its rim.
    flame: Color,
    lamp: Color,
    ember: Color,
    /// The taper and the glove, lit and in shade.
    wax: Color,
    shade: Color,
    brass: Color,
    rune: Color,
    coal: Color,
    /// The dark line around every pointer.
    edge: Color,
}

impl From<&Colors> for Inks {
    fn from(colors: &Colors) -> Self {
        Self {
            flame: *colors.active_strong,
            lamp: *colors.active,
            ember: *colors.critical_strong,
            wax: *colors.text,
            shade: *colors.text_accent,
            brass: *colors.accent,
            rune: *colors.info,
            coal: *colors.critical_glow,
            edge: *colors.background,
        }
    }
}

enum Shape {
    /// A line from `a` to `b`, round at both ends, as thick as `ra`
    /// about `a` and `rb` about `b`.
    Cone {
        a: Vec2,
        b: Vec2,
        ra: f32,
        rb: f32,
    },
    /// The part of a circle that starts at the angle `from` and goes
    /// on for `sweep`, `half` thick to each side.
    Arc {
        at: Vec2,
        r: f32,
        from: f32,
        sweep: f32,
        half: f32,
    },
    Poly(Vec<Vec2>),
}

fn disc(at: Vec2, r: f32) -> Shape {
    Shape::Cone {
        a: at,
        b: at,
        ra: r,
        rb: r,
    }
}

fn line(a: Vec2, b: Vec2, half: f32) -> Shape {
    Shape::Cone {
        a,
        b,
        ra: half,
        rb: half,
    }
}

fn ring(at: Vec2, r: f32, half: f32) -> Shape {
    Shape::Arc {
        at,
        r,
        from: 0.0,
        sweep: TAU,
        half,
    }
}

/// An arrowhead with its point at `tip`, facing `toward`.
fn head(tip: Vec2, toward: Vec2, long: f32, wide: f32) -> Shape {
    let back = tip - toward * long;
    let across = toward.perp() * wide;
    Shape::Poly(vec![tip, back + across, back - across])
}

impl Shape {
    /// The distance of `p` outside the shape. Less than nothing
    /// inside it.
    fn away(&self, p: Vec2) -> f32 {
        match self {
            Shape::Cone { a, b, ra, rb } => {
                let (pa, ba) = (p - *a, *b - *a);
                let h = ba.length_squared();
                if h < 1e-6 {
                    return pa.length() - ra.max(*rb);
                }
                // The round cone's distance.
                let rr = ra - rb;
                let a2 = h - rr * rr;
                let y = pa.dot(ba);
                let z = y - h;
                let x2 = (pa * h - ba * y).length_squared();
                let (y2, z2) = (y * y * h, z * z * h);
                let k = rr.signum() * rr * rr * x2;
                if z.signum() * a2 * z2 > k {
                    (x2 + z2).sqrt() / h - rb
                } else if y.signum() * a2 * y2 < k {
                    (x2 + y2).sqrt() / h - ra
                } else {
                    ((x2 * a2 / h).sqrt() + y * rr) / h - ra
                }
            }
            Shape::Arc {
                at,
                r,
                from,
                sweep,
                half,
            } => {
                let to = p - *at;
                let turned = (to.to_angle() - from).rem_euclid(TAU);
                if turned <= *sweep {
                    return (to.length() - r).abs() - half;
                }
                let end =
                    |angle: f32| *at + Vec2::from_angle(angle) * *r;
                p.distance(end(*from))
                    .min(p.distance(end(from + sweep)))
                    - half
            }
            Shape::Poly(points) => {
                let mut near = f32::MAX;
                let mut inside = false;
                for (i, a) in points.iter().enumerate() {
                    let b = points[(i + 1) % points.len()];
                    let (edge, to) = (b - *a, p - *a);
                    let along = (to.dot(edge)
                        / edge.length_squared())
                    .clamp(0.0, 1.0);
                    near = near.min((to - edge * along).length());
                    if (a.y > p.y) != (b.y > p.y)
                        && p.x < a.x + (p.y - a.y) / edge.y * edge.x
                    {
                        inside = !inside;
                    }
                }
                if inside { -near } else { near }
            }
        }
    }
}

/// The shapes of one pointer, each over the ones before it, and the
/// light its flame sheds: its source and its reach.
#[derive(Default)]
struct Picture {
    shapes: Vec<(Shape, Color)>,
    light: Option<(Vec2, f32)>,
}

impl Picture {
    fn add(&mut self, shape: Shape, ink: Color) {
        self.shapes.push((shape, ink));
    }

    /// A flame that comes to a point at `tip`, swells to `swell` at
    /// `long` along `back`, and leans a little on its way there.
    fn flame(
        &mut self,
        tip: Vec2,
        back: Vec2,
        long: f32,
        swell: f32,
        inks: &Inks,
    ) {
        let belly = tip + back * long;
        let waist =
            tip + back * long * 0.45 - back.perp() * swell * 0.24;
        // Each tongue: its start between the tip and the waist, its
        // end past the belly, and its width.
        let tongues = [
            (0.0, 0.0, 1.0, inks.ember),
            (0.55, 0.3, 0.64, inks.lamp),
            (1.0, 0.7, 0.4, inks.flame),
            (1.0, 1.0, 0.17, inks.wax),
        ];
        for (start, past, wide, ink) in tongues {
            let (mid, end) = (swell * 0.52 * wide, swell * wide);
            let from = tip.lerp(waist, start);
            let to = belly + back * past;
            if start < 1.0 {
                self.add(
                    Shape::Cone {
                        a: from,
                        b: waist,
                        ra: 0.45,
                        rb: mid,
                    },
                    ink,
                );
            }
            self.add(
                Shape::Cone {
                    a: waist.lerp(to, (start - 0.6).max(0.0)),
                    b: to,
                    ra: if start < 1.0 { mid } else { 0.2 },
                    rb: end,
                },
                ink,
            );
        }
        self.light = Some((belly, swell * 2.1));
    }

    /// A lit taper that leans up and to the left, with the point of
    /// its flame at `tip` and its foot `foot` from there.
    fn taper(
        &mut self,
        tip: Vec2,
        long: f32,
        swell: f32,
        foot: f32,
        inks: &Inks,
    ) {
        let back = Vec2::splat(FRAC_1_SQRT_2);
        let at = |far: f32| tip + back * far;
        self.add(line(at(long), at(foot), 1.6), inks.wax);
        self.add(line(at(foot - 6.0), at(foot), 2.0), inks.brass);
        self.add(
            line(at(foot - 7.0), at(foot - 6.4), 2.7),
            inks.brass,
        );
        // The wick, dark between the wax and the flame.
        let wick = at(long + swell + 0.2);
        let across = back.perp() * 2.0;
        self.add(line(wick - across, wick + across, 0.6), inks.edge);
        self.flame(tip, back, long, swell, inks);
    }

    /// The cuff of a glove whose wrist is at `at`.
    fn cuff(&mut self, at: Vec2, inks: &Inks) {
        let half = Vec2::X * 4.2;
        self.add(line(at - half, at + half, 2.3), inks.brass);
        self.add(disc(at, 1.2), inks.lamp);
    }

    /// The picture as an image, with `edge` around all of it and
    /// the light of its flame in `glow` behind it.
    fn paint(&self, edge: Color, glow: Color) -> Image {
        let (edge, glow) = (Srgba::from(edge), Srgba::from(glow));
        let mut data = Vec::with_capacity(SIDE * SIDE * 4);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let mut near = f32::MAX;
                let mut colour = edge;
                for (shape, ink) in &self.shapes {
                    let away = shape.away(p);
                    near = near.min(away);
                    colour = colour.mix(
                        &Srgba::from(*ink),
                        (0.5 - away).clamp(0.0, 1.0),
                    );
                }
                let cover = (1.6 - near).clamp(0.0, 1.0);
                let lit = self.light.map_or(0.0, |(from, reach)| {
                    let dim =
                        (1.0 - p.distance(from) / reach).max(0.0);
                    0.65 * dim * dim * (1.0 - cover)
                });
                let alpha = cover + lit;
                let colour =
                    glow.mix(&colour, cover / alpha.max(1e-4));
                data.extend(colour.with_alpha(alpha).to_u8_array());
            }
        }
        Image::new(
            Extent3d {
                width: SIDE as u32,
                height: SIDE as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }
}

impl PointerMode {
    /// The pixel of its picture that is on the pointer.
    pub(crate) fn hotspot(self) -> (u16, u16) {
        match self {
            Self::Point => (3, 3),
            Self::Press => (7, 7),
            Self::Pick => (14, 14),
            Self::Grab => (16, 18),
            Self::Carry => (16, 17),
            _ => (16, 16),
        }
    }

    /// The number of pictures it goes through.
    pub(crate) fn frames(self) -> usize {
        match self {
            Self::Wait => 8,
            _ => 1,
        }
    }

    /// Its picture, the one numbered `frame` where it has several.
    pub(crate) fn face(self, frame: usize, inks: &Inks) -> Image {
        let mut art = Picture::default();
        let (hx, hy) = self.hotspot();
        let hot = Vec2::new(hx as f32 + 0.5, hy as f32 + 0.5);
        let mid = Vec2::splat(16.0);
        match self {
            Self::Point => art.taper(hot, 11.5, 4.6, 31.0, inks),
            Self::Press => {
                art.taper(hot, 11.5, 5.4, 27.5, inks);
                // The flame flares, and sparks fly off to both sides.
                let heart = hot + Vec2::splat(6.2);
                for turn in [-75.0_f32, -30.0, 120.0, 165.0] {
                    let out = Vec2::from_angle(turn.to_radians());
                    art.add(
                        line(
                            heart + out * 8.0,
                            heart + out * 10.6,
                            0.9,
                        ),
                        inks.flame,
                    );
                }
            }
            Self::Pick => {
                // A glass held up to the thing: a ring of the agents'
                // magic with the flame in its middle.
                let back = Vec2::splat(FRAC_1_SQRT_2);
                art.add(
                    line(hot + back * 10.0, hot + back * 17.6, 2.1),
                    inks.brass,
                );
                art.add(
                    line(hot + back * 9.8, hot + back * 10.6, 2.9),
                    inks.brass,
                );
                art.add(ring(hot, 9.0, 1.1), inks.rune);
                for out in [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y] {
                    art.add(
                        line(hot + out * 9.0, hot + out * 10.8, 1.1),
                        inks.rune,
                    );
                }
                art.flame(hot - back * 5.2, back, 6.0, 2.9, inks);
                art.light = Some((hot, 9.0));
            }
            Self::Grab => {
                let fingers = [
                    (9.6, 15.0, 7.2, 8.6, 2.2),
                    (13.6, 13.5, 12.8, 5.0, 2.3),
                    (18.4, 13.5, 19.2, 5.0, 2.3),
                    (22.4, 15.0, 24.6, 8.8, 2.1),
                    (10.5, 21.0, 4.8, 18.4, 2.3),
                ];
                for (ax, ay, bx, by, r) in fingers {
                    art.add(
                        line(Vec2::new(ax, ay), Vec2::new(bx, by), r),
                        inks.wax,
                    );
                }
                art.add(disc(Vec2::new(16.0, 19.0), 6.8), inks.wax);
                art.cuff(Vec2::new(16.0, 26.5), inks);
            }
            Self::Carry => {
                art.add(
                    line(
                        Vec2::new(11.5, 17.5),
                        Vec2::new(20.5, 17.5),
                        6.0,
                    ),
                    inks.wax,
                );
                for x in [9.8, 13.9, 18.1, 22.2] {
                    art.add(disc(Vec2::new(x, 12.4), 2.8), inks.wax);
                }
                for x in [11.85, 16.0, 20.15] {
                    art.add(
                        line(
                            Vec2::new(x, 11.6),
                            Vec2::new(x, 14.0),
                            0.45,
                        ),
                        inks.edge,
                    );
                }
                art.add(
                    line(
                        Vec2::new(7.6, 18.6),
                        Vec2::new(14.4, 20.4),
                        2.4,
                    ),
                    inks.shade,
                );
                art.cuff(Vec2::new(16.0, 26.3), inks);
            }
            Self::Slide => {
                for out in [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y] {
                    art.add(
                        head(mid + out * 13.5, out, 6.2, 5.0),
                        inks.wax,
                    );
                }
                art.add(disc(mid, 2.9), inks.ember);
                art.add(disc(mid, 1.7), inks.flame);
            }
            Self::Turn => {
                for from in [20.0_f32, 200.0] {
                    let (from, sweep) =
                        (from.to_radians(), 105.0_f32.to_radians());
                    art.add(
                        Shape::Arc {
                            at: mid,
                            r: 10.0,
                            from,
                            sweep,
                            half: 1.3,
                        },
                        inks.wax,
                    );
                    let end = Vec2::from_angle(from + sweep);
                    art.add(
                        head(
                            mid + end * 10.0 + end.perp() * 5.6,
                            end.perp(),
                            6.4,
                            4.0,
                        ),
                        inks.wax,
                    );
                }
                art.add(disc(mid, 2.9), inks.ember);
                art.add(disc(mid, 1.7), inks.flame);
            }
            Self::Write => {
                for (foot, lean) in [(6.0, -1.6), (26.0, 1.6)] {
                    for side in [-1.0, 1.0] {
                        art.add(
                            line(
                                Vec2::new(16.0 + side * 1.0, foot),
                                Vec2::new(
                                    16.0 + side * 3.6,
                                    foot + lean,
                                ),
                                0.9,
                            ),
                            inks.brass,
                        );
                    }
                }
                art.add(
                    line(
                        Vec2::new(16.0, 6.4),
                        Vec2::new(16.0, 25.6),
                        0.9,
                    ),
                    inks.wax,
                );
            }
            Self::Resize(axis) => {
                let along = match axis {
                    ResizeAxis::Across => Vec2::X,
                    ResizeAxis::Upright => Vec2::Y,
                    ResizeAxis::Falling => Vec2::splat(FRAC_1_SQRT_2),
                    ResizeAxis::Rising => {
                        Vec2::new(FRAC_1_SQRT_2, -FRAC_1_SQRT_2)
                    }
                };
                art.add(
                    line(mid - along * 7.0, mid + along * 7.0, 1.3),
                    inks.wax,
                );
                for out in [along, -along] {
                    art.add(
                        head(mid + out * 13.2, out, 6.4, 5.0),
                        inks.wax,
                    );
                }
                art.add(disc(mid, 2.6), inks.brass);
            }
            Self::Refuse => {
                let slash = Vec2::splat(FRAC_1_SQRT_2) * 9.6;
                art.add(ring(mid, 10.4, 1.9), inks.coal);
                art.add(
                    line(mid - slash, mid + slash, 1.8),
                    inks.coal,
                );
            }
            Self::Wait => {
                art.flame(
                    Vec2::new(16.0, 8.6),
                    Vec2::Y,
                    9.0,
                    3.6,
                    inks,
                );
                // Fireflies go round it, the first one brightest.
                let turn = frame as f32 / self.frames() as f32 * TAU;
                let flies = [
                    (2.3, inks.flame),
                    (1.9, inks.lamp),
                    (1.5, inks.lamp),
                    (1.1, inks.ember),
                    (0.7, inks.ember),
                ];
                for (i, (r, ink)) in flies.into_iter().enumerate() {
                    let out = Vec2::from_angle(turn - i as f32 * 0.5);
                    art.add(disc(mid + out * 11.2, r), ink);
                }
            }
        }
        art.paint(inks.edge, inks.lamp)
    }
}
