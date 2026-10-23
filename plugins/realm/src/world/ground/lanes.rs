//! The ground the roads between towns run over, kept clear of trees.
//! Each road's line is found as the road's own is, a step of the
//! search at a time.

use std::collections::{HashMap, HashSet};

use glam::{Vec2, Vec3Swizzles};

use crate::build::mesh::Line;
use crate::society::{Pair, Society};
use crate::world::land::{Land, Search};
use crate::world::layout::{End, Route};

/// Crossings a step of a search looks at.
const CROSSINGS_A_STEP: usize = 64;

/// Towns further apart than this have no road between them.
const LONGEST: f32 = 3_000.0;

/// A road between towns: its ends, the first the lesser, and the
/// places of their towns.
type Course = ((End, End), Vec2, Vec2);

/// Ground that roads between towns run over, where no tree stands.
#[derive(Default)]
pub struct Lanes {
    cells: HashSet<(i32, i32)>,
    /// The line of each road, with what it was found for.
    lines: HashMap<Pair, (Course, Vec<Vec2>)>,
    /// The road whose line is being found.
    finding: Option<(Pair, Course, Search)>,
    /// The ground held when the lines were found.
    holds: Vec<(Vec2, f32)>,
}

impl Lanes {
    fn cell(p: Vec2) -> (i32, i32) {
        let c = (p * 0.5).floor();
        (c.x as i32, c.y as i32)
    }

    /// The line of each road between towns found so far.
    pub fn lines(&self) -> impl Iterator<Item = (Pair, &[Vec2])> {
        self.lines
            .iter()
            .map(|(pair, (_, line))| (*pair, line.as_slice()))
    }

    /// True when a road between towns runs over `p`.
    pub fn holds(&self, p: Vec2) -> bool {
        self.cells.contains(&Self::cell(p))
    }

    /// Keeps up with the roads between towns, finding the lines of
    /// those that are new a step at a time while `left` says so. The
    /// places of the cells that came or went, one in each.
    pub fn follow(
        &mut self,
        land: &Land,
        society: &Society,
        left: impl Fn() -> bool,
    ) -> Vec<Vec2> {
        let wanted = courses(land, society);
        let mut changed = false;
        let holds = land.held();
        if holds != self.holds {
            self.holds = holds;
            changed |= !self.lines.is_empty();
            self.lines.clear();
            self.finding = None;
        }
        let before = self.lines.len();
        self.lines.retain(|pair, (course, _)| {
            wanted.get(pair) == Some(course)
        });
        changed |= self.lines.len() != before;
        if self.finding.as_ref().is_some_and(|(pair, course, _)| {
            wanted.get(pair) != Some(course)
        }) {
            self.finding = None;
        }
        while left() {
            let Some((pair, course, search)) = &mut self.finding
            else {
                let mut missing = wanted
                    .iter()
                    .filter(|(pair, _)| {
                        !self.lines.contains_key(pair)
                    })
                    .collect::<Vec<_>>();
                missing.sort_by_key(|(pair, _)| **pair);
                let Some((pair, course)) = missing.first() else {
                    break;
                };
                let ((a, b), ..) = **course;
                self.finding = Some((
                    **pair,
                    **course,
                    land.search(a.site, b.site),
                ));
                continue;
            };
            let Some(corridor) = search.step(land, CROSSINGS_A_STEP)
            else {
                continue;
            };
            let ((a, b), ..) = *course;
            let way = Route::way(land, a, b, &corridor);
            let line = Line::spline(&way, 0.5)
                .pts
                .into_iter()
                .map(|p| p.xz())
                .collect();
            self.lines.insert(*pair, (*course, line));
            self.finding = None;
            changed = true;
        }
        if !changed {
            return Vec::new();
        }
        let mut cells = HashSet::new();
        for (_, line) in self.lines.values() {
            for p in line.iter().step_by(2) {
                for off in [
                    Vec2::ZERO,
                    Vec2::X * 2.0,
                    -Vec2::X * 2.0,
                    Vec2::Y * 2.0,
                    -Vec2::Y * 2.0,
                ] {
                    cells.insert(Self::cell(*p + off));
                }
            }
        }
        let moved = cells
            .symmetric_difference(&self.cells)
            .map(|c| (Vec2::new(c.0 as f32, c.1 as f32) + 0.5) * 2.0)
            .collect();
        self.cells = cells;
        moved
    }
}

/// The roads between towns, each with what its line is found for.
fn courses(land: &Land, society: &Society) -> HashMap<Pair, Course> {
    let end = |agent| {
        let agent = society.agent(agent)?;
        Some(End {
            site: society.town(agent.town)?.site,
            plot: agent.plot,
        })
    };
    society
        .roads
        .iter()
        .filter_map(|road| {
            let (a, b) = (end(road.pair.0)?, end(road.pair.1)?);
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            let at = (land.sites[a.site].at, land.sites[b.site].at);
            (a.site != b.site && at.0.distance(at.1) <= LONGEST)
                .then_some((road.pair, ((a, b), at.0, at.1)))
        })
        .collect()
}
