//! What a house is built from, and its meshes as the engine takes
//! them, handed over across frames when one call has no room left.

use std::f32::consts::TAU;

use glam::{Affine3A, Vec3};
use hamlet_engine_sdk::scene::{Mesh, MeshData};

use crate::build::mesh::Mb;
use crate::build::upload::{Unsent, Upload};
use crate::places::house::{self, Remains, TownStyle};
use crate::places::yard::Yard;
use crate::society::AgentId;
use crate::world::land::Land;

const VFX: &str = include_str!("../../../../../assets/vfx.toml");

/// The leader's banner: a flag of stripes of its town's colours.
#[derive(Clone, Copy, PartialEq, Debug, serde::Deserialize)]
pub struct FlagSettings {
    /// Its waves along it, and how far they swing; the cloth stands
    /// still in the realm, caught at one moment.
    pub swing: f32,
    pub waves: f32,
    /// Stripes of the cloth, and how much of the last is cut back.
    pub stripes: usize,
    pub taper: f32,
}

/// The sections of `assets/vfx.toml` a town reads.
#[derive(serde::Deserialize)]
struct VfxTuning {
    flag: FlagSettings,
}

impl FlagSettings {
    /// The banner's settings as `assets/vfx.toml` has them.
    pub fn read() -> Result<Self, String> {
        toml::from_str::<VfxTuning>(VFX)
            .map(|vfx| vfx.flag)
            .map_err(|why| format!("assets/vfx.toml: {why}"))
    }

    /// Adds the cloth of a banner `long` by `tall` to `mb`, hung by
    /// its top corner at `at` and lying along x, in stripes of
    /// `colours` in turn, seen from both sides.
    pub fn cloth(
        &self,
        mb: &mut Mb,
        at: Vec3,
        long: f32,
        tall: f32,
        colours: &[Vec3; 2],
    ) {
        // Columns of points to a stripe, so that a stripe bends.
        const FINE: usize = 4;
        let before = mb.xf;
        mb.at(before * Affine3A::from_translation(at));
        let stripes = self.stripes.max(1);
        let ripple = |u: f32| {
            Vec3::Z * self.swing * u * (u * self.waves * TAU).sin()
        };
        for face in [1.0f32, -1.0] {
            for stripe in 0..stripes {
                let colour = colours[stripe % 2];
                let mut column = Vec::new();
                for step in 0..=FINE {
                    let u = (stripe as f32
                        + step as f32 / FINE as f32)
                        / stripes as f32;
                    // Only the last stripe is cut back.
                    let cut = self.taper
                        * 0.5
                        * ((u * stripes as f32)
                            - (stripes - 1) as f32)
                            .max(0.0);
                    let pair = [cut, 1.0 - cut].map(|v| {
                        let p = Vec3::new(u * long, -v * tall, 0.0)
                            + ripple(u);
                        mb.v(p, Vec3::Z * face, colour)
                    });
                    column.push(pair);
                }
                for pair in column.windows(2) {
                    mb.quad(
                        pair[0][0], pair[0][1], pair[1][1],
                        pair[1][0],
                    );
                }
            }
        }
        mb.at(before);
    }
}

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

/// A house's meshes as they are handed over: the body, and for a
/// house that is lived in its window panes and where its door
/// lantern hangs.
pub struct Built {
    pub body: Handed,
    pub fittings: Option<(Handed, Vec3)>,
}

/// A mesh as it is handed over.
pub enum Handed {
    Waiting(MeshData),
    There(Mesh),
    /// The engine refused it: nothing is drawn.
    Refused,
}

impl Handed {
    /// Hands the mesh over if it waits, within `upload`. True once
    /// it is there or refused.
    fn hand(&mut self, upload: &mut Upload) -> bool {
        let Self::Waiting(_) = self else {
            return true;
        };
        let Self::Waiting(data) =
            std::mem::replace(self, Self::Refused)
        else {
            return true;
        };
        match upload.mesh(data) {
            Ok(mesh) => {
                *self = Self::There(mesh);
                true
            }
            Err(Unsent::Full(data)) => {
                *self = Self::Waiting(data);
                false
            }
            Err(Unsent::Refused(why)) => {
                hamlet_engine_sdk::warn(&format!("a house: {why}"));
                true
            }
        }
    }

    /// The mesh, once it is there.
    pub fn mesh(&self) -> Option<&Mesh> {
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
        let mut body = built.body;
        if let Some((at, colours)) = built.banner {
            flag.cloth(
                &mut body,
                at,
                house::BANNER.x,
                house::BANNER.y,
                &colours,
            );
        }
        Self {
            body: Handed::Waiting(body.data()),
            fittings: Some((
                Handed::Waiting(built.panes.data()),
                built.lamp,
            )),
        }
    }

    /// Hands over what waits, within `upload`. True once all of it
    /// is there or refused.
    pub fn hand(&mut self, upload: &mut Upload) -> bool {
        self.body.hand(upload)
            && self
                .fittings
                .as_mut()
                .is_none_or(|(panes, _)| panes.hand(upload))
    }
}
