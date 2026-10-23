//! The plinth a town stands on: the land's own ground about the town
//! cut round, coloured as the realm colours its ground, the soil
//! under it, and a wooden base with a ring of brass.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3, Vec3Swizzles};

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::diorama::DioramaSettings;
use crate::world::land::Land;

/// The depth of the grass's edge down the plinth's side.
const GRASS_EDGE: f32 = 0.08;

/// The plinth's meshes, in the frame of the town's common.
pub struct Plinth {
    pub top: Mb,
    pub sides: Mb,
    pub brass: Mb,
}

impl Plinth {
    /// The plinth of the town whose common stands at `middle`, its
    /// top `radius` wide.
    pub fn of(
        land: &Land,
        middle: Frame,
        radius: f32,
        set: &DioramaSettings,
    ) -> Self {
        let pal = &land.pal;
        let (rings, segments) =
            (set.rings.max(1), set.segments.max(8));
        let unturn = middle.turn.inverse();
        // The ground at a point of the common's frame: where it is,
        // its normal and its colour.
        let ground = |p: Vec2| {
            let world = middle.point(Vec3::new(p.x, 0.0, p.y)).xz();
            let height = |d: Vec2| land.surface(world + d);
            let step = 0.3;
            let normal = Vec3::new(
                height(-Vec2::X * step) - height(Vec2::X * step),
                2.0 * step,
                height(-Vec2::Y * step) - height(Vec2::Y * step),
            )
            .normalize();
            let y = height(Vec2::ZERO);
            (
                Vec3::new(p.x, y - middle.at.y, p.y),
                unturn * normal,
                land.ground_colour(world, y, normal),
            )
        };
        let round = |s: usize| {
            Vec2::from_angle(s as f32 / segments as f32 * TAU)
        };

        let mut top = Mb::new();
        let (at, normal, colour) = ground(Vec2::ZERO);
        let centre = top.v(at, normal, colour);
        let mut last = vec![centre; segments];
        let mut rim = Vec::with_capacity(segments);
        for ring in 1..=rings {
            let r = radius * ring as f32 / rings as f32;
            let now = (0..segments)
                .map(|s| {
                    let (at, normal, colour) = ground(round(s) * r);
                    if ring == rings {
                        rim.push(at.y);
                    }
                    top.v(at, normal, colour)
                })
                .collect::<Vec<_>>();
            for s in 0..segments {
                let next = (s + 1) % segments;
                if ring == 1 {
                    top.tri(centre, now[s], now[next]);
                } else {
                    top.quad(last[s], last[next], now[next], now[s]);
                }
            }
            last = now;
        }

        let soil = set.soil;
        let earth =
            [pal.dirt_shade, pal.mud, pal.dirt, pal.grass_shade];
        let mut sides = Mb::new();
        for (band, colour) in earth.into_iter().enumerate() {
            let height = |s: usize, k: usize| match band + k {
                4 => rim[s],
                3 => rim[s] - GRASS_EDGE,
                at => -soil * (1.0 - at as f32 / 3.0),
            };
            let column = (0..segments)
                .map(|s| {
                    let out = round(s);
                    let normal = Vec3::new(out.x, 0.0, out.y);
                    [0, 1].map(|k| {
                        let at = out * radius;
                        sides.v(
                            Vec3::new(at.x, height(s, k), at.y),
                            normal,
                            colour,
                        )
                    })
                })
                .collect::<Vec<_>>();
            for s in 0..segments {
                let next = (s + 1) % segments;
                sides.quad(
                    column[s][0],
                    column[next][0],
                    column[next][1],
                    column[s][1],
                );
            }
        }
        let (base, out) = (set.base, radius + set.lip);
        for profile in [
            [(out, -soil), (radius, -soil)],
            [(out, -soil - base), (out, -soil)],
            [(0.0, -soil - base), (out, -soil - base)],
        ] {
            sides.lathe(Vec3::ZERO, &profile, segments, |_, _| {
                pal.timber_dark
            });
        }

        let mut brass = Mb::new();
        brass.torus(
            Vec3::Y * -(soil + base * 0.3),
            Vec3::Y,
            out,
            set.brass,
            segments,
            pal.brass,
        );
        Self { top, sides, brass }
    }
}
