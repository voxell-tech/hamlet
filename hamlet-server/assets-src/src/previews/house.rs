//! A house as the realm builds one for a hamlet (`places/town`): the
//! realm's house wearing a look as the realm dresses one, its window
//! panes and door lantern, and the moving parts of its emote placed
//! as the realm places them.

use bevy::math::Vec3;
use hamlet_engine_sdk::scene::{MeshData, Pbr};
use looks::wardrobe::{Emote, Skin};
use realm::bake::{
    AgentId, Attire, AttireSettings, Built, FlagSettings, Frame, Glb,
    HamletLook, Handed, KState, Kind, Land, Perch, Recipe, Shell,
    Stock, TownPart, WorldTuning, attire, house, lamp_glass,
    lamp_glow, rgba, skinned,
};

use crate::previews::{Drawn, Form, Node};

/// The agent whose house every hamlet's look is shown on.
const AGENT: AgentId = AgentId(2);

/// A kind of moving part of the emote: its mesh, its own look with
/// the colour it glows after dark, or none for the shared matte.
type Part = (Kind, Form, Option<(Pbr, Vec3)>);

/// A house wearing a look.
pub struct House {
    pub meshes: Vec<MeshData>,
    /// Its body, panes and door lantern.
    nodes: Vec<Node>,
    emote: Option<(Emote, Perch, Vec<Part>)>,
    set: AttireSettings,
    /// How strongly candles burn: 0 by day, 1 after dark.
    candle: f32,
}

impl House {
    /// The house wearing `skin`, the `.glb` `mesh` and `emote`, lit
    /// as by day or, with `candle` 1, after dark.
    pub fn new(
        tuning: &WorldTuning,
        land: &Land,
        (skin, mesh, emote): (
            Option<Skin>,
            Option<&Glb>,
            Option<Emote>,
        ),
        candle: f32,
    ) -> Result<Self, String> {
        let set = tuning.attire;
        let look = HamletLook {
            skin,
            mesh: None,
            emote,
        };
        let attire = Attire {
            pal: look
                .skin
                .as_ref()
                .map(|skin| skinned(&land.pal, skin)),
            shell: mesh.and_then(Shell::of),
            look,
        };
        let built = Built::of(
            land,
            &FlagSettings::read()?,
            Recipe {
                agent: AGENT,
                site: 0,
                hall: false,
                leader: false,
                remains: None,
                wings: 0,
                yard: house::yard_of(land, AGENT, false, 0.0, 0),
            },
            Some(&attire),
        );
        let mut house = Self {
            meshes: Vec::new(),
            nodes: Vec::new(),
            emote: None,
            set,
            candle,
        };
        let pal = &land.pal;
        let root = house.node(None, Frame::IDENTITY, None, true);
        let Handed::Waiting(body) = built.body else {
            return Err("the house was not built".into());
        };
        let body = house.keep(body);
        house.nodes[root].drawn = Some(Drawn {
            form: body,
            look: Stock::matte(),
        });
        if let Some((Handed::Waiting(panes), lamp)) = built.fittings {
            let panes = house.keep(panes);
            house.node(
                Some(root),
                Frame::IDENTITY,
                Some(Drawn {
                    form: panes,
                    look: Stock::pane(pal, candle),
                }),
                false,
            );
            let glass = house.keep(
                TownPart::Lamp.build(pal, &land.set.yard).data(),
            );
            house.node(
                Some(root),
                Frame::at(lamp),
                Some(Drawn {
                    form: glass,
                    look: lamp_glass(
                        pal,
                        lamp_glow(KState::Rest, 0.0, candle),
                    ),
                }),
                false,
            );
        }
        if let (Some(emote), Some((chimney, eaves))) =
            (&attire.look.emote, built.perch)
        {
            let perch = Perch {
                frame: Frame::IDENTITY,
                chimney,
                eaves,
            };
            let mut made = Vec::new();
            for (kind, mb) in
                attire::meshes(emote, &set).unwrap_or_default()
            {
                made.push((kind, house.keep(mb.data())));
            }
            let parts = attire::looks(emote, &set)
                .into_iter()
                .filter_map(|(kind, own)| {
                    let form = match kind.mesh() {
                        Some(of) => made
                            .iter()
                            .find(|(made, _)| *made == of)
                            .map(|(_, form)| *form)?,
                        None => Form::Ball,
                    };
                    Some((kind, form, own))
                })
                .collect();
            house.emote = Some((emote.clone(), perch, parts));
        }
        Ok(house)
    }

    fn keep(&mut self, mesh: MeshData) -> Form {
        self.meshes.push(mesh);
        Form::Built(self.meshes.len() - 1)
    }

    fn node(
        &mut self,
        parent: Option<usize>,
        at: Frame,
        drawn: Option<Drawn>,
        shadows: bool,
    ) -> usize {
        self.nodes.push(Node {
            parent,
            at,
            drawn,
            shadows,
        });
        self.nodes.len() - 1
    }

    /// The house at clock time `t`, its emote's parts placed as
    /// `Emotes::stir` places them and lit as `Emotes::light` lights
    /// them.
    pub fn nodes(&self, t: f32) -> Vec<Node> {
        let mut nodes = self.nodes.clone();
        let Some((emote, perch, parts)) = &self.emote else {
            return nodes;
        };
        let strength =
            attire::strength(emote, self.candle, &self.set);
        for (kind, form, own) in parts {
            let look = match own {
                Some((pbr, glows)) if *glows != Vec3::ZERO => Pbr {
                    emissive: rgba(*glows * strength),
                    ..*pbr
                },
                Some((pbr, _)) => *pbr,
                None => Stock::matte(),
            };
            for at in attire::parts(emote, *kind, perch, t, &self.set)
            {
                nodes.push(Node {
                    parent: Some(0),
                    at: perch.frame.then(at),
                    drawn: Some(Drawn { form: *form, look }),
                    shadows: true,
                });
            }
        }
        nodes
    }
}
