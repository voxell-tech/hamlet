//! The tiles of one thing that covers the land, built in background
//! tasks and kept in step with the view.

use std::collections::HashSet;
use std::sync::Arc;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::tasks::futures_lite::future::poll_once;
use bevy::tasks::{Task, block_on};

use super::Ground;
use crate::generator::DetailSettings;
use crate::generator::ground::{self, Sight, Tile};
use crate::generator::land::Land;

/// One tile: what is drawn of it and what is being built.
pub(super) struct Patch<B> {
    drawn: Option<Entity>,
    /// The tile that takes its place: in the world, hidden, and not
    /// whole yet. `drawn` stays until it is.
    coming: Option<Entity>,
    task: Option<Task<B>>,
    /// The land changed under what is drawn of it.
    old: bool,
    /// It is drawn under the tiles that are taking its place.
    sunk: bool,
    /// Bytes of mesh data in what is drawn of it.
    pub(super) bytes: usize,
    /// What it was built for, besides the land: the layer's key
    /// for its tile at that time.
    key: u64,
}

impl<B> Default for Patch<B> {
    fn default() -> Self {
        Self {
            drawn: None,
            coming: None,
            task: None,
            old: false,
            sunk: false,
            bytes: 0,
            key: 0,
        }
    }
}

/// The tiles of one kind of thing that covers the land, each built
/// in a background task as `B`: those the view calls for and those
/// still drawn from the view before.
pub(super) struct Layer<B> {
    patches: HashMap<Tile, Patch<B>>,
    wanted: Arc<HashSet<Tile>>,
    /// What each wanted tile is to be built for, besides the land.
    keys: HashMap<Tile, u64>,
    /// The tiles that were coming for a tile whose land changed
    /// under it.
    dropped: Vec<Entity>,
    /// Tiles that are being built.
    pub(super) building: usize,
    /// Something changed since the tiles were last looked over.
    stirred: bool,
    /// Every tile the view calls for is drawn as the land now is.
    pub(super) settled: bool,
}

impl<B> Default for Layer<B> {
    fn default() -> Self {
        Self {
            patches: HashMap::default(),
            wanted: Arc::default(),
            keys: HashMap::default(),
            dropped: Vec::new(),
            building: 0,
            stirred: false,
            settled: true,
        }
    }
}

impl<B: Send + 'static> Layer<B> {
    pub(super) fn settle(&mut self) {
        self.settled = self.wanted.iter().all(|tile| {
            self.patches
                .get(tile)
                .is_some_and(|p| p.drawn.is_some() && !p.old)
        });
    }

    pub(super) fn drawn(&self) -> impl Iterator<Item = &Patch<B>> {
        self.patches.values().filter(|p| p.drawn.is_some())
    }

    /// Has the tiles within `hold` of `at` built anew.
    pub(super) fn redo(&mut self, chunk: f32, at: Vec2, hold: f32) {
        for (tile, patch) in &mut self.patches {
            let reach = tile.side(chunk) * 0.71 + hold;
            if tile.middle(chunk).distance(at) < reach {
                patch.old = true;
                if patch.task.take().is_some() {
                    self.building -= 1;
                }
                self.dropped.extend(patch.coming.take());
                self.stirred = true;
            }
        }
    }

    /// Takes as the tiles to draw those of the view the camera is
    /// being taken to, and of the view it has now where the first
    /// has none, so that no two lie one over the other. A tile
    /// depends on the tiles drawn with it by its `key`: one whose
    /// key is no longer what it was built for is built anew.
    pub(super) fn choose(
        &mut self,
        land: &Land,
        detail: &DetailSettings,
        (here, there): (&Sight, &Sight),
        finer: bool,
        key: impl Fn(&HashSet<Tile>, Tile) -> u64,
    ) {
        let patches = &self.patches;
        let tiles = |sight: &Sight| {
            ground::wanted(land, detail, sight, finer, |tile| {
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
        if *self.wanted != wanted {
            self.keys = wanted
                .iter()
                .map(|tile| (*tile, key(&wanted, *tile)))
                .collect();
            for (tile, patch) in &mut self.patches {
                let Some(key) = self.keys.get(tile) else {
                    continue;
                };
                if patch.key == *key {
                    continue;
                }
                patch.key = *key;
                patch.old = patch.drawn.is_some();
                if patch.task.take().is_some() {
                    self.building -= 1;
                }
                self.dropped.extend(patch.coming.take());
            }
            self.wanted = Arc::new(wanted);
        }
        self.stirred = true;
    }

    /// Puts the built tiles into the world with `place`, which
    /// returns the entity, the bytes of its meshes and true if it is
    /// whole. A tile that is not whole is spawned hidden, and the one
    /// it replaces stays drawn until [`Layer::whole`] is told.
    /// Returns how many came.
    pub(super) fn land(
        &mut self,
        commands: &mut Commands,
        mut place: impl FnMut(
            &mut Commands,
            Tile,
            B,
        ) -> (Entity, usize, bool),
    ) -> usize {
        if self.building == 0 {
            return 0;
        }
        let mut came = 0;
        for (tile, patch) in &mut self.patches {
            let Some(task) = &mut patch.task else {
                continue;
            };
            let Some(built) = block_on(poll_once(task)) else {
                continue;
            };
            patch.task = None;
            self.building -= 1;
            self.stirred = true;
            if !self.wanted.contains(tile) {
                continue;
            }
            let (entity, bytes, whole) =
                place(commands, *tile, built);
            if let Some(old) = patch.coming.take() {
                commands.entity(old).try_despawn();
            }
            if whole {
                if let Some(old) = patch.drawn.take() {
                    commands.entity(old).try_despawn();
                }
                *patch = Patch {
                    drawn: Some(entity),
                    bytes,
                    key: patch.key,
                    ..default()
                };
            } else {
                patch.coming = Some(entity);
                patch.bytes = bytes;
            }
            came += 1;
        }
        came
    }

    /// Shows what was coming as `entity` in place of what its tile
    /// had drawn, now that it is whole.
    pub(super) fn whole(
        &mut self,
        commands: &mut Commands,
        entity: Entity,
    ) {
        let Some(patch) = self
            .patches
            .values_mut()
            .find(|patch| patch.coming == Some(entity))
        else {
            return;
        };
        if let Some(old) = patch.drawn.take() {
            commands.entity(old).try_despawn();
        }
        patch.drawn = patch.coming.take();
        patch.old = false;
        patch.sunk = false;
        commands.entity(entity).try_insert(Visibility::Inherited);
        self.stirred = true;
        self.settle();
    }

    /// Takes out the tiles the view has left, each once what takes
    /// its place is drawn. Until then one that finer tiles replace
    /// lies lower by `sink`, so that they show as they come.
    pub(super) fn retire(
        &mut self,
        commands: &mut Commands,
        top: u8,
        sink: f32,
        places: &mut Query<&mut Transform, With<Ground>>,
    ) {
        if !self.stirred {
            return;
        }
        for gone in self.dropped.drain(..) {
            commands.entity(gone).try_despawn();
        }
        let wanted = &*self.wanted;
        let building = &mut self.building;
        let ready = self
            .patches
            .iter()
            .filter(|(_, patch)| patch.drawn.is_some() && !patch.old)
            .map(|(tile, _)| *tile)
            .collect::<HashSet<_>>();
        self.patches.retain(|tile, patch| {
            let mut lift = |patch: &mut Patch<B>, by: f32| {
                if let Some(mut at) =
                    patch.drawn.and_then(|e| places.get_mut(e).ok())
                {
                    at.translation.y = by;
                }
            };
            if wanted.contains(tile) {
                if patch.sunk {
                    lift(patch, 0.0);
                    patch.sunk = false;
                }
                return true;
            }
            // Nobody waits for what was coming to a tile the view
            // has left, or for what was being built for it.
            if let Some(coming) = patch.coming.take() {
                commands.entity(coming).try_despawn();
            }
            if patch.task.take().is_some() {
                *building -= 1;
            }
            let Some(entity) = patch.drawn else {
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
                    .filter(|w| tile.is_over(**w))
                    .all(|w| ready.contains(w)),
            };
            if covered {
                commands.entity(entity).try_despawn();
                return false;
            }
            if above.is_none() && !patch.sunk && sink > 0.0 {
                lift(patch, -sink);
                patch.sunk = true;
            }
            true
        });
    }

    /// Starts building the tiles that are missing, those nearest
    /// `centre` first, with at most `most` being built at a time.
    /// `start` is given the tile and all that are wanted with it.
    /// It rests once none is missing, until something changes.
    pub(super) fn build(
        &mut self,
        chunk: f32,
        centre: Vec2,
        most: usize,
        start: impl Fn(Tile, &Arc<HashSet<Tile>>) -> Task<B>,
    ) {
        if !self.stirred {
            return;
        }
        self.settle();
        let room = most.saturating_sub(self.building);
        if room == 0 {
            return;
        }
        let mut missing = self
            .wanted
            .iter()
            .filter(|tile| {
                self.patches.get(*tile).is_none_or(|p| {
                    p.task.is_none()
                        && p.coming.is_none()
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
            let patch = self.patches.entry(tile).or_default();
            patch.task = Some(start(tile, &self.wanted));
            patch.key = self.keys.get(&tile).copied().unwrap_or(0);
            self.building += 1;
        }
    }
}
