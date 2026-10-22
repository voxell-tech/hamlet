//! What agents and hamlets wear (docs/looks.md): the files worn, as
//! the closet reads them from the store, read for what wears them,
//! and the looks of each agent and town drawn.

pub mod closet;
pub mod glb;

use std::collections::{BTreeMap, HashMap};

use looks::wardrobe::{Clip, Emote, Skin, Target};
use looks::{Outfit, Worn};

use crate::bridge::Bridge;
use crate::society::{AgentId, Society, TownId};
use crate::wardrobe::closet::Delivery;
use crate::wardrobe::glb::Glb;

/// What an agent's robot wears: its skin with the file it is read
/// from, the file of its mesh and its clip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentLook {
    pub skin: Option<(String, Skin)>,
    pub mesh: Option<String>,
    pub clip: Option<Clip>,
}

/// What a hamlet's houses wear: its skin, the file of its mesh and
/// its emote.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HamletLook {
    pub skin: Option<Skin>,
    pub mesh: Option<String>,
    pub emote: Option<Emote>,
}

/// The looks worn, read.
#[derive(Default)]
pub struct Wardrobe {
    /// The bytes of each file worn, and each mesh read.
    files: BTreeMap<String, Vec<u8>>,
    meshes: HashMap<String, Glb>,
    /// The looks by the agent's name and the hamlet's name on disk.
    by_agent: BTreeMap<String, AgentLook>,
    by_hamlet: BTreeMap<String, HamletLook>,
    /// The looks of the society's agents and towns.
    agents: HashMap<AgentId, AgentLook>,
    towns: HashMap<TownId, HamletLook>,
    /// One more each time the looks of an agent or a town change.
    pub version: u64,
}

impl Wardrobe {
    /// Wears what `delivery` says, the files it brings added to
    /// those kept and those no longer worn let go.
    pub fn wear(&mut self, delivery: Delivery) {
        let Delivery { worn, files } = delivery;
        self.files.extend(files);
        let named = worn.files().collect::<Vec<_>>();
        self.files.retain(|file, _| named.contains(&file.as_str()));
        self.meshes.retain(|file, _| named.contains(&file.as_str()));
        for outfit in
            worn.agents.values().chain(worn.hamlets.values())
        {
            if let Some(file) = &outfit.mesh {
                self.mesh(file);
            }
        }
        self.by_agent = worn
            .agents
            .iter()
            .map(|(name, outfit)| (name.clone(), self.agent(outfit)))
            .collect();
        self.by_hamlet = worn
            .hamlets
            .iter()
            .map(|(name, outfit)| (name.clone(), self.hamlet(outfit)))
            .collect();
        self.tell(&worn);
    }

    /// Finds the looks of each agent and town of `society` by their
    /// names in `bridge`.
    pub fn resolve(&mut self, bridge: &Bridge, society: &Society) {
        let agents = society
            .agents
            .iter()
            .filter_map(|agent| {
                let (name, _) = bridge.named(agent.id)?;
                Some((agent.id, self.by_agent.get(name)?.clone()))
            })
            .collect::<HashMap<_, _>>();
        let towns = society
            .towns
            .iter()
            .filter_map(|town| {
                let name = bridge.hamlet(town.id)?;
                Some((town.id, self.by_hamlet.get(name)?.clone()))
            })
            .collect::<HashMap<_, _>>();
        if agents != self.agents || towns != self.towns {
            self.agents = agents;
            self.towns = towns;
            self.version += 1;
        }
    }

    /// What the robot of `agent` wears.
    pub fn of_agent(&self, agent: AgentId) -> Option<&AgentLook> {
        self.agents.get(&agent)
    }

    /// What every agent's robot wears.
    pub fn agent_looks(&self) -> impl Iterator<Item = &AgentLook> {
        self.agents.values()
    }

    /// What the houses of `town` wear.
    pub fn of_town(&self, town: TownId) -> Option<&HamletLook> {
        self.towns.get(&town)
    }

    /// The mesh read from `file`.
    pub fn glb(&self, file: &str) -> Option<&Glb> {
        self.meshes.get(file)
    }

    /// Reads the mesh of `file`, once.
    fn mesh(&mut self, file: &str) {
        if self.meshes.contains_key(file) {
            return;
        }
        let Some(bytes) = self.files.get(file) else {
            return;
        };
        match Glb::read(bytes) {
            Ok(glb) => {
                self.meshes.insert(file.to_owned(), glb);
            }
            Err(why) => warn(file, &why),
        }
    }

    fn agent(&self, outfit: &Outfit) -> AgentLook {
        let target = Target::Agent;
        AgentLook {
            skin: outfit.skin.as_ref().and_then(|file| {
                let skin =
                    self.read(file, |b| Skin::read(b, target))?;
                Some((file.clone(), skin))
            }),
            mesh: self.worn_mesh(&outfit.mesh),
            clip: outfit.emote.as_ref().and_then(|file| {
                match self.read(file, |b| Emote::read(b, target))? {
                    Emote::Clip(clip) => Some(clip),
                    _ => None,
                }
            }),
        }
    }

    fn hamlet(&self, outfit: &Outfit) -> HamletLook {
        let target = Target::Hamlet;
        HamletLook {
            skin: outfit.skin.as_ref().and_then(|file| {
                self.read(file, |b| Skin::read(b, target))
            }),
            mesh: self.worn_mesh(&outfit.mesh),
            emote: outfit.emote.as_ref().and_then(|file| {
                self.read(file, |b| Emote::read(b, target))
            }),
        }
    }

    /// The mesh `file`, when it was read.
    fn worn_mesh(&self, file: &Option<String>) -> Option<String> {
        file.clone().filter(|file| self.meshes.contains_key(file))
    }

    /// The file `file` read by `read`; none, told in the log, when
    /// it is not there or cannot be read.
    fn read<T>(
        &self,
        file: &str,
        read: impl Fn(&[u8]) -> anyhow::Result<T>,
    ) -> Option<T> {
        let bytes = self.files.get(file)?;
        read(bytes)
            .map_err(|why| warn(file, &format!("{why:#}")))
            .ok()
    }

    /// Tells the log how many are dressed.
    fn tell(&self, worn: &Worn) {
        hamlet_engine_sdk::info(&format!(
            "looks: {} agents and {} hamlets wear something, of {} \
             files",
            worn.agents.len(),
            worn.hamlets.len(),
            self.files.len()
        ));
    }
}

fn warn(file: &str, why: &str) {
    hamlet_engine_sdk::warn(&format!(
        "looks: {file} is not worn: {why}"
    ));
}
