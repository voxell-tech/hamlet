//! The light of the grooves: orange at the door of an agent that
//! waits, red where one has faulted. Every other groove shares one
//! material that is not lit.

use std::collections::HashMap;

use glam::Vec3;
use hamlet_engine_sdk::scene::gpu::{
    Shaded, Shader, ShaderKind, set_params, shaded_material,
};
use hamlet_engine_sdk::scene::{AlphaMode, Entity, Material};

use super::Drawn;
use crate::frame::Frame;
use crate::keeper::KState;
use crate::palette::Palette;
use crate::realm::Ctx;
use crate::society::Pair;
use crate::tint::to_linear;

/// The light of a groove that is lit fully, against sunlit white
/// paper at 1.
const LIT: f32 = 4.0;

/// How the strip is lit, as the shader reads it.
const BREATH: f32 = 3.0;
const STUTTER: f32 = 4.0;

/// Height of the light over the groove it lies on.
const OVER: f32 = 0.003;

/// The light's colours, in sRGB: an agent that waits, and one that
/// faulted.
const SIGNAL: Vec3 = Vec3::new(1.0, 0.541, 0.122);
const FAULT: Vec3 = Vec3::new(1.0, 0.322, 0.278);

/// The light over one road's groove.
struct Lit {
    /// The road it was made for, by the number it was laid with.
    laid: u64,
    material: Material,
    params: Vec<f32>,
    /// Kept only so it is not dropped: the light's entity and a
    /// piece under it for each piece of the groove.
    _held: (Entity, Vec<Entity>),
}

/// The grooves' materials.
pub(super) struct Grooves {
    shader: Shader,
    unlit: Material,
    /// The bed's colour and the strip's, the first params of each.
    stone: [f32; 8],
    lit: HashMap<Pair, Lit>,
}

impl Grooves {
    pub(super) fn make(pal: &Palette) -> Result<Self, String> {
        let shader = Shader::compile(
            ShaderKind::Material,
            include_str!("groove.wgsl"),
        )?;
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
        let unlit = shaded_material(
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
    pub(super) fn unlit(&self) -> &Material {
        &self.unlit
    }

    /// Lights the grooves that have something to show, and puts out
    /// the rest.
    pub(super) fn light(
        &mut self,
        ctx: &Ctx,
        drawn: &HashMap<Pair, Drawn>,
    ) {
        let (society, looks) = (ctx.society, &ctx.tuning.looks);
        let gain = ctx.sky.look.inlay_gain;
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
                0.0,
                0.0,
            ]);
            wanted.insert(pair, params);
        }
        self.lit.retain(|pair, lit| {
            drawn.get(pair).is_some_and(|road| road.laid == lit.laid)
                && wanted.contains_key(pair)
        });
        for (pair, params) in wanted {
            let Some(road) = drawn.get(&pair) else {
                continue;
            };
            match self.lit.get_mut(&pair) {
                Some(lit) => {
                    if lit.params != params {
                        let _ = set_params(&lit.material, &params);
                        lit.params = params;
                    }
                }
                None => match self.lit_over(road, params) {
                    Ok(lit) => {
                        self.lit.insert(pair, lit);
                    }
                    Err(why) => {
                        hamlet_engine_sdk::warn(&format!(
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
    ) -> Result<Lit, String> {
        let material = shaded_material(
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
        let over = Entity::make(Some(&road.entity), 0)?;
        over.set_transform(Frame::at(Vec3::Y * OVER).scene());
        let mut pieces = Vec::new();
        for mesh in &road.groove {
            let piece = Entity::make(Some(&over), 0)?;
            piece.draw(mesh, &material);
            piece.set_shadows(false, false);
            pieces.push(piece);
        }
        Ok(Lit {
            laid: road.laid,
            material,
            params,
            _held: (over, pieces),
        })
    }
}
