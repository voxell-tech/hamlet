//! A road built a step at a time: its route, the rows of its surface,
//! its meshes, and their pieces handed to the engine.

use hamlet_engine_sdk::scene::{Mesh, MeshData};

use super::mesh::{self, Rows, Span};
use crate::layout::{End, Route};
use crate::realm::Ctx;
use crate::society::RoadLevel;
use crate::upload::{Unsent, Upload};

/// Rows of a road's surface worked out in one step.
const ROWS_A_STEP: usize = 48;

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
    /// Its route is yet to be found.
    Route,
    /// The rows of its surface, being worked out.
    Rows { route: Route, rows: Rows },
    /// Its rows worked out, its meshes yet to be made.
    Meshes { route: Route, rows: Rows },
    /// Its meshes, being handed over piece by piece.
    Handing {
        route: Route,
        span: Option<Span>,
        bridge: bool,
        surface: (Vec<MeshData>, Vec<Mesh>),
        groove: (Vec<MeshData>, Vec<Mesh>),
    },
}

/// A road whose meshes the engine holds.
pub(super) struct Built {
    pub route: Route,
    pub surface: Vec<Mesh>,
    pub groove: Vec<Mesh>,
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
        Self {
            ends,
            level,
            salt,
            met,
            stage: Some(Stage::Route),
        }
    }

    /// Takes the road a step further: the road once it is built.
    pub(super) fn step(
        &mut self,
        ctx: &Ctx,
        upload: &mut Upload,
    ) -> Result<Option<Built>, String> {
        let land = ctx.land;
        let stage = match self.stage.take() {
            Some(Stage::Route) => {
                let route =
                    Route::between(land, self.ends.0, self.ends.1);
                let rows = Rows::new(land, &route, self.level);
                Stage::Rows { route, rows }
            }
            Some(Stage::Rows { route, mut rows }) => {
                let done = rows.fill(land, &route, ROWS_A_STEP);
                if done {
                    Stage::Meshes { route, rows }
                } else {
                    Stage::Rows { route, rows }
                }
            }
            Some(Stage::Meshes { route, rows }) => {
                let surface = mesh::surface(
                    land, &route, &rows, self.level, self.salt,
                    &self.met,
                );
                let groove = mesh::groove(
                    land, &route, &rows, self.level, self.salt,
                );
                let bridge = route
                    .raised
                    .iter()
                    .any(|&h| h > land.set.road.bridge_from);
                Stage::Handing {
                    span: Span::of(land, &route),
                    route,
                    bridge,
                    surface: (
                        surface.data().pieces(PIECE),
                        Vec::new(),
                    ),
                    groove: (groove.data().pieces(PIECE), Vec::new()),
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
                    match upload.mesh(data) {
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
