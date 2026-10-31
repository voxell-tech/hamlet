//! The shape of the land: its rivers, its heights and its colours.

use std::f32::consts::TAU;

use bevy::prelude::*;
use hamlet_vfx::ground::SoilUniform;

use super::{HOLD_EASE, LandData, Unfit};
use crate::generator::noise::{fbm, key, perlin, unit};
use crate::meshes::{lerp, smoothstep};

impl LandData {
    /// `p`, moved straight away from the nearest river if nearer
    /// than `min`.
    pub(super) fn clear_of_river(&self, p: Vec2, min: f32) -> Vec2 {
        let Some((k, u, off)) = self.river_by(p) else {
            return p;
        };
        let min = min * self.river_slant(k, u);
        if off.abs() >= min {
            return p;
        }
        let side = if off < 0.0 { -1.0 } else { 1.0 };
        p + self.river_dir.perp() * (side * min - off)
    }

    /// Factor by which the sideways offset is longer than the true
    /// distance where a river runs at a slant.
    fn river_slant(&self, k: i32, u: f32) -> f32 {
        let slope = self.river_centre(k, u + 0.5)
            - self.river_centre(k, u - 0.5);
        (1.0 + slope * slope).sqrt()
    }

    fn river_frame(&self, p: Vec2) -> (f32, f32) {
        (p.dot(self.river_dir), p.dot(self.river_dir.perp()))
    }

    /// The sideways offset of river `k` at distance `u` along the
    /// rivers' axis. Rivers lie a gap apart and each wanders on its
    /// own, less than half the gap, so no two meet.
    pub(super) fn river_centre(&self, k: i32, u: f32) -> f32 {
        let own = self.seed.wrapping_add(key(&[k as u32]));
        let seed = if k == 0 { self.seed } else { own };
        let set = &self.set.land;
        let drift = if k == 0 {
            0.0
        } else {
            set.river_drift
                * perlin(
                    Vec2::new(u * set.drift_scale, 5.7),
                    own.wrapping_add(1),
                )
        };
        24.0 + k as f32 * set.river_gap
            + drift
            + set.bends.1
                * perlin(Vec2::new(u * set.bends.0, 3.3), seed)
            + set.kinks.1
                * perlin(Vec2::new(u * set.kinks.0, 9.1), seed)
    }

    /// The river that runs near `p`, if one does: its number, the
    /// distance along it, and how far to its side `p` lies.
    fn river_by(&self, p: Vec2) -> Option<(i32, f32, f32)> {
        let (u, v) = self.river_frame(p);
        let set = &self.set.land;
        // Further than this from a river's own line, nothing feels
        // the river.
        let reach =
            set.bends.1 + set.kinks.1 + set.river_drift + 90.0;
        let k0 = ((v - 24.0) / set.river_gap).round() as i32;
        (k0 - 1..=k0 + 1)
            .filter(|&k| {
                (v - 24.0 - k as f32 * set.river_gap).abs() < reach
            })
            .map(|k| (k, u, v - self.river_centre(k, u)))
            .min_by(|a, b| a.2.abs().total_cmp(&b.2.abs()))
    }

    /// Distance from `p` to the middle line of the nearest river.
    /// Very large where none runs near.
    pub(crate) fn river_dist(&self, p: Vec2) -> f32 {
        match self.river_by(p) {
            Some((k, u, off)) => off.abs() / self.river_slant(k, u),
            None => f32::MAX,
        }
    }

    /// 0 on low ground, 1 where a hill stands as tall as hills go.
    pub(crate) fn mound(&self, p: Vec2) -> f32 {
        (self.hill(p) / self.set.land.mound_height.1.max(0.1))
            .min(1.0)
    }

    /// The hills at `p`: how high the tallest of them lifts the
    /// ground there. Each hill is a stroke: a short line with a level
    /// top about it, its sides falling away in a skirt as steep as
    /// the stroke's own angle. Where two meet, the taller wins. One
    /// may stand in each cell of a grid, beyond the plains about the
    /// middle and clear of the towns.
    pub(crate) fn hill(&self, p: Vec2) -> f32 {
        let set = &self.set.land;
        let (side, share) = set.mounds;
        let edge = p.length()
            + set.edge.1 * perlin(p * set.edge.0, self.seed + 50);
        if edge < set.plains {
            return 0.0;
        }
        let cell = (p / side).floor();
        let ragged = self.ragged(p);
        let mut top = 0f32;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let (x, z) = (cell.x as i32 + dx, cell.y as i32 + dz);
                let h = |k: u32| {
                    unit(key(&[
                        self.seed + 55,
                        x as u32,
                        z as u32,
                        k,
                    ]))
                };
                if h(0) > share {
                    continue;
                }
                let mid = (Vec2::new(x as f32, z as f32)
                    + 0.2
                    + Vec2::new(h(1), h(2)) * 0.6)
                    * side;
                let half = Vec2::from_angle(h(3) * TAU)
                    * set.mound_length
                    * h(4)
                    * 0.5;
                let lerp_by =
                    |(a, b): (f32, f32), t: f32| lerp(a, b, t);
                let tall = lerp_by(set.mound_height, h(5));
                let flat = lerp_by(set.mound_size, h(6));
                let skirt =
                    lerp_by(set.mound_skirt, h(7)).to_radians();
                let run = tall / skirt.tan().max(0.05);
                if self
                    .near_town(mid, flat + run + half.length() + 8.0)
                {
                    continue;
                }
                let along = segment_dist(p, mid - half, mid + half);
                let rise = stroke(along + ragged, flat, run);
                top = top.max(tall * rise);
            }
        }
        if top <= 0.0 {
            return 0.0;
        }
        // Lumps over the tops, so that no top is quite level.
        let lumps = 0.35 * perlin(p / 16.0, self.seed + 56)
            + 0.15 * perlin(p / 8.0, self.seed + 57);
        top + lumps * (top / set.mound_height.1.max(0.1)).min(1.0)
    }

    /// How far the edges of strokes are pushed in or out at `p`, so
    /// that a hill's foot and the coast wander.
    fn ragged(&self, p: Vec2) -> f32 {
        let amount = self.set.land.ragged;
        amount
            * (0.7 * perlin(p / 5.0, self.seed + 58)
                + 0.3 * perlin(p / 2.0, self.seed + 59))
    }

    /// True when a town stands within `reach` of `p`.
    fn near_town(&self, p: Vec2, reach: f32) -> bool {
        let clear = reach + self.set.town.radius;
        self.shores().any(|at| at.distance(p) < clear)
    }

    /// The ground as it lies before any site levels it or a river
    /// cuts it, `d` from the nearest river.
    fn wild(&self, p: Vec2, d: f32) -> f32 {
        let (seed, set) = (self.seed, &self.set.land);
        // Rolling plains, four octaves.
        let h = set.plain_y
            + set.hills.1 * fbm(p * set.hills.0, 4, seed + 20)
            + set.grain.1 * perlin(p * set.grain.0, seed + 30);
        h + self.hill(p) * smoothstep(8.0, 40.0, d)
    }

    pub fn height(&self, p: Vec2) -> f32 {
        self.height_wide(p, 0.0)
    }

    /// Cells along the side of the ground's tile about `mid`, which
    /// is `side` wide: more where a river runs through it.
    pub fn cells(&self, mid: Vec2, side: f32) -> usize {
        let cells = self.set.land.tile_cells;
        if self.river_dist(mid) < side * 0.75 {
            cells.1
        } else {
            cells.0
        }
        .max(1)
    }

    /// The height of the ground as it is drawn up close: flat
    /// triangles between the points its finest tiles take from
    /// [`Self::height`]. What stands on the ground stands on this.
    pub fn surface(&self, p: Vec2) -> f32 {
        let chunk = self.set.land.chunk;
        let lo = (p / chunk).floor() * chunk;
        let cells = self.cells(lo + chunk * 0.5, chunk);
        let step = chunk / cells as f32;
        let cell = ((p - lo) / step)
            .floor()
            .clamp(Vec2::ZERO, Vec2::splat(cells as f32 - 1.0));
        let f = ((p - lo) / step - cell).clamp(Vec2::ZERO, Vec2::ONE);
        let h = |i: f32, j: f32| {
            self.height(lo + (cell + Vec2::new(i, j)) * step)
        };
        // A tile's quads are cut from their first corner to the
        // one across.
        let (h00, h11) = (h(0.0, 0.0), h(1.0, 1.0));
        if f.y >= f.x {
            let h01 = h(0.0, 1.0);
            h00 + (h11 - h01) * f.x + (h01 - h00) * f.y
        } else {
            let h10 = h(1.0, 0.0);
            h00 + (h10 - h00) * f.x + (h11 - h10) * f.y
        }
    }

    /// How far `p` lies inside the island's coast, below zero out at
    /// sea. The island is the land within reach of the middle and of
    /// every town, the reaches run smoothly together.
    pub(crate) fn inside(&self, p: Vec2) -> f32 {
        let set = &self.set.land;
        let near = self
            .shores()
            .map(|at| at.distance(p))
            .fold(f32::MAX, |a, b| {
                smooth_min(a, b, set.island * 0.3)
            });
        set.island
            - near
            - set.coast.1 * perlin(p * set.coast.0, self.seed + 40)
    }

    /// The ground `h` high at `p` as the coast leaves it. The island
    /// is one stroke: level inland, its rim falling in a skirt to the
    /// sea floor, steep in some stretches and gentle in others. Out
    /// at sea the floor shelves away from the shore, and here and
    /// there a small stroke stands out of the water.
    fn shore(&self, p: Vec2, h: f32) -> f32 {
        let set = &self.set.land;
        let water = set.water_y;
        let inside = self.inside(p) + self.ragged(p);
        let run = lerp(
            set.rim.0,
            set.rim.1,
            0.5 + 0.5 * perlin(p * 0.012, self.seed + 41),
        );
        let floor = lerp(
            water - set.shelf,
            water - set.sea_floor,
            smoothstep(0.0, -40.0, inside),
        );
        let ground =
            floor + (h - floor) * stroke(run - inside, 0.0, run);
        if (-30.0..run).contains(&inside) {
            ground.max(self.stack(p, floor))
        } else {
            ground
        }
    }

    /// The rock standing out of the sea at `p`, if one does, over a
    /// floor `floor` high: a small stroke with a narrow top and steep
    /// sides. Far under the floor where none stands.
    fn stack(&self, p: Vec2, floor: f32) -> f32 {
        let set = &self.set.land;
        let (side, share, tall) = set.stacks;
        let cell = (p / side).floor();
        let mut top = f32::MIN;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let (x, z) = (cell.x as i32 + dx, cell.y as i32 + dz);
                let h = |k: u32| {
                    unit(key(&[
                        self.seed + 43,
                        x as u32,
                        z as u32,
                        k,
                    ]))
                };
                if h(0) > share {
                    continue;
                }
                let at = (Vec2::new(x as f32, z as f32)
                    + 0.2
                    + Vec2::new(h(1), h(2)) * 0.6)
                    * side;
                if !(-24.0..=-5.0).contains(&self.inside(at)) {
                    continue;
                }
                let flat = 0.8 + 1.4 * h(3);
                let height = set.water_y + tall * (0.4 + 0.6 * h(4));
                let rise =
                    stroke(at.distance(p), flat, 2.5 + 1.5 * h(5));
                top = top.max(floor + (height - floor) * rise);
            }
        }
        top
    }

    /// The height with every river `wide` wider on each side, for
    /// ground drawn too coarsely to hold a river's true bed.
    pub(crate) fn height_wide(&self, p: Vec2, wide: f32) -> f32 {
        let d = self.river_dist(p);
        let mut h = self.wild(p, d);

        let ds = self.site_dist(p);
        let gentle = smoothstep(
            self.set.town.radius + 15.0,
            self.set.town.radius,
            ds,
        );
        h = lerp(
            h,
            self.set.land.site_y + 0.2 * (h - self.set.land.site_y),
            gentle,
        );
        h = lerp(h, self.set.land.site_y, smoothstep(13.0, 8.0, ds));

        let d = (d - wide).max(0.0);
        h = lerp(0.3, h, smoothstep(4.0, 22.0, d));
        h = lerp(h, -1.3, smoothstep(5.2, 2.2, d));
        h = self.shore(p, h);
        for hold in &self.holds {
            let away = hold.at.distance(p);
            h = lerp(
                h,
                hold.y,
                smoothstep(
                    hold.radius + HOLD_EASE,
                    hold.radius,
                    away,
                ),
            );
        }
        h
    }

    pub fn normal(&self, p: Vec2) -> Vec3 {
        self.normal_wide(p, 0.4, 0.0)
    }

    /// The ground's normal as it is over `e` to each side, with
    /// rivers `wide` wider.
    pub(crate) fn normal_wide(
        &self,
        p: Vec2,
        e: f32,
        wide: f32,
    ) -> Vec3 {
        let h = |dx: f32, dz: f32| {
            self.height_wide(p + Vec2::new(dx, dz), wide)
        };
        Vec3::new(
            h(-e, 0.0) - h(e, 0.0),
            2.0 * e,
            h(0.0, -e) - h(0.0, e),
        )
        .normalize()
    }

    /// `p` on the ground as it is drawn.
    pub fn on_ground(&self, p: Vec2) -> Vec3 {
        Vec3::new(p.x, self.surface(p), p.y)
    }

    pub fn is_water(&self, p: Vec2) -> bool {
        self.height(p) < self.set.land.water_y + 0.12
    }

    /// `Ok` when the ground at `at` suits a town: dry, clear of the
    /// rivers and off the hills.
    pub(crate) fn suits(&self, at: Vec2) -> Result<(), Unfit> {
        let (town, water) = (&self.set.town, self.set.land.water_y);
        // A little nearer than the land's own sites keep, so that
        // the nearest of them suit.
        if self.river_dist(at) < town.radius + 2.5 {
            return Err(Unfit::Water);
        }
        let rim = (0..8).map(|i| {
            at + Vec2::from_angle(i as f32 / 8.0 * TAU) * town.radius
        });
        for p in rim.chain([at]) {
            if self.mound(p) > 0.03 {
                return Err(Unfit::Steep);
            }
            if self.wild(p, self.river_dist(p)) < water + 0.6 {
                return Err(Unfit::Water);
            }
        }
        Ok(())
    }

    /// Distance of `p` out of the home plains, from 0 on them to 1
    /// well beyond.
    fn afar(&self, p: Vec2) -> f32 {
        let plains = self.set.land.plains;
        smoothstep(plains, plains + 500.0, p.length())
    }

    /// 0 in open meadow, 1 in thick forest.
    pub(crate) fn forest(&self, p: Vec2) -> f32 {
        let n = fbm(p * 0.017, 3, self.seed + 70);
        let ds = self.site_dist(p);
        // Far out, whole regions are wooded and others bare.
        let afar = self.afar(p);
        let bare = if afar > 0.0 {
            self.set.land.wilds
                * afar
                * fbm(p * 0.0021, 2, self.seed + 72)
        } else {
            0.0
        };
        smoothstep(-0.12 + bare, 0.22 + bare, n)
            * smoothstep(
                self.set.town.radius + 2.0,
                self.set.town.radius + 12.0,
                ds,
            )
    }

    /// The grass at `p`, for what is coloured on its vertices beside
    /// the ground, as the edge of a road. The ground's own shader
    /// paints the same grass.
    pub fn grass(&self, p: Vec2) -> Vec3 {
        let pal = &self.pal;
        let broad = 0.5 + 0.5 * fbm(p * 0.035, 3, self.seed + 80);
        let fine = 0.5 + 0.5 * perlin(p * 0.33, self.seed + 81);
        let base = pal
            .grass_shade
            .lerp(pal.grass, smoothstep(0.3, 0.62, broad));
        base.lerp(
            pal.grass_sun,
            0.45 * fine * smoothstep(0.35, 0.7, broad),
        )
    }

    /// The share of heath in the grass at `p`: none near the middle,
    /// and far out in some regions.
    pub(crate) fn heath(&self, p: Vec2) -> f32 {
        let afar = self.afar(p);
        if afar <= 0.0 {
            return 0.0;
        }
        let heath = fbm(p * 0.0016, 2, self.seed + 73);
        self.set.land.heath * afar * smoothstep(0.05, 0.4, heath)
    }

    /// What the ground's shader paints the ground with.
    pub fn soil(&self) -> SoilUniform {
        let pal = &self.pal;
        let c = |colour: Vec3| colour.extend(1.0);
        SoilUniform {
            grass_shade: c(pal.grass_shade),
            grass: c(pal.grass),
            grass_sun: c(pal.grass_sun),
            grass_high: c(pal.grass_high),
            rock_dark: c(pal.rock_dark),
            rock: c(pal.rock),
            rock_warm: c(pal.rock_warm),
            snow: c(pal.snow),
            dirt: c(pal.dirt),
            dirt_shade: c(pal.dirt_shade),
            mud: c(pal.mud),
            water: self.set.land.water_y,
            seed: self.seed,
        }
    }
}

/// How much of a stroke's height stands `dist` from its line: all of
/// it on its level top, `flat` wide, then a skirt that falls away as
/// a gaussian and is nearly gone `run` further out.
fn stroke(dist: f32, flat: f32, run: f32) -> f32 {
    let u = (dist - flat).max(0.0) / run.max(0.01);
    (-5.5 * u * u).exp()
}

/// The distance from `p` to the segment from `a` to `b`.
fn segment_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6))
        .clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The lesser of `a` and `b`, rounded off where they are within `k`
/// of each other, so that two rounds of land join in one coast.
fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    lerp(b, a, h) - k * h * (1.0 - h)
}
