//! The ground as tiles: which ones a view calls for, and the mesh
//! and the trees of each. Four tiles of one size lie under one of
//! the next, so near ground is fine and far ground cheap. Tiles are
//! built a row at a time over the frames, nearest first, and taken
//! out when the view has left them (docs/endless-world.md).

mod grove;
mod layer;
mod sod;

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{Material, Mesh, Pbr, Shape};

use crate::build::frame::Frame;
use crate::build::stock::rgba;
use crate::build::upload::{Draw, Unsent};
use crate::realm::{Ctx, Shared};
use crate::world::flora::{self, Tree};
use crate::world::ground::grove::{Grove, GroveJob};
use crate::world::ground::layer::{Layer, TileEntity};
use crate::world::ground::sod::{Recipe, Sod, SodJob};
use crate::world::land::{Land, LandData};
use crate::world::palette::Palette;

/// The most time the ground builds for in one frame, so that what
/// comes after it in the frame has its share.
const SHARE: Duration = Duration::from_millis(5);

/// The camera's sight of the ground.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sight {
    pub eye: Vec3,
    /// The middle of the ground that is drawn, and how far from it
    /// the ground reaches.
    pub centre: Vec2,
    pub reach: f32,
}

/// The extent of the ground drawn and the pace the world fills in at.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetailSettings {
    /// Camera distance beyond which a town is drawn as one plain
    /// mesh.
    pub town_far: f32,
    /// Camera distance beyond which a robot is not drawn.
    pub robots_far: f32,
    /// A robot further than this outside the view is not posed. It
    /// covers the reach of its lantern and of its shadow.
    pub robot_sight: f32,
    /// Distance from the camera's focus to which the ground is
    /// drawn, as a share of the distance at which the fog is whole.
    pub ground_reach: f32,
    /// Sizes of ground tile there are, each twice the side of the
    /// one before.
    pub tile_levels: u8,
    /// A tile is drawn as four finer ones while the camera is nearer
    /// to it than its side times this.
    pub tile_split: f32,
    /// Distance past that at which the four stay once drawn, so a
    /// camera at the line does not swap them back and forth.
    pub tile_keep: f32,
    /// Factors by which a tile with a river in it, and one with a
    /// town on it, is drawn finer further off.
    pub river_split: f32,
    pub town_split: f32,
    /// Depth of a tile of the second size under the true ground, so
    /// that roads and floors are not buried where the ground is
    /// drawn coarsely. Each size up doubles it, to the most given.
    pub tile_sink: (f32, f32),
    /// Drop of a tile that is being replaced under the tiles that
    /// take its place.
    pub stale_sink: f32,
    /// Depth under the water's level of the plain ground that shows
    /// where no tile is drawn yet. Deeper than any river bed.
    pub bed: f32,
    /// Sizes of tile, from the finest, on which every tree stands as
    /// itself. On larger ones a tile's trees are one plain mesh.
    pub tree_levels: u8,
    /// Sizes of tile, from the finest, on which those plain trees
    /// still have a crown. Beyond, each is a speck, and each size up
    /// keeps half as many.
    pub crown_levels: u8,
    /// Tiles of ground built at one time, and as many of trees.
    pub tile_tasks: usize,
    /// The most trees planted in one frame.
    pub trees_per_frame: usize,
    /// The most finished houses, roads and commons put into the
    /// world in one frame.
    pub builds_per_frame: usize,
}

/// One square of ground: its size, counted from the finest, and its
/// place among the tiles of that size.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Tile {
    pub level: u8,
    pub x: i32,
    pub z: i32,
}

impl Tile {
    pub fn side(self, chunk: f32) -> f32 {
        chunk * (1u32 << self.level) as f32
    }

    /// Its corner of least x and z.
    pub fn corner(self, chunk: f32) -> Vec2 {
        Vec2::new(self.x as f32, self.z as f32) * self.side(chunk)
    }

    pub fn middle(self, chunk: f32) -> Vec2 {
        self.corner(chunk) + self.side(chunk) * 0.5
    }

    /// The four tiles that lie under it.
    fn quarters(self) -> [Tile; 4] {
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dz)| Tile {
            level: self.level - 1,
            x: self.x * 2 + dx,
            z: self.z * 2 + dz,
        })
    }

    /// The tile of the next size that it lies under.
    pub fn over(self) -> Tile {
        Tile {
            level: self.level + 1,
            x: self.x.div_euclid(2),
            z: self.z.div_euclid(2),
        }
    }

    /// True when `other` is this tile or lies under it.
    pub fn covers(self, other: Tile) -> bool {
        if other.level > self.level {
            return false;
        }
        let shift = self.level - other.level;
        (other.x >> shift, other.z >> shift) == (self.x, self.z)
    }

    /// The tile of `drawn`, of this size or larger, that covers the
    /// place `by` tiles of this size away. None where finer tiles
    /// lie there, or nothing does.
    fn beside(
        self,
        drawn: &HashSet<Tile>,
        top: u8,
        by: (i32, i32),
    ) -> Option<Tile> {
        let mut tile = Tile {
            level: self.level,
            x: self.x + by.0,
            z: self.z + by.1,
        };
        loop {
            if drawn.contains(&tile) {
                return Some(tile);
            }
            if tile.level >= top {
                return None;
            }
            tile = tile.over();
        }
    }

    /// Distance over the ground from `p` to the tile.
    fn away(self, chunk: f32, p: Vec2) -> f32 {
        let (lo, side) = (self.corner(chunk), self.side(chunk));
        (p - p.clamp(lo, lo + side)).length()
    }
}

/// The places about a tile: across each edge and each corner.
const ABOUT: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// A number that changes when the tiles of `drawn` that the rim of
/// `tile` is cut to do: those about it that are no smaller, and
/// those about each of them that is larger. `top` is the largest
/// size there is. A tile built for one number is right for as long
/// as the number holds.
fn rim_key(drawn: &HashSet<Tile>, top: u8, tile: Tile) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut open = vec![tile];
    let mut at = 0;
    while let Some(&of) = open.get(at) {
        at += 1;
        for by in ABOUT {
            let beside = of.beside(drawn, top, by);
            beside.hash(&mut hasher);
            if let Some(beside) = beside
                && beside.level > of.level
                && !open.contains(&beside)
            {
                open.push(beside);
            }
        }
    }
    hasher.finish()
}

/// True when a river runs through `tile`.
fn watered(land: &Land, tile: Tile) -> bool {
    let chunk = land.set.land.chunk;
    land.river_dist(tile.middle(chunk)) < tile.side(chunk) * 0.75
}

/// What the choice of tiles asks of each tile's ground, kept from one
/// choice to the next for the tiles in sight until the land changes:
/// whether a river runs through it, and whether a town stands on it.
#[derive(Default)]
struct Lies(HashMap<Tile, (bool, bool)>);

impl Lies {
    fn lie(&mut self, land: &Land, tile: Tile) -> (bool, bool) {
        *self.0.entry(tile).or_insert_with(|| {
            let chunk = land.set.land.chunk;
            let (side, mid) = (tile.side(chunk), tile.middle(chunk));
            let hold = land.set.town.radius + 15.0;
            let town = land
                .sites
                .iter()
                .any(|s| s.at.distance(mid) < side * 0.71 + hold);
            (watered(land, tile), town)
        })
    }

    /// Forgets the tiles out of reach of every one of `sights`.
    fn keep_within(&mut self, chunk: f32, sights: [&Sight; 2]) {
        self.0.retain(|tile, _| {
            sights.iter().any(|sight| {
                tile.away(chunk, sight.centre) <= sight.reach
            })
        });
    }
}

/// The tiles `sight` calls for, each as large as its distance
/// allows. With `finer`, tiles with a river or a town are kept finer
/// than the rest. `drawn` tells which tiles are there now: four that
/// are stay a little longer than they would be asked for anew.
fn wanted(
    land: &Land,
    detail: &DetailSettings,
    lies: &mut Lies,
    sight: &Sight,
    finer: bool,
    drawn: impl Fn(Tile) -> bool,
) -> Vec<Tile> {
    let chunk = land.set.land.chunk;
    let top = detail.tile_levels.max(1) - 1;
    let side = chunk * (1u32 << top) as f32;
    let lo = ((sight.centre - sight.reach) / side).floor();
    let hi = ((sight.centre + sight.reach) / side).floor();
    let mut out = Vec::new();
    let mut open = Vec::new();
    for x in lo.x as i32..=hi.x as i32 {
        for z in lo.y as i32..=hi.y as i32 {
            open.push(Tile { level: top, x, z });
        }
    }
    while let Some(tile) = open.pop() {
        if tile.away(chunk, sight.centre) > sight.reach {
            continue;
        }
        let side = tile.side(chunk);
        let far =
            Vec2::new(tile.away(chunk, sight.eye.xz()), sight.eye.y)
                .length();
        let kept =
            tile.level > 0 && tile.quarters().into_iter().any(&drawn);
        let near = side
            * detail.tile_split
            * if kept { detail.tile_keep } else { 1.0 };
        // Whether a river runs through it or a town stands on it is
        // asked only where it would make a difference.
        let finest = detail.town_split.max(detail.river_split);
        let split = tile.level > 0
            && (far < near
                || (finer && far < near * finest && {
                    let (river, town) = lies.lie(land, tile);
                    (river && far < near * detail.river_split)
                        || (town && far < near * detail.town_split)
                }));
        if split {
            open.extend(tile.quarters());
        } else {
            out.push(tile);
        }
    }
    out
}

/// A tile's ground as it is drawn: its mesh, and under it the water
/// where it dips under the water's level.
struct Turf {
    entity: Entity,
}

/// A tile's trees as they are drawn: those that stand as themselves
/// as copies of each kind's mesh under it, or all as one plain mesh.
struct Wood {
    entity: Entity,
    /// Copies not yet given, with the mesh of their kind.
    waiting: Vec<(Handle<Mesh>, Vec<Transform>)>,
}

impl TileEntity for Turf {
    fn entity(&self) -> Entity {
        self.entity
    }
}

impl TileEntity for Wood {
    fn entity(&self) -> Entity {
        self.entity
    }
}

/// The ground drawn about the camera.
#[derive(Resource)]
pub struct Ground {
    sods: Layer<Turf, SodJob>,
    woods: Layer<Wood, GroveJob>,
    lies: Lies,
    /// The plain ground that shows where no tile is drawn yet, and
    /// where it was last put.
    bed: Entity,
    bed_at: Option<Frame>,
    /// A square for a tile's water, and the water.
    pond: Handle<Mesh>,
    water: Handle<Material>,
    /// The mesh of each kind of tree, as they are handed over; none
    /// for one the engine refused.
    kinds: Vec<Option<Handle<Mesh>>>,
    /// The views the tiles were last chosen for: the camera's, and
    /// the one it is being taken to.
    sight: Option<(Sight, Sight)>,
    /// The place of each site at that time, and the ground then
    /// held.
    sites: Vec<Vec2>,
    holds: Vec<(Vec2, f32)>,
    /// Trees go first in the next frame, so the ground does not keep
    /// them waiting.
    trees_first: bool,
}

impl Ground {
    /// No tiles yet, only the plain ground under where they will be.
    pub fn make(
        pal: &Palette,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let plain = pal.grass_shade.lerp(pal.grass, 0.85);
        let bed_mesh =
            draw.assets.add(Mesh::shape(Shape::Plane((2.0, 2.0)))?);
        let bed_look = draw.material(Pbr {
            roughness: 0.92,
            ..Pbr::colored(rgba(plain))
        })?;
        let pond =
            draw.assets.add(Mesh::shape(Shape::Plane((1.0, 1.0)))?);
        let water = draw.material(Pbr {
            roughness: 0.22,
            ..Pbr::colored(rgba(pal.river))
        })?;
        let bed = draw.spawn(None, 0);
        draw.draw(bed, &bed_mesh, &bed_look);
        draw.shadows(bed, false, true);
        Ok(Self {
            sods: Layer::default(),
            woods: Layer::default(),
            lies: Lies::default(),
            bed,
            bed_at: None,
            pond,
            water,
            kinds: Vec::new(),
            sight: None,
            sites: Vec::new(),
            holds: Vec::new(),
            trees_first: false,
        })
    }

    /// Keeps the ground and the trees drawn about where the camera
    /// looks and where it is going: chooses the tiles `sights` call
    /// for, builds those missing a step at a time within what the
    /// call may spend, and takes out those the view has left behind.
    pub fn update(
        &mut self,
        ctx: &Ctx,
        sights: &[Sight],
        draw: &mut Draw,
    ) {
        let Some(sight) = sights.first().copied() else {
            return;
        };
        let going = sights.get(1).copied().unwrap_or(sight);
        let (land, detail) = (ctx.land, &ctx.tuning.detail);
        let chunk = land.set.land.chunk;
        let until = Instant::now() + SHARE;
        self.lay_bed(ctx, &sight, draw);
        let changed = self.follow_land(land);
        let moved = |was: &Sight, now: &Sight| {
            was.eye.distance(now.eye) > 0.5
                || was.centre.distance(now.centre) > 0.5
                || (was.reach - now.reach).abs() > 1.0
        };
        let looked = self.sight.is_none_or(|was| {
            moved(&was.0, &sight) || moved(&was.1, &going)
        });
        let top = detail.tile_levels.max(1) - 1;
        if looked || changed {
            let views = (&sight, &going);
            // A tile of ground is cut to the tiles drawn beside it.
            self.sods.choose(
                land,
                detail,
                &mut self.lies,
                views,
                true,
                |drawn, tile| rim_key(drawn, top, tile),
            );
            self.woods.choose(
                land,
                detail,
                &mut self.lies,
                views,
                false,
                |_, _| 0,
            );
            self.lies.keep_within(chunk, [&sight, &going]);
            self.sight = Some((sight, going));
        }
        self.sods.retire(top, detail.stale_sink, draw);
        self.woods.retire(top, 0.0, draw);
        self.sods.start(
            chunk,
            going.centre,
            detail.tile_tasks,
            |tile, drawn| {
                SodJob::new(Recipe::of(land, detail, tile), drawn)
            },
        );
        if self.kinds.len() == flora::KINDS {
            self.woods.start(
                chunk,
                going.centre,
                detail.tile_tasks,
                |tile, _| GroveJob::new(land, detail, tile),
            );
        }
        // True while the ground's share of the frame lasts.
        let left = || Instant::now() < until;
        if self.trees_first {
            self.plant(ctx, draw, &left);
            self.lay(ctx, draw, &left);
        } else {
            self.lay(ctx, draw, &left);
            self.plant(ctx, draw, &left);
        }
        self.trees_first = !self.trees_first;
    }

    /// Keeps the plain ground under where the camera looks.
    fn lay_bed(&mut self, ctx: &Ctx, sight: &Sight, draw: &mut Draw) {
        let land = ctx.land;
        let far = ctx.tuning.camera.far;
        let next = Frame::at(Vec3::new(
            sight.centre.x,
            land.set.land.water_y - ctx.tuning.detail.bed,
            sight.centre.y,
        ))
        .with_size(Vec3::splat(far * 2.0));
        if self.bed_at != Some(next) {
            draw.place(self.bed, next);
            self.bed_at = Some(next);
        }
    }

    /// Has the ground a site came to or left built anew, and the
    /// ground that came to be held or was let go, with the trees
    /// about each. True when the land changed.
    fn follow_land(&mut self, land: &Land) -> bool {
        let chunk = land.set.land.chunk;
        let mut changed = false;
        if !land.sites.iter().map(|s| &s.at).eq(&self.sites) {
            let sites =
                land.sites.iter().map(|s| s.at).collect::<Vec<_>>();
            let hold = land.set.town.radius + 17.0;
            for i in 0..sites.len().max(self.sites.len()) {
                let (now, was) = (sites.get(i), self.sites.get(i));
                if now == was {
                    continue;
                }
                for at in [now, was].into_iter().flatten() {
                    self.sods.redo(chunk, *at, hold);
                    self.woods.redo(chunk, *at, hold);
                }
            }
            self.sites = sites;
            changed = true;
        }
        if !land.rounds().eq(self.holds.iter().copied()) {
            let holds = land.held();
            for round in holds.iter().chain(&self.holds) {
                if holds.contains(round) && self.holds.contains(round)
                {
                    continue;
                }
                let reach = LandData::hold_reach(round.1) + 17.0;
                self.sods.redo(chunk, round.0, reach);
                self.woods.redo(chunk, round.0, reach);
            }
            self.holds = holds;
            changed = true;
        }
        if changed {
            self.lies = Lies::default();
        }
        changed
    }

    /// Builds the ground of the nearest tiles missing it, a row at a
    /// time while `left` says so, and draws each once it is built.
    fn lay(
        &mut self,
        ctx: &Ctx,
        draw: &mut Draw,
        left: &impl Fn() -> bool,
    ) {
        while left() && draw.left() {
            let Some((tile, job)) = self.sods.next() else {
                return;
            };
            let Some(sod) = job.step(ctx.land) else {
                continue;
            };
            match self.turf(ctx, tile, sod, draw) {
                Ok(turf) => self.sods.finish(Some(turf), draw),
                Err(Unbuilt::Full(sod)) => {
                    if let Some((_, job)) = self.sods.next() {
                        job.wait(sod);
                    }
                    return;
                }
                Err(Unbuilt::Refused(why)) => {
                    hamlet_engine_bevy::warn(&format!(
                        "ground: {why}"
                    ));
                    self.sods.finish(None, draw);
                }
            }
        }
    }

    /// The ground of `tile` as `sod` has it, drawn.
    fn turf(
        &self,
        ctx: &Ctx,
        tile: Tile,
        sod: Sod,
        draw: &mut Draw,
    ) -> Result<Turf, Unbuilt<Sod>> {
        let Sod { mesh, wet } = sod;
        let mesh =
            draw.mesh(mesh).map_err(|unsent| match unsent {
                Unsent::Full(mesh) => {
                    Unbuilt::Full(Sod { mesh, wet })
                }
                Unsent::Refused(why) => Unbuilt::Refused(why),
            })?;
        let entity = draw.spawn(None, 0);
        draw.draw(entity, &mesh, &ctx.stock.matte);
        draw.shadows(entity, false, true);
        // Water lies only where the ground dips under it.
        if wet {
            let land = ctx.land;
            let chunk = land.set.land.chunk;
            let (mid, side) = (tile.middle(chunk), tile.side(chunk));
            let water = draw.spawn(Some(entity), 0);
            draw.place(
                water,
                Frame::at(Vec3::new(
                    mid.x,
                    land.set.land.water_y,
                    mid.y,
                ))
                .with_size(Vec3::new(side, 1.0, side)),
            );
            draw.draw(water, &self.pond, &self.water);
            draw.shadows(water, false, true);
        }
        Ok(Turf { entity })
    }

    /// Finds the trees of the nearest tiles missing them, a column at
    /// a time while `left` says so, and plants each tile's at once.
    /// The meshes of the kinds of tree are handed over first.
    fn plant(
        &mut self,
        ctx: &Ctx,
        draw: &mut Draw,
        left: &impl Fn() -> bool,
    ) {
        while self.kinds.len() < flora::KINDS {
            if !left() || !draw.left() {
                return;
            }
            match flora::mesh(ctx.pal, self.kinds.len()).upload(draw)
            {
                Ok(mesh) => self.kinds.push(Some(mesh)),
                Err(Unsent::Full(_)) => return,
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_bevy::warn(&format!(
                        "trees: {why}"
                    ));
                    self.kinds.push(None);
                }
            }
        }
        let mut room = ctx.tuning.detail.trees_per_frame;
        for wood in self.woods.drawn_mut() {
            if !wood.waiting.is_empty() {
                wood.give(ctx, draw, &mut room);
            }
        }
        while left() && draw.left() && room > 0 {
            let Some((_, job)) = self.woods.next() else {
                return;
            };
            let Some(grove) = job.step(ctx.land) else {
                continue;
            };
            match self.wood(ctx, grove, draw, &mut room) {
                Ok(wood) => self.woods.finish(Some(wood), draw),
                Err(Unbuilt::Full(grove)) => {
                    if let Some((_, job)) = self.woods.next() {
                        job.wait(grove);
                    }
                    return;
                }
                Err(Unbuilt::Refused(why)) => {
                    hamlet_engine_bevy::warn(&format!(
                        "trees: {why}"
                    ));
                    self.woods.finish(None, draw);
                }
            }
        }
    }

    /// The trees of `grove`, drawn. The copies of a kind that find no
    /// room in this frame are given in a later one.
    fn wood(
        &self,
        ctx: &Ctx,
        grove: Grove,
        draw: &mut Draw,
        room: &mut usize,
    ) -> Result<Wood, Unbuilt<Grove>> {
        let Grove { trees, canopy } = grove;
        let canopy = match canopy {
            Some(canopy) => match draw.mesh(canopy) {
                Ok(mesh) => Some(mesh),
                Err(Unsent::Full(canopy)) => {
                    return Err(Unbuilt::Full(Grove {
                        trees,
                        canopy: Some(canopy),
                    }));
                }
                Err(Unsent::Refused(why)) => {
                    return Err(Unbuilt::Refused(why));
                }
            },
            None => None,
        };
        let entity = draw.spawn(None, 0);
        if let Some(canopy) = &canopy {
            draw.draw(entity, canopy, &ctx.stock.matte);
        }
        // The copies of each kind.
        let mut kinds = vec![Vec::new(); flora::KINDS];
        for tree in &trees {
            kinds[tree.kind].push(tree.frame().transform());
        }
        let mut wood = Wood {
            entity,
            waiting: Vec::new(),
        };
        for (kind, placed) in kinds.into_iter().enumerate() {
            let Some(mesh) = &self.kinds[kind] else {
                continue;
            };
            if placed.is_empty() {
                continue;
            }
            wood.waiting.push((mesh.clone(), placed));
        }
        wood.give(ctx, draw, room);
        Ok(wood)
    }
}

impl Wood {
    /// Gives each kind its copies while the frame has time and
    /// `room` trees left to plant, taking what it plants
    /// from `room`.
    fn give(&mut self, ctx: &Ctx, draw: &mut Draw, room: &mut usize) {
        let entity = self.entity;
        self.waiting.retain_mut(|(mesh, copies)| {
            if *room == 0 || !draw.left() {
                return true;
            }
            *room = room.saturating_sub(copies.len());
            draw.commands.spawn((
                Instances {
                    mesh: mesh.clone(),
                    material: ctx.stock.matte.clone(),
                    copies: std::mem::take(copies),
                },
                ChildOf(entity),
            ));
            false
        });
    }
}

/// Keeps the ground and the trees drawn about where the camera looks.
pub fn build_ground(
    mut ground: ResMut<Ground>,
    shared: Shared,
    mut draw: Draw,
) {
    let floor = shared.floor();
    let ctx = shared.ctx(&floor);
    ground.update(&ctx, &shared.now.sights, &mut draw);
}

/// Something built that was not drawn.
enum Unbuilt<T> {
    /// The frame had no time left to hand it over: it waits for the
    /// next.
    Full(T),
    /// The engine refused it, saying why.
    Refused(String),
}

/// The realm's methods on [`Tree`].
trait TreeExt {
    /// Its place, turn and size.
    fn frame(&self) -> Frame;
}

impl TreeExt for Tree {
    fn frame(&self) -> Frame {
        Frame::at(self.at)
            .with_turn(Quat::from_rotation_y(self.yaw))
            .with_size(Vec3::splat(self.scale))
    }
}
