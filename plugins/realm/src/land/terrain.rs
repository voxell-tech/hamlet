//! The shape of the land: its rivers, its heights and its colours.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3};

use crate::land::{HOLD_EASE, LandData, Unfit};
use crate::mesh::{lerp, smoothstep};
use crate::noise::{fbm, key, perlin, ridged};

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
        let own = self.seed.wrapping_add(key(&[k as u64]));
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

    /// 0 on the plains, 1 in the high ranges. The plains about the
    /// middle have none.
    pub(crate) fn mountain(&self, p: Vec2) -> f32 {
        let seed = self.seed;
        let set = &self.set.land;
        let edge = p.length()
            + set.edge.1 * perlin(p * set.edge.0, seed + 50);
        let out = smoothstep(set.plains, set.plains + 105.0, edge);
        if out <= 0.0 {
            return 0.0;
        }
        let range = fbm(p * set.range_scale, 2, seed + 53);
        out * smoothstep(set.ranges.0, set.ranges.1, range)
    }

    /// The ground as it lies before any site levels it or a river
    /// cuts it, `d` from the nearest river.
    fn wild(&self, p: Vec2, d: f32) -> f32 {
        let (seed, set) = (self.seed, &self.set.land);
        // Rolling plains, four octaves.
        let mut h = set.plain_y
            + set.hills.1 * fbm(p * set.hills.0, 4, seed + 20)
            + set.grain.1 * perlin(p * set.grain.0, seed + 30);
        let m = self.mountain(p);
        if m > 0.0 {
            // Ridged noise, five octaves, warped so the ridges bend.
            let warp = Vec2::new(
                perlin(p * set.ridge_warp.0, seed + 51),
                perlin(p * set.ridge_warp.0, seed + 52),
            ) * set.ridge_warp.1;
            let peaks =
                ridged((p + warp) * set.peaks.0, 5, seed + 60);
            h += m.powf(1.4)
                * (set.peaks.1 + set.peaks.2 * peaks)
                * smoothstep(8.0, 70.0, d);
        }
        h
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
    /// rivers and out of the mountains.
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
            if self.mountain(p) > 0.03 {
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

    pub fn ground_colour(&self, p: Vec2, y: f32, n: Vec3) -> Vec3 {
        let pal = &self.pal;
        let mut c = self.grass(p);
        // Far out, some regions are heath.
        let afar = self.afar(p);
        if afar > 0.0 {
            let heath = fbm(p * 0.0016, 2, self.seed + 73);
            c = c.lerp(
                pal.grass_high,
                self.set.land.heath
                    * afar
                    * smoothstep(0.05, 0.4, heath),
            );
        }
        c = c.lerp(pal.grass_shade * 0.9, 0.55 * self.forest(p));
        // Thinner, duller grass with height, then rock, then snow.
        c = c.lerp(pal.grass_high, smoothstep(5.0, 18.0, y));
        let grain = perlin(p * 0.21, self.seed + 82);
        let steep = 1.0 - n.y;
        let rock = smoothstep(0.1, 0.24, steep + 0.03 * grain)
            .max(smoothstep(20.0, 34.0, y + 4.0 * grain));
        let stone = pal
            .rock_dark
            .lerp(pal.rock, 0.5 + 0.5 * grain)
            .lerp(pal.rock_warm, smoothstep(-0.3, 0.6, n.x));
        c = c.lerp(stone, rock);
        let snow = smoothstep(36.0, 44.0, y + 5.0 * grain)
            * smoothstep(0.5, 0.25, steep);
        c = c.lerp(pal.snow, snow);
        // A sandy shore, wet at the waterline.
        let shore = smoothstep(
            self.set.land.water_y + 0.75,
            self.set.land.water_y + 0.2,
            y,
        );
        c = c.lerp(pal.dirt.lerp(pal.dirt_shade, 0.4), shore * 0.8);
        c.lerp(
            pal.mud,
            smoothstep(
                self.set.land.water_y + 0.2,
                self.set.land.water_y - 0.3,
                y,
            ),
        )
    }
}
