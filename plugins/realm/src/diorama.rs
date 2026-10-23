//! The `diorama` service: the town of a hamlet template as the realm
//! would found it, built with the realm's own builders and handed
//! over in parts for the Agent Bazaar to set on a plinth.

mod plinth;
mod town;
// Shared with the bazaar, which reads what is written here.
#[allow(dead_code)]
pub mod wire;

use std::time::Instant;

use hamlet_engine_sdk::scene::MeshData;
use hamlet_engine_sdk::{decode, encode, info};

use crate::diorama::town::Town;
use crate::diorama::wire::{Ask, Part, Piece, Stage};
use crate::tuning::WorldTuning;

const SETTINGS: &str = include_str!("../assets/diorama.toml");

/// The bytes the engine counts for the meshes of one part, so that
/// the bazaar makes a part's meshes well within one call, as the
/// realm holds its own calls to.
const PART_BYTES: usize = 640 * 1024;

/// The most agents a town is built for, its mayor aside.
const MOST_AGENTS: u32 = 48;

/// The plinth and what stands on it.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DioramaSettings {
    /// Grass left about the outermost house.
    pub margin: f32,
    /// The depth of the soil under the grass, of the wooden base
    /// under that, and how far the base stands out.
    pub soil: f32,
    pub base: f32,
    pub lip: f32,
    /// The thickness of the brass ring about the base.
    pub brass: f32,
    /// Rings and segments of the plinth's top.
    pub rings: usize,
    pub segments: usize,
    /// Trees on the grass about the houses.
    pub trees: usize,
    /// The robots' size over the realm's, so that each reads beside
    /// its house when drawn small.
    pub robot_scale: f32,
}

/// The file the settings are in.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DioramaTuning {
    diorama: DioramaSettings,
}

impl DioramaSettings {
    fn read() -> Result<Self, String> {
        toml::from_str::<DioramaTuning>(SETTINGS)
            .map(|tuning| tuning.diorama)
            .map_err(|why| {
                format!("plugins/realm/assets/diorama.toml: {why}")
            })
    }
}

/// The town last asked for, while its parts are fetched.
#[derive(Default)]
pub struct Dioramas {
    kept: Option<Kept>,
}

/// A town's parts, as they are answered.
struct Kept {
    template: String,
    agents: u32,
    parts: Vec<Vec<u8>>,
}

impl Dioramas {
    /// Answers `argument`, an [`Ask`], with the bytes of its
    /// [`Part`]. The town is built when its first part is asked
    /// for, or another town is, and let go once its last part is
    /// answered.
    pub fn serve(
        &mut self,
        argument: &[u8],
    ) -> Result<Vec<u8>, String> {
        let ask =
            decode::<Ask>(argument).map_err(|why| why.to_string())?;
        let kept = self.kept.take().filter(|kept| {
            kept.template == ask.template && kept.agents == ask.agents
        });
        let kept = match kept {
            Some(kept) => kept,
            None => Kept::build(&ask)?,
        };
        let count = kept.parts.len() as u32;
        let Some(part) = kept.parts.get(ask.part as usize).cloned()
        else {
            return Err(format!(
                "the town of {} has {count} parts",
                ask.template
            ));
        };
        if ask.part + 1 < count {
            self.kept = Some(kept);
        }
        Ok(part)
    }
}

impl Kept {
    fn build(ask: &Ask) -> Result<Self, String> {
        if ask.agents > MOST_AGENTS {
            return Err(format!(
                "a town of {} agents is more than {MOST_AGENTS}",
                ask.agents
            ));
        }
        let began = Instant::now();
        let parts = parts(&ask.template, ask.agents)?;
        let bytes = parts.iter().map(Vec::len).sum::<usize>();
        info(&format!(
            "the diorama of {} is built: {} parts, {bytes} bytes, \
             in {} ms",
            ask.template,
            parts.len(),
            began.elapsed().as_millis()
        ));
        Ok(Self {
            template: ask.template.clone(),
            agents: ask.agents,
            parts,
        })
    }
}

/// The parts of the town of `template` with `agents` besides its
/// mayor, each the bytes of a [`Part`].
fn parts(
    template: &str,
    agents: u32,
) -> Result<Vec<Vec<u8>>, String> {
    let (tuning, pal) = WorldTuning::read()?;
    let set = DioramaSettings::read()?;
    let town = Town::build(&tuning, pal, &set, template, agents)?;
    pack(town.stage, town.things)
        .iter()
        .map(|part| encode(part).map_err(|why| why.to_string()))
        .collect()
}

/// Packs `things`, each a mesh for each look, into parts of at most
/// [`PART_BYTES`], a thing whole in one part where it fits in one,
/// its meshes of one look merged with those of the others in it.
/// `stage` goes in the first.
fn pack(stage: Stage, things: Vec<Vec<(u8, MeshData)>>) -> Vec<Part> {
    let mut groups = vec![Vec::<(u8, MeshData)>::new()];
    let mut filled = 0;
    let things = things.into_iter().flat_map(|thing| {
        let bytes =
            thing.iter().map(|(_, data)| data.bytes()).sum::<usize>();
        match bytes > PART_BYTES {
            true => {
                thing.into_iter().map(|mesh| vec![mesh]).collect()
            }
            false => vec![thing],
        }
    });
    for thing in things {
        let bytes =
            thing.iter().map(|(_, data)| data.bytes()).sum::<usize>();
        let last =
            groups.last_mut().expect("there is always a group");
        if filled + bytes > PART_BYTES && !last.is_empty() {
            groups.push(Vec::new());
            filled = 0;
        }
        filled += bytes;
        groups
            .last_mut()
            .expect("there is always a group")
            .extend(thing);
    }
    let count = groups.len() as u32;
    let mut stage = Some(stage);
    groups
        .into_iter()
        .enumerate()
        .map(|(at, group)| {
            let mut looks = group
                .iter()
                .map(|(look, _)| *look)
                .collect::<Vec<_>>();
            looks.sort_unstable();
            looks.dedup();
            let pieces = looks
                .into_iter()
                .map(|look| {
                    let data = merged(
                        group
                            .iter()
                            .filter(|(of, _)| *of == look)
                            .map(|(_, data)| data),
                    );
                    Piece::pack(
                        look,
                        data.positions,
                        &data.normals,
                        &data.colors,
                        data.indices,
                    )
                })
                .collect();
            Part {
                part: at as u32,
                parts: count,
                stage: stage.take(),
                pieces,
            }
        })
        .collect()
}

/// The diorama's methods on [`MeshData`].
trait BytesExt {
    /// The bytes the engine counts for it, without UVs.
    fn bytes(&self) -> usize;
}

impl BytesExt for MeshData {
    fn bytes(&self) -> usize {
        let vertices = self.positions.len() / 3;
        4 * (vertices * (3 + 3 + 4) + self.indices.len())
    }
}

/// `meshes` as one, without UVs.
fn merged<'a>(
    meshes: impl Iterator<Item = &'a MeshData>,
) -> MeshData {
    let mut whole = MeshData {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
    };
    for data in meshes {
        let first = (whole.positions.len() / 3) as u32;
        whole.positions.extend_from_slice(&data.positions);
        whole.normals.extend_from_slice(&data.normals);
        whole.colors.extend_from_slice(&data.colors);
        whole.indices.extend(data.indices.iter().map(|i| i + first));
    }
    whole
}

#[cfg(test)]
mod tests;
