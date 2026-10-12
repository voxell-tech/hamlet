//! The roads: one entity each, laid again only when its level, its
//! ends or the ground it crosses change.

use std::collections::HashSet;

use bevy::prelude::*;

use super::{Pass, RoadDrawn};
use crate::generator::building::Course;
use crate::generator::layout::End;
use crate::generator::noise;
use crate::generator::society::{AgentId, Society};

fn end_of(society: &Society, id: AgentId) -> Option<End> {
    let agent = society.agent(id)?;
    Some(End {
        site: society.town(agent.town)?.site,
        plot: agent.plot,
    })
}

impl Pass<'_, '_, '_> {
    /// Takes out the roads the society no longer holds and has
    /// built those that are new or changed. A road stays as it is
    /// until its new mesh is ready.
    pub(super) fn roads(&mut self) {
        let Self {
            commands,
            society,
            land,
            drawn,
            builds,
            ..
        } = self;
        let live = society
            .roads
            .iter()
            .map(|r| r.pair)
            .collect::<HashSet<_>>();
        builds.keep_roads(|pair| live.contains(pair));
        drawn.roads.retain(|pair, road| {
            let stays = live.contains(pair);
            if !stays {
                commands.entity(road.entity).try_despawn();
            }
            stays
        });
        // The old or new place of a town: a road between towns that
        // passes near may have to find another way, or may have a
        // shorter one.
        let sites =
            land.sites.iter().map(|s| s.at).collect::<Vec<_>>();
        let mut shifted = (0..sites.len().max(drawn.sites.len()))
            .filter(|&i| sites.get(i) != drawn.sites.get(i))
            .flat_map(|i| [sites.get(i), drawn.sites.get(i)])
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        drawn.sites = sites;
        // So may one that passes ground that came to be held, as a
        // chief's is, or that was let go.
        let holds = land.held();
        if holds != drawn.holds {
            let changed = holds
                .iter()
                .filter(|hold| !drawn.holds.contains(hold))
                .chain(
                    drawn
                        .holds
                        .iter()
                        .filter(|hold| !holds.contains(hold)),
                )
                .map(|hold| hold.0)
                .collect::<Vec<_>>();
            shifted.extend(changed);
            drawn.holds = holds;
        }
        let near = land.set.town.radius + land.set.land.site_spacing;
        let crossed = |road: &RoadDrawn| {
            road.course.between()
                && road.route.line.pts.iter().step_by(8).any(|p| {
                    shifted
                        .iter()
                        .any(|at| at.distance(p.xz()) < near)
                })
        };
        let reach = land.set.road.bridge_half * 2.0;
        let mut spans = drawn
            .roads
            .iter()
            .filter_map(|(pair, d)| Some((*pair, d.span.as_ref()?)))
            .collect::<Vec<_>>();
        spans.sort_by_key(|(pair, _)| *pair);
        for r in &society.roads {
            let (Some(a), Some(b)) = (
                end_of(society, r.pair.0),
                end_of(society, r.pair.1),
            ) else {
                continue;
            };
            let mut course = Course::of(land, (a, b));
            // Two bridges that cross are one deck, so a road is laid
            // again when the bridges its own meet are others.
            let own = drawn
                .roads
                .get(&r.pair)
                .and_then(|d| d.span.as_ref());
            let met = || {
                spans.iter().filter(|(pair, span)| {
                    *pair != r.pair
                        && own
                            .is_none_or(|own| own.nears(span, reach))
                })
            };
            if own.is_some() {
                let marks = met()
                    .flat_map(|(pair, span)| {
                        [
                            u64::from(pair.0.0),
                            u64::from(pair.1.0),
                            span.mark(),
                        ]
                    })
                    .collect::<Vec<_>>();
                if !marks.is_empty() {
                    course.meets = noise::key(&marks);
                }
            }
            let same = match builds.road(r.pair) {
                Some(job) => {
                    job.level == r.level
                        && job.course == course
                        && (shifted.is_empty() || !course.between())
                }
                None => drawn.roads.get(&r.pair).is_some_and(|d| {
                    d.level == r.level
                        && d.course == course
                        && !crossed(d)
                }),
            };
            if !same {
                let met =
                    met().map(|(_, span)| (*span).clone()).collect();
                builds.lay_road(land, r.pair, r.level, course, met);
            }
        }
    }
}
