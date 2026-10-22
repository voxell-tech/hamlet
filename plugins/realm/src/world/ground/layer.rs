//! The tiles of one thing that covers the land, built a step at a
//! time and kept in step with the view.

use std::collections::{HashMap, HashSet};

use glam::Vec2;

use crate::world::ground::{DetailSettings, Lies, Sight, Tile};
use crate::world::land::Land;

/// What is drawn of a tile, which can be lowered under the tiles
/// that take its place.
pub trait Lift {
    fn lift(&self, by: f32);
}

/// One tile: what is drawn of it and what is being built.
struct Patch<D, J> {
    drawn: Option<D>,
    job: Option<J>,
    /// The land changed under what is drawn of it.
    old: bool,
    /// It is drawn under the tiles that are taking its place.
    sunk: bool,
    /// The engine refused what was built for it, so it is not built
    /// again.
    refused: bool,
    /// What it was built for, besides the land: the layer's key for
    /// its tile at that time.
    key: u64,
}

impl<D, J> Default for Patch<D, J> {
    fn default() -> Self {
        Self {
            drawn: None,
            job: None,
            old: false,
            sunk: false,
            refused: false,
            key: 0,
        }
    }
}

/// The tiles of one kind of thing that covers the land, each built
/// as a job `J` and drawn as `D`: those the view calls for and those
/// still drawn from the view before.
pub struct Layer<D, J> {
    patches: HashMap<Tile, Patch<D, J>>,
    wanted: HashSet<Tile>,
    /// What each wanted tile is to be built for, besides the land.
    keys: HashMap<Tile, u64>,
    /// The tiles being built, the nearest first.
    jobs: Vec<Tile>,
    /// Something changed since the tiles were last looked over.
    stirred: bool,
}

impl<D, J> Default for Layer<D, J> {
    fn default() -> Self {
        Self {
            patches: HashMap::new(),
            wanted: HashSet::new(),
            keys: HashMap::new(),
            jobs: Vec::new(),
            stirred: false,
        }
    }
}

impl<D: Lift, J> Layer<D, J> {
    /// Forgets the job of `tile`, if it has one.
    fn stop(&mut self, tile: Tile) {
        if let Some(patch) = self.patches.get_mut(&tile)
            && patch.job.take().is_some()
        {
            self.jobs.retain(|job| *job != tile);
        }
    }

    /// Has the tiles within `hold` of `at` built anew.
    pub fn redo(&mut self, chunk: f32, at: Vec2, hold: f32) {
        let near = self
            .patches
            .keys()
            .filter(|tile| {
                let reach = tile.side(chunk) * 0.71 + hold;
                tile.middle(chunk).distance(at) < reach
            })
            .copied()
            .collect::<Vec<_>>();
        for tile in near {
            self.stop(tile);
            if let Some(patch) = self.patches.get_mut(&tile) {
                patch.old = true;
                patch.refused = false;
            }
            self.stirred = true;
        }
    }

    /// Takes as the tiles to draw those of the view the camera is
    /// being taken to, and of the view it has now where the first
    /// has none, so that no two lie one over the other. A tile
    /// depends on the tiles drawn with it by its `key`: one whose
    /// key is no longer what it was built for is built anew.
    pub fn choose(
        &mut self,
        land: &Land,
        detail: &DetailSettings,
        lies: &mut Lies,
        (here, there): (&Sight, &Sight),
        finer: bool,
        key: impl Fn(&HashSet<Tile>, Tile) -> u64,
    ) {
        let patches = &self.patches;
        let mut tiles = |sight: &Sight| {
            super::wanted(land, detail, lies, sight, finer, |tile| {
                patches.get(&tile).is_some_and(|p| p.drawn.is_some())
            })
        };
        let mut wanted =
            tiles(there).into_iter().collect::<HashSet<_>>();
        if here != there {
            let top = detail.tile_levels.max(1) - 1;
            // Every tile that one of those lies under.
            let mut over = HashSet::new();
            for tile in &wanted {
                let mut tile = *tile;
                while tile.level < top {
                    tile = tile.over();
                    if !over.insert(tile) {
                        break;
                    }
                }
            }
            let free = |mut tile: Tile| {
                if over.contains(&tile) {
                    return false;
                }
                loop {
                    if wanted.contains(&tile) {
                        return false;
                    }
                    if tile.level >= top {
                        return true;
                    }
                    tile = tile.over();
                }
            };
            let fill = tiles(here)
                .into_iter()
                .filter(|tile| free(*tile))
                .collect::<Vec<_>>();
            wanted.extend(fill);
        }
        if self.wanted != wanted {
            self.keys = wanted
                .iter()
                .map(|tile| (*tile, key(&wanted, *tile)))
                .collect();
            let stale = self
                .patches
                .iter()
                .filter(|(tile, patch)| {
                    self.keys
                        .get(tile)
                        .is_some_and(|key| patch.key != *key)
                })
                .map(|(tile, _)| *tile)
                .collect::<Vec<_>>();
            for tile in stale {
                self.stop(tile);
                let key = self.keys[&tile];
                if let Some(patch) = self.patches.get_mut(&tile) {
                    patch.key = key;
                    patch.old = patch.drawn.is_some();
                    patch.refused = false;
                }
            }
            self.wanted = wanted;
        }
        self.stirred = true;
    }

    /// Takes out the tiles the view has left, each once what takes
    /// its place is drawn. Until then one that finer tiles replace
    /// lies lower by `sink`, so that they show as they come.
    pub fn retire(&mut self, top: u8, sink: f32) {
        if !self.stirred {
            return;
        }
        let ready = self
            .patches
            .iter()
            .filter(|(_, patch)| patch.drawn.is_some() && !patch.old)
            .map(|(tile, _)| *tile)
            .collect::<HashSet<_>>();
        let wanted = &self.wanted;
        let mut stopped = Vec::new();
        self.patches.retain(|tile, patch| {
            if wanted.contains(tile) {
                if patch.sunk {
                    if let Some(drawn) = &patch.drawn {
                        drawn.lift(0.0);
                    }
                    patch.sunk = false;
                }
                return true;
            }
            // Nobody waits for what was being built for a tile the
            // view has left.
            if patch.job.take().is_some() {
                stopped.push(*tile);
            }
            let Some(drawn) = &patch.drawn else {
                return false;
            };
            let mut over = *tile;
            let mut above = None;
            while over.level < top && above.is_none() {
                over = over.over();
                above = wanted.contains(&over).then_some(over);
            }
            let covered = match above {
                Some(above) => ready.contains(&above),
                None => wanted
                    .iter()
                    .filter(|w| tile.covers(**w))
                    .all(|w| ready.contains(w)),
            };
            if covered {
                return false;
            }
            if above.is_none() && !patch.sunk && sink > 0.0 {
                drawn.lift(-sink);
                patch.sunk = true;
            }
            true
        });
        self.jobs.retain(|tile| !stopped.contains(tile));
    }

    /// Starts the jobs of the tiles that are missing, those nearest
    /// `centre` first, with at most `most` under way at a time.
    /// `start` is given the tile and all that are wanted with it. It
    /// rests once none is missing, until something changes.
    pub fn start(
        &mut self,
        chunk: f32,
        centre: Vec2,
        most: usize,
        start: impl Fn(Tile, &HashSet<Tile>) -> J,
    ) {
        if !self.stirred {
            return;
        }
        let room = most.saturating_sub(self.jobs.len());
        if room == 0 {
            return;
        }
        let mut missing = self
            .wanted
            .iter()
            .filter(|tile| {
                self.patches.get(*tile).is_none_or(|p| {
                    p.job.is_none()
                        && !p.refused
                        && (p.drawn.is_none() || p.old)
                })
            })
            .copied()
            .collect::<Vec<_>>();
        let far = |tile: &Tile| tile.middle(chunk).distance(centre);
        missing.sort_by(|a, b| {
            far(a)
                .total_cmp(&far(b))
                .then((a.level, a.x, a.z).cmp(&(b.level, b.x, b.z)))
        });
        self.stirred = missing.len() > room;
        for tile in missing.into_iter().take(room) {
            let job = start(tile, &self.wanted);
            let key = self.keys.get(&tile).copied().unwrap_or(0);
            let patch = self.patches.entry(tile).or_default();
            patch.job = Some(job);
            patch.key = key;
            self.jobs.push(tile);
        }
    }

    /// What is drawn of each tile.
    pub fn drawn_mut(&mut self) -> impl Iterator<Item = &mut D> {
        self.patches.values_mut().filter_map(|p| p.drawn.as_mut())
    }

    /// The tile of the first job under way, and the job.
    pub fn next(&mut self) -> Option<(Tile, &mut J)> {
        let tile = *self.jobs.first()?;
        let job = self.patches.get_mut(&tile)?.job.as_mut()?;
        Some((tile, job))
    }

    /// Draws `drawn` for the tile of the first job, which is done, in
    /// place of what it had; or, with none, marks the tile as one the
    /// engine refused.
    pub fn finish(&mut self, drawn: Option<D>) {
        if self.jobs.is_empty() {
            return;
        }
        let tile = self.jobs.remove(0);
        self.stirred = true;
        let Some(patch) = self.patches.get_mut(&tile) else {
            return;
        };
        patch.job = None;
        match drawn {
            Some(drawn) => {
                patch.drawn = Some(drawn);
                patch.old = false;
                patch.sunk = false;
            }
            None => patch.refused = true,
        }
    }
}
