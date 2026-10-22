//! A road built a step at a time: its way between the towns, the
//! ground along its line, the rows of its surface, its meshes, and
//! their pieces handed to the engine.

use glam::Vec3Swizzles;
use hamlet_engine_bevy::prelude::Handle;
use hamlet_engine_bevy::scene::{Mesh, MeshData};

use crate::build::mesh::Line;
use crate::build::upload::{Draw, Unsent};
use crate::places::road::mesh::{self, Rows, Span, Stretch};
use crate::realm::Ctx;
use crate::society::RoadLevel;
use crate::world::land::Search;
use crate::world::layout::{End, Route};

/// Crossings of the corridors looked at in one step.
const CROSSINGS_A_STEP: usize = 64;

/// Points of a road's line whose ground is looked up in one step.
const GROUND_A_STEP: usize = 128;

/// Rows of a road's surface worked out in one step.
const ROWS_A_STEP: usize = 48;

/// Rows of a road's surface and groove made into meshes in one step.
const MESH_ROWS_A_STEP: usize = 256;

/// The farthest apart the towns of a road's two ends may lie for the
/// road to be laid, so that no road's work is without end.
const LONGEST: f32 = 3_000.0;

/// The most bytes of one piece of a road's mesh, so that a long road
/// is handed over in several calls.
const PIECE: usize = 256 * 1024;

/// A road being built.
pub(super) struct Build {
    ends: (End, End),
    level: RoadLevel,
    /// The road's own number, which settles its small differences.
    salt: u64,
    /// The bridges of other roads its own may meet.
    met: Vec<Span>,
    stage: Option<Stage>,
}

enum Stage {
    /// Its way is yet to be found.
    Way,
    /// The corridor between its two towns, being searched for.
    Search(Search),
    /// The line of its way, and the ground's height at its points as
    /// far as it is looked up.
    Ground { line: Line, ground: Vec<f32> },
    /// The rows of its surface, being worked out.
    Rows { route: Route, rows: Rows },
    /// Its meshes, being made a stretch at a time, and the stretches
    /// left.
    Meshes {
        route: Route,
        rows: Rows,
        stretches: Vec<Stretch>,
        surface: Vec<MeshData>,
        groove: Vec<MeshData>,
    },
    /// Its meshes, being handed over piece by piece.
    Handing {
        route: Route,
        span: Option<Span>,
        bridge: bool,
        surface: (Vec<MeshData>, Vec<Handle<Mesh>>),
        groove: (Vec<MeshData>, Vec<Handle<Mesh>>),
    },
}

/// A road whose meshes the engine holds.
pub(super) struct Built {
    pub route: Route,
    pub surface: Vec<Handle<Mesh>>,
    pub groove: Vec<Handle<Mesh>>,
    /// Its bridges, for the roads that meet them.
    pub span: Option<Span>,
    /// True when any of it stands high enough to throw a shadow.
    pub bridge: bool,
}

impl Build {
    pub(super) fn new(
        ends: (End, End),
        level: RoadLevel,
        salt: u64,
        met: Vec<Span>,
    ) -> Self {
        // A road's line is the same whichever way its ends are given.
        let ends = if ends.0 <= ends.1 {
            ends
        } else {
            (ends.1, ends.0)
        };
        Self {
            ends,
            level,
            salt,
            met,
            stage: Some(Stage::Way),
        }
    }

    /// Takes the road a step further: the road once it is built.
    pub(super) fn step(
        &mut self,
        ctx: &Ctx,
        draw: &mut Draw,
    ) -> Result<Option<Built>, String> {
        let land = ctx.land;
        let (a, b) = self.ends;
        let along = |way: Vec<_>| Stage::Ground {
            line: Line::spline(&way, 0.5),
            ground: Vec::new(),
        };
        let stage = match self.stage.take() {
            Some(Stage::Way) if a.site == b.site => {
                along(Route::way(land, a, b, &[]))
            }
            Some(Stage::Way) => {
                let apart = land.sites[a.site]
                    .at
                    .distance(land.sites[b.site].at);
                if apart > LONGEST {
                    return Err(format!(
                        "its towns lie {apart:.0} apart, more than \
                         {LONGEST:.0}"
                    ));
                }
                Stage::Search(land.search(a.site, b.site))
            }
            Some(Stage::Search(mut search)) => {
                match search.step(land, CROSSINGS_A_STEP) {
                    Some(corridor) => {
                        along(Route::way(land, a, b, &corridor))
                    }
                    None => Stage::Search(search),
                }
            }
            Some(Stage::Ground { line, mut ground }) => {
                let from = ground.len();
                let to = (from + GROUND_A_STEP).min(line.pts.len());
                ground.extend(
                    line.pts[from..to]
                        .iter()
                        .map(|p| land.surface(p.xz())),
                );
                if ground.len() < line.pts.len() {
                    Stage::Ground { line, ground }
                } else {
                    let route = Route::over(
                        land,
                        line,
                        ground,
                        [a.site, b.site],
                    );
                    let rows = Rows::new(land, &route, self.level);
                    Stage::Rows { route, rows }
                }
            }
            Some(Stage::Rows { route, mut rows }) => {
                if rows.fill(land, &route, ROWS_A_STEP) {
                    Stage::Meshes {
                        stretches: rows.stretches(MESH_ROWS_A_STEP),
                        route,
                        rows,
                        surface: Vec::new(),
                        groove: Vec::new(),
                    }
                } else {
                    Stage::Rows { route, rows }
                }
            }
            Some(Stage::Meshes {
                route,
                rows,
                mut stretches,
                mut surface,
                mut groove,
            }) => {
                if let Some(stretch) = stretches.pop() {
                    let (level, salt) = (self.level, self.salt);
                    let made = [
                        (
                            &mut surface,
                            mesh::surface(
                                land, &route, &rows, level, salt,
                                &self.met, stretch,
                            ),
                        ),
                        (
                            &mut groove,
                            mesh::groove(
                                land, &route, &rows, level, salt,
                                stretch,
                            ),
                        ),
                    ];
                    for (pieces, mb) in made {
                        if !mb.is_empty() {
                            pieces.extend(mb.data().pieces(PIECE));
                        }
                    }
                    Stage::Meshes {
                        route,
                        rows,
                        stretches,
                        surface,
                        groove,
                    }
                } else {
                    let bridge = route
                        .raised
                        .iter()
                        .any(|&h| h > land.set.road.bridge_from);
                    Stage::Handing {
                        span: Span::of(land, &route),
                        route,
                        bridge,
                        surface: (surface, Vec::new()),
                        groove: (groove, Vec::new()),
                    }
                }
            }
            Some(Stage::Handing {
                route,
                span,
                bridge,
                mut surface,
                mut groove,
            }) => {
                let waiting = if surface.0.is_empty() {
                    &mut groove
                } else {
                    &mut surface
                };
                if let Some(data) = waiting.0.pop() {
                    match draw.mesh(data) {
                        Ok(made) => waiting.1.push(made),
                        Err(Unsent::Full(data)) => {
                            waiting.0.push(data)
                        }
                        Err(Unsent::Refused(why)) => return Err(why),
                    }
                }
                if surface.0.is_empty() && groove.0.is_empty() {
                    return Ok(Some(Built {
                        route,
                        surface: surface.1,
                        groove: groove.1,
                        span,
                        bridge,
                    }));
                }
                Stage::Handing {
                    route,
                    span,
                    bridge,
                    surface,
                    groove,
                }
            }
            None => return Err("it was left half built".into()),
        };
        self.stage = Some(stage);
        Ok(None)
    }
}

/// The road's methods on [`MeshData`].
trait PiecesExt {
    /// The mesh cut into pieces of at most `most` bytes, whole
    /// triangles each, or kept whole if it fits.
    fn pieces(self, most: usize) -> Vec<MeshData>;
}

impl PiecesExt for MeshData {
    fn pieces(self, most: usize) -> Vec<MeshData> {
        let lists = |data: &MeshData| {
            [&data.positions, &data.normals, &data.uvs, &data.colors]
                .map(|list| list.len())
        };
        let bytes = 4
            * (lists(&self).iter().sum::<usize>()
                + self.indices.len());
        if bytes <= most {
            return vec![self];
        }
        let count = self.positions.len() / 3;
        let widths = lists(&self).map(|len| len / count.max(1));
        // A vertex's bytes, and three of an index.
        let vertex = 4 * widths.iter().sum::<usize>();
        let mut out = Vec::new();
        let mut at = vec![u32::MAX; count];
        let mut piece = Piece::default();
        for triangle in self.indices.as_chunks::<3>().0 {
            if piece.bytes(vertex) + 3 * (vertex + 4) > most {
                piece.finish(&mut at, &mut out);
            }
            for &index in triangle {
                let index = index as usize;
                if at[index] == u32::MAX {
                    at[index] = piece.used.len() as u32;
                    piece.used.push(index);
                }
                piece.indices.push(at[index]);
            }
        }
        piece.finish(&mut at, &mut out);
        let from =
            [&self.positions, &self.normals, &self.uvs, &self.colors];
        out.into_iter()
            .map(|(used, indices)| {
                let [positions, normals, uvs, colors] =
                    from.map(|list| {
                        let width = list.len() / count.max(1);
                        used.iter()
                            .flat_map(|&v| {
                                list[v * width..(v + 1) * width]
                                    .iter()
                                    .copied()
                            })
                            .collect()
                    });
                MeshData {
                    positions,
                    normals,
                    uvs,
                    colors,
                    indices,
                }
            })
            .collect()
    }
}

/// A piece being cut: the vertices of the whole it uses, in its own
/// order, and its triangles over them.
#[derive(Default)]
struct Piece {
    used: Vec<usize>,
    indices: Vec<u32>,
}

impl Piece {
    fn bytes(&self, vertex: usize) -> usize {
        self.used.len() * vertex + self.indices.len() * 4
    }

    /// Ends the piece, if it has anything, and begins the next.
    fn finish(
        &mut self,
        at: &mut [u32],
        out: &mut Vec<(Vec<usize>, Vec<u32>)>,
    ) {
        for &v in &self.used {
            at[v] = u32::MAX;
        }
        let piece = std::mem::take(self);
        if !piece.indices.is_empty() {
            out.push((piece.used, piece.indices));
        }
    }
}
