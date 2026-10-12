//! The ground the roads between towns run over, kept clear of trees.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::Standing;
use crate::generator::society::Pair;
use crate::generator::sync::{Drawn, RoadDrawn};

/// Ground that roads between towns run over, where no tree stands.
#[derive(Resource, Default)]
pub(in crate::generator) struct Lanes {
    cells: HashSet<(i32, i32)>,
    /// The roads it is drawn up for: what is drawn of each, and a
    /// number for the line it runs along.
    noted: HashMap<Pair, (Entity, u64)>,
}

impl Lanes {
    fn cell(p: Vec2) -> (i32, i32) {
        let c = (p * 0.5).floor();
        (c.x as i32, c.y as i32)
    }

    pub(super) fn holds(&self, p: Vec2) -> bool {
        self.cells.contains(&Self::cell(p))
    }

    /// A number for the line a road runs along.
    fn mark(road: &RoadDrawn) -> u64 {
        let mut hasher = DefaultHasher::new();
        for p in &road.route.line.pts {
            (p.x.to_bits(), p.z.to_bits()).hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Notes where a road runs, and adds to `new` the cells it is
    /// the first to run over.
    fn note(
        &mut self,
        pair: Pair,
        road: &RoadDrawn,
        new: &mut HashSet<(i32, i32)>,
    ) {
        self.noted.insert(pair, (road.entity, Self::mark(road)));
        for p in road.route.line.pts.iter().step_by(2) {
            let p = p.xz();
            for off in [
                Vec2::ZERO,
                Vec2::X * 2.0,
                -Vec2::X * 2.0,
                Vec2::Y * 2.0,
                -Vec2::Y * 2.0,
            ] {
                let cell = Self::cell(p + off);
                if self.cells.insert(cell) {
                    new.insert(cell);
                }
            }
        }
    }
}

/// Keeps trees off the roads between towns: notes where those roads
/// run, and fells what stands there.
pub(in crate::generator) fn clear_lanes(
    mut commands: Commands,
    drawn: Res<Drawn>,
    mut lanes: ResMut<Lanes>,
    trees: Query<(Entity, &Transform), With<Standing>>,
) {
    let between =
        || drawn.roads.iter().filter(|(_, r)| r.course.between());
    // Roads that are new, and those drawn anew along the line they
    // had. One that is gone or runs elsewhere has all drawn up
    // again.
    let (mut came, mut again) = (Vec::new(), Vec::new());
    let (mut kept, mut redo) = (0, false);
    for (pair, road) in between() {
        match lanes.noted.get(pair) {
            None => came.push((*pair, road)),
            Some((entity, _)) if *entity == road.entity => kept += 1,
            Some((_, mark)) if *mark == Lanes::mark(road) => {
                kept += 1;
                again.push((*pair, road.entity));
            }
            Some(_) => redo = true,
        }
    }
    redo |= kept < lanes.noted.len();
    if !redo && came.is_empty() && again.is_empty() {
        return;
    }
    let lanes = &mut *lanes;
    let mut new = HashSet::new();
    if redo {
        lanes.cells.clear();
        lanes.noted.clear();
        for (pair, road) in between() {
            lanes.note(*pair, road, &mut new);
        }
    } else {
        for (pair, entity) in again {
            if let Some(noted) = lanes.noted.get_mut(&pair) {
                noted.0 = entity;
            }
        }
        for (pair, road) in came {
            lanes.note(pair, road, &mut new);
        }
    }
    if new.is_empty() {
        return;
    }
    for (tree, at) in &trees {
        if new.contains(&Lanes::cell(at.translation.xz())) {
            commands.entity(tree).try_despawn();
        }
    }
}
