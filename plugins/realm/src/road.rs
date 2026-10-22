//! The roads between agents: one each, laid again only when its
//! level, its ends or the ground it crosses change. A road is built a
//! step at a time over the frames, and stays as it was until its new
//! meshes are all handed over.

mod build;
mod groove;
mod mesh;

use std::collections::{HashMap, HashSet};

use glam::{Vec2, Vec3Swizzles};
use hamlet_engine_sdk::scene::{Entity, Mesh};

use crate::frame::Frame;
use crate::land::Land;
use crate::layout::{End, Route};
use crate::noise;
use crate::palette::Palette;
use crate::realm::Ctx;
use crate::road::build::{Build, Built};
use crate::road::groove::Grooves;
use crate::road::mesh::Span;
use crate::society::{AgentId, Pair, RoadLevel, Society};
use crate::upload::Upload;

/// Roads and their bridges.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadSettings {
    /// Height above the ground at which a road counts as a bridge.
    pub bridge_from: f32,
    pub bridge_half: f32,
    /// Height of a bridge deck above the water.
    pub deck: f32,
    /// Half the width of the trodden strip at each level of use,
    /// from overgrown to rail.
    pub half_width: [f32; 4],
    /// Half the width of a paved road's flags.
    pub flags_half: f32,
    /// Width of the groove's slate bed.
    pub groove_width: f32,
    /// Ground a robot covers per second on a road.
    pub walk_speed: f32,
    /// Ground a cart covers per second on a road between towns, and
    /// the stretch at either end over which it gets going and
    /// slows.
    pub ride_speed: f32,
    pub ride_ramp: f32,
    /// Height of a cart's bed over the road.
    pub cart_bed: f32,
    /// The longest a cart takes one way, in seconds. It goes faster
    /// on a road too long for its speed.
    pub ride_time: f32,
    /// Most a stretch through the mountains counts longer than its
    /// length when a road between towns picks its way.
    pub climb: f32,
    /// The most crossings looked at for a way between two towns
    /// before the road runs straight.
    pub search: usize,
}

/// The makings of a road's line: its ends, and the places of the
/// towns it runs between.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Course {
    ends: (End, End),
    /// The places of the towns of its two ends.
    at: (Vec2, Vec2),
    /// A number for the bridges of other roads that its own bridges
    /// meet. Zero for none.
    meets: u64,
}

impl Course {
    fn of(land: &Land, ends: (End, End)) -> Self {
        Self {
            ends,
            at: (
                land.sites[ends.0.site].at,
                land.sites[ends.1.site].at,
            ),
            meets: 0,
        }
    }

    /// True when it runs from one town to another.
    fn between(&self) -> bool {
        self.ends.0.site != self.ends.1.site
    }
}

/// A road as it is drawn.
struct Drawn {
    level: RoadLevel,
    course: Course,
    route: Route,
    /// Its bridges, for the roads that meet them.
    span: Option<Span>,
    /// The agent at the route's start.
    first: AgentId,
    /// A number of its own each time a road is laid.
    laid: u64,
    /// The road's entity, under which its surface and its groove
    /// hang, each in pieces.
    entity: Entity,
    groove: Vec<Mesh>,
    /// What is kept only so it is not dropped: the pieces' entities
    /// and the surface's meshes.
    _held: (Vec<Entity>, Vec<Mesh>),
}

/// A road being laid.
struct Job {
    level: RoadLevel,
    course: Course,
    build: Build,
}

impl Society {
    /// The end of a road at the house of `id`.
    fn end_of(&self, id: AgentId) -> Option<End> {
        let agent = self.agent(id)?;
        Some(End {
            site: self.town(agent.town)?.site,
            plot: agent.plot,
        })
    }
}

/// The roads drawn.
pub struct Roads {
    drawn: HashMap<Pair, Drawn>,
    jobs: HashMap<Pair, Job>,
    /// The society's shape and the land's places when the roads were
    /// last looked over. None to look them over again whatever.
    shape: Option<(u64, u64)>,
    /// The place of each site, and the ground held, when the roads
    /// were last looked over.
    sites: Vec<Vec2>,
    holds: Vec<(Vec2, f32)>,
    /// Roads laid so far, which numbers each.
    laid: u64,
    grooves: Grooves,
}

impl Roads {
    /// No roads yet.
    pub fn make(pal: &Palette) -> Result<Self, String> {
        Ok(Self {
            drawn: HashMap::new(),
            jobs: HashMap::new(),
            shape: None,
            sites: Vec::new(),
            holds: Vec::new(),
            laid: 0,
            grooves: Grooves::make(pal)?,
        })
    }

    /// Brings what is drawn in line with the society, within
    /// `upload`.
    pub fn update(&mut self, ctx: &Ctx, upload: &mut Upload) {
        let shape = (ctx.society.shape(), ctx.land.places());
        if self.shape != Some(shape) {
            self.look_over(ctx.society, ctx.land);
            self.shape = Some(shape);
        }
        self.work(ctx, upload);
        self.grooves.light(ctx, &self.drawn);
    }

    /// Takes out the roads the society no longer holds and sets about
    /// laying those that are new or changed.
    fn look_over(&mut self, society: &Society, land: &Land) {
        let live = society
            .roads
            .iter()
            .map(|r| r.pair)
            .collect::<HashSet<_>>();
        self.jobs.retain(|pair, _| live.contains(pair));
        self.drawn.retain(|pair, _| live.contains(pair));
        // The old or new place of a town: a road between towns that
        // passes near may have to find another way, or may have a
        // shorter one.
        let sites =
            land.sites.iter().map(|s| s.at).collect::<Vec<_>>();
        let mut shifted = (0..sites.len().max(self.sites.len()))
            .filter(|&i| sites.get(i) != self.sites.get(i))
            .flat_map(|i| [sites.get(i), self.sites.get(i)])
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        self.sites = sites;
        // So may one that passes ground that came to be held, as a
        // chief's is, or that was let go.
        let holds = land.held();
        if holds != self.holds {
            let changed = holds
                .iter()
                .filter(|hold| !self.holds.contains(hold))
                .chain(
                    self.holds
                        .iter()
                        .filter(|hold| !holds.contains(hold)),
                )
                .map(|hold| hold.0)
                .collect::<Vec<_>>();
            shifted.extend(changed);
            self.holds = holds;
        }
        let near = land.set.town.radius + land.set.land.site_spacing;
        let crossed = |road: &Drawn| {
            road.course.between()
                && road.route.line.pts.iter().step_by(8).any(|p| {
                    shifted
                        .iter()
                        .any(|at| at.distance(p.xz()) < near)
                })
        };
        let reach = land.set.road.bridge_half * 2.0;
        let mut spans = self
            .drawn
            .iter()
            .filter_map(|(pair, d)| Some((*pair, d.span.as_ref()?)))
            .collect::<Vec<_>>();
        spans.sort_by_key(|(pair, _)| *pair);
        let mut lay = Vec::new();
        for r in &society.roads {
            let (Some(a), Some(b)) =
                (society.end_of(r.pair.0), society.end_of(r.pair.1))
            else {
                continue;
            };
            let mut course = Course::of(land, (a, b));
            // Two bridges that cross are one deck, so a road is laid
            // again when the bridges its own meet are others.
            let own =
                self.drawn.get(&r.pair).and_then(|d| d.span.as_ref());
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
            let same = match self.jobs.get(&r.pair) {
                Some(job) => {
                    job.level == r.level
                        && job.course == course
                        && (shifted.is_empty() || !course.between())
                }
                None => self.drawn.get(&r.pair).is_some_and(|d| {
                    d.level == r.level
                        && d.course == course
                        && !crossed(d)
                }),
            };
            if !same {
                let met =
                    met().map(|(_, span)| (*span).clone()).collect();
                lay.push((r.pair, r.level, course, met));
            }
        }
        for (pair, level, course, met) in lay {
            let salt =
                (u64::from(pair.0.0) << 32) | u64::from(pair.1.0);
            self.jobs.insert(
                pair,
                Job {
                    level,
                    course,
                    build: Build::new(course.ends, level, salt, met),
                },
            );
        }
    }

    /// Takes the roads being laid a step further while the call has
    /// time, one road at a time, the roads nearest the camera first.
    fn work(&mut self, ctx: &Ctx, upload: &mut Upload) {
        while upload.left() {
            let eye = ctx.eye.xz();
            let next = self
                .jobs
                .iter()
                .min_by(|(pa, a), (pb, b)| {
                    let far = |job: &Job| {
                        let (p, q) = job.course.at;
                        p.distance(eye).min(q.distance(eye))
                    };
                    far(a).total_cmp(&far(b)).then(pa.cmp(pb))
                })
                .map(|(pair, _)| *pair);
            let Some(pair) = next else {
                return;
            };
            let Some(job) = self.jobs.get_mut(&pair) else {
                return;
            };
            match job.build.step(ctx, upload) {
                Ok(None) => {}
                Ok(Some(built)) => {
                    if let Some(job) = self.jobs.remove(&pair) {
                        self.place(ctx, pair, job, built);
                    }
                }
                Err(why) => {
                    hamlet_engine_sdk::warn(&format!(
                        "a road: {why}"
                    ));
                    self.jobs.remove(&pair);
                }
            }
        }
    }

    /// Puts a road that is built into the world in place of the one
    /// it replaces.
    fn place(
        &mut self,
        ctx: &Ctx,
        pair: Pair,
        job: Job,
        built: Built,
    ) {
        let mut spans = built.span.is_some();
        if let Some(old) = self.drawn.remove(&pair) {
            spans |= old.span.is_some();
        }
        // The roads whose bridges meet its own are looked over again.
        if spans {
            self.shape = None;
        }
        let entity = match Entity::make(None, 0) {
            Ok(entity) => entity,
            Err(why) => {
                hamlet_engine_sdk::warn(&format!("a road: {why}"));
                return;
            }
        };
        entity.set_transform(Frame::IDENTITY.scene());
        let mut parts = Vec::new();
        let mut part = |mesh: &Mesh, material, cast| {
            if let Ok(piece) = Entity::make(Some(&entity), 0) {
                piece.draw(mesh, material);
                piece.set_shadows(cast, true);
                parts.push(piece);
            }
        };
        for mesh in &built.surface {
            // Only a bridge stands high enough to throw a shadow.
            part(mesh, &ctx.stock.matte, built.bridge);
        }
        for mesh in &built.groove {
            part(mesh, self.grooves.unlit(), false);
        }
        self.laid += 1;
        let ends = job.course.ends;
        self.drawn.insert(
            pair,
            Drawn {
                level: job.level,
                course: job.course,
                route: built.route,
                span: built.span,
                first: if ends.0 <= ends.1 { pair.0 } else { pair.1 },
                laid: self.laid,
                entity,
                groove: built.groove,
                _held: (parts, built.surface),
            },
        );
    }
}
