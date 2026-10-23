//! The light of the grooves: a mote under a robot out with a message,
//! orange at the door of an agent that waits, red where one has
//! faulted. Every other groove shares one material that is not lit.

use std::collections::HashMap;

use glam::Vec3;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    AlphaMode, Material, Shaded, Shader, ShaderKind,
};

use crate::build::frame::Frame;
use crate::build::upload::Draw;
use crate::folk::keeper::KState;
use crate::places::road::Drawn;
use crate::places::road::traffic::Trip;
use crate::realm::Ctx;
use crate::society::Pair;
use crate::world::palette::Palette;
use crate::world::tint::to_linear;

/// The light of a groove that is lit fully, against sunlit white
/// paper at 1.
const LIT: f32 = 4.0;

/// How the strip is lit, as the shader reads it.
const TRAVEL: f32 = 1.0;
const BREATH: f32 = 3.0;
const STUTTER: f32 = 4.0;

/// Height of the light over the groove it lies on.
const OVER: f32 = 0.003;

/// The light's colours, in sRGB: a message under way, an agent that
/// waits, and one that faulted.
const CURRENT: Vec3 = Vec3::new(0.247, 0.847, 0.910);
const SIGNAL: Vec3 = Vec3::new(1.0, 0.541, 0.122);
const FAULT: Vec3 = Vec3::new(1.0, 0.322, 0.278);

/// The light over one road's groove.
struct Lit {
    /// The road it was made for, by the number it was laid with.
    laid: u64,
    material: Handle<Material>,
    params: Vec<f32>,
    /// The light's entity, with a piece under it for each piece of
    /// the groove.
    over: Entity,
}

/// The grooves' materials.
pub(super) struct Grooves {
    shader: Handle<Shader>,
    unlit: Handle<Material>,
    /// The bed's colour and the strip's, the first params of each.
    stone: [f32; 8],
    lit: HashMap<Pair, Lit>,
}

impl Grooves {
    pub(super) fn make(
        pal: &Palette,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let shader = draw.assets.add(Shader::compile(
            ShaderKind::Material,
            include_str!("../../build/inlay.wgsl"),
        )?);
        let stone = [
            pal.bed.x,
            pal.bed.y,
            pal.bed.z,
            1.0,
            pal.pewter.x,
            pal.pewter.y,
            pal.pewter.z,
            1.0,
        ];
        let unlit = draw.assets.add_shaded_material(
            &shader,
            Shaded {
                alpha_mode: AlphaMode::Opaque,
                double_sided: false,
                lit: true,
            },
            &stone,
            None,
            None,
        )?;
        Ok(Self {
            shader,
            unlit,
            stone,
            lit: HashMap::new(),
        })
    }

    /// The material of a groove with nothing to show.
    pub(super) fn unlit(&self) -> &Handle<Material> {
        &self.unlit
    }

    /// Lights the grooves that have something to show, and puts out
    /// the rest.
    pub(super) fn light(
        &mut self,
        ctx: &Ctx,
        drawn: &HashMap<Pair, Drawn>,
        trips: &[Trip],
        draw: &mut Draw,
    ) {
        let (society, looks) = (ctx.society, &ctx.tuning.looks);
        let gain = ctx.sky.look.inlay_gain;
        let speed = ctx.land.set.road.walk_speed;
        let mut wanted = HashMap::new();
        for agent in society.agents.iter().filter(|a| !a.archived) {
            let (hue, mode, reach) = match agent.state {
                KState::Waiting => (SIGNAL, BREATH, 4.5),
                KState::Fault => (FAULT, STUTTER, 7.0),
                _ => continue,
            };
            let Some(town) = society.town(agent.town) else {
                continue;
            };
            let pair = Pair::new(agent.id, town.leader);
            let Some(road) = drawn.get(&pair) else {
                continue;
            };
            let total = road.route.total();
            let range = if road.first == agent.id {
                (-1.0, reach)
            } else {
                (total - reach, total + 1.0)
            };
            let hue = to_linear(hue);
            let mut params = self.stone.to_vec();
            params.extend([
                hue.x,
                hue.y,
                hue.z,
                LIT * gain,
                range.0,
                range.1,
                0.0,
                total,
                mode,
                0.0,
                0.0,
                looks.trace_half_life,
                looks.strip_share,
                1.0,
                speed,
                0.0,
            ]);
            wanted.insert(pair, params);
        }
        for trip in trips {
            let Some(road) = drawn.get(&trip.pair) else {
                continue;
            };
            let total = road.route.total();
            let forward = (road.first == trip.walker) != trip.back;
            let mote = if road.first == trip.walker {
                trip.gone
            } else {
                total - trip.gone
            };
            // The shader leaves a trace behind a mote that runs up
            // the line. One running down it shows only
            // its head.
            let from = if forward { 0.0 } else { total + 1.0 };
            let hue = to_linear(CURRENT);
            let mut params = self.stone.to_vec();
            params.extend([
                hue.x,
                hue.y,
                hue.z,
                LIT * gain,
                0.0,
                total,
                0.0,
                total + 2.0,
                TRAVEL,
                0.0,
                mote,
                looks.trace_half_life,
                looks.strip_share,
                1.0,
                speed,
                from,
            ]);
            wanted.insert(trip.pair, params);
        }
        self.lit.retain(|pair, lit| {
            let kept = drawn
                .get(pair)
                .is_some_and(|road| road.laid == lit.laid)
                && wanted.contains_key(pair);
            if !kept {
                draw.despawn(lit.over);
            }
            kept
        });
        for (pair, params) in wanted {
            let Some(road) = drawn.get(&pair) else {
                continue;
            };
            match self.lit.get_mut(&pair) {
                Some(lit) => {
                    if lit.params != params {
                        let _ = draw
                            .assets
                            .set_params(&lit.material, &params);
                        lit.params = params;
                    }
                }
                None => match self.lit_over(road, params, draw) {
                    Ok(lit) => {
                        self.lit.insert(pair, lit);
                    }
                    Err(why) => {
                        hamlet_engine_bevy::warn(&format!(
                            "a groove's light: {why}"
                        ));
                    }
                },
            }
        }
    }

    /// The light over the groove of `road`, given `params`.
    fn lit_over(
        &self,
        road: &Drawn,
        params: Vec<f32>,
        draw: &mut Draw,
    ) -> Result<Lit, String> {
        let material = draw.assets.add_shaded_material(
            &self.shader,
            Shaded {
                alpha_mode: AlphaMode::Add,
                double_sided: false,
                lit: false,
            },
            &params,
            None,
            None,
        )?;
        let over = draw.spawn(Some(road.entity), 0);
        draw.place(over, Frame::at(Vec3::Y * OVER));
        for mesh in &road.groove {
            let piece = draw.spawn(Some(over), 0);
            draw.draw(piece, mesh, &material);
            draw.shadows(piece, false, false);
        }
        Ok(Lit {
            laid: road.laid,
            material,
            params,
            over,
        })
    }
}
