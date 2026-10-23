//! What a house is built from, and its meshes as the engine takes
//! them, handed over across frames when one frame has no time left.

use glam::Vec3;
use hamlet_engine_bevy::prelude::Handle;
use hamlet_engine_bevy::scene::{Mesh, MeshData};

use crate::build::mesh::Mb;
use crate::build::upload::{Draw, Unsent};
use crate::places::house::{self, Remains, TownStyle};
use crate::places::town::flag::FlagSettings;
use crate::places::yard::Yard;
use crate::society::AgentId;
use crate::world::land::Land;

/// The makings of a house's mesh, besides its agent.
#[derive(Clone, Copy)]
pub struct Recipe {
    pub agent: AgentId,
    pub site: usize,
    pub hall: bool,
    pub leader: bool,
    /// Its remains, if its agent is archived.
    pub remains: Option<Remains>,
    pub wings: u8,
    pub yard: Yard,
}

/// A house's meshes as they are handed over: the body, for a house
/// that is lived in its window panes and where its door lantern
/// hangs, and for a leader's the cloth of its banner and where it
/// hangs.
pub struct Built {
    pub body: Handed,
    pub fittings: Option<(Handed, Vec3)>,
    pub banner: Option<(Handed, Vec3)>,
}

/// A mesh as it is handed over.
pub enum Handed {
    Waiting(MeshData),
    There(Handle<Mesh>),
    /// The engine refused it: nothing is drawn.
    Refused,
}

impl Handed {
    /// Hands the mesh over if it waits and the frame has time. True
    /// once it is there or refused.
    fn hand(&mut self, draw: &mut Draw) -> bool {
        let Self::Waiting(_) = self else {
            return true;
        };
        let Self::Waiting(data) =
            std::mem::replace(self, Self::Refused)
        else {
            return true;
        };
        match draw.mesh(data) {
            Ok(mesh) => {
                *self = Self::There(mesh);
                true
            }
            Err(Unsent::Full(data)) => {
                *self = Self::Waiting(data);
                false
            }
            Err(Unsent::Refused(why)) => {
                hamlet_engine_bevy::warn(&format!("a house: {why}"));
                true
            }
        }
    }

    /// The mesh, once it is there.
    pub fn mesh(&self) -> Option<&Handle<Mesh>> {
        match self {
            Self::There(mesh) => Some(mesh),
            _ => None,
        }
    }
}

impl Built {
    /// The meshes of the house `recipe` makes.
    pub fn of(
        land: &Land,
        flag: &FlagSettings,
        recipe: Recipe,
    ) -> Self {
        if let Some(remains) = recipe.remains {
            let rest = match remains {
                Remains::Ruin => {
                    house::ruin(land, recipe.agent, recipe.hall)
                }
                Remains::Bare => Mb::new(),
            };
            return Self {
                body: Handed::Waiting(rest.data()),
                fittings: None,
                banner: None,
            };
        }
        let built = house::house(
            land,
            recipe.agent,
            &TownStyle::of(land, recipe.site),
            recipe.hall,
            recipe.leader,
            recipe.wings,
            &recipe.yard,
        );
        let banner = built.banner.map(|(at, colours)| {
            let cloth = flag.cloth(
                house::BANNER.x,
                house::BANNER.y,
                &colours,
            );
            (Handed::Waiting(cloth.data()), at)
        });
        Self {
            body: Handed::Waiting(built.body.data()),
            fittings: Some((
                Handed::Waiting(built.panes.data()),
                built.lamp,
            )),
            banner,
        }
    }

    /// Hands over what waits, while the frame has time. True once all
    /// of it is there or refused.
    pub fn hand(&mut self, draw: &mut Draw) -> bool {
        self.body.hand(draw)
            && self
                .fittings
                .as_mut()
                .is_none_or(|(panes, _)| panes.hand(draw))
            && self
                .banner
                .as_mut()
                .is_none_or(|(cloth, _)| cloth.hand(draw))
    }
}
