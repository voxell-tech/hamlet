//! What a robot wears (docs/looks.md, "How an agent wears them"): a
//! skin over its materials, a mesh in place of its parts and a clip
//! played on top of its motion.

use std::collections::HashMap;

use glam::{EulerRot, Quat, Vec3};
use hamlet_engine_sdk::scene::{Material, Mesh, MeshData, Pbr};
use looks::wardrobe::{Clip, PARTS, Skin};

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::stock::rgba;
use crate::build::upload::{Unsent, Upload};
use crate::folk::keeper::pose::{LIT, candle, dice, ramp};
use crate::folk::keeper::who::Paint;
use crate::wardrobe::{AgentLook, Wardrobe};
use crate::world::palette::Palette;
use crate::world::tint::to_linear;

/// The most a skin may brighten a colour its meshes are made in.
const MOST_TINT: f32 = 8.0;

/// How a robot plays the clip it wears.
#[derive(
    Clone, Copy, PartialEq, Debug, Default, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct WearSettings {
    /// Seconds a clip takes to ease in and out of the robot's own
    /// motion.
    pub emote_blend: f32,
    /// The share of a clip's `every` an idle robot waits on top of
    /// it before it plays the clip again, at most.
    pub emote_spread: f32,
}

/// Every name a worn mesh's parts are found by. A part of one side,
/// such as `arm_l`, is found by its own name first and then by the
/// name both sides share, such as `arm`.
pub const MESH_PARTS: [&str; 23] = [
    "torso",
    "key",
    "emblem",
    "mantle",
    "arm",
    "arm_l",
    "arm_r",
    "leg",
    "leg_l",
    "leg_r",
    "wheels",
    "wheels_l",
    "wheels_r",
    "head",
    "glass",
    "eye",
    "eye_l",
    "eye_r",
    "hat",
    "pennant",
    "pole",
    "lamp",
    "lamp_glass",
];

/// A part of a robot a worn mesh can replace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Torso,
    Key,
    Emblem,
    Mantle,
    ArmL,
    ArmR,
    LegL,
    LegR,
    WheelsL,
    WheelsR,
    Head,
    Glass,
    EyeL,
    EyeR,
    Hat,
    Pennant,
    Pole,
    Lamp,
    LampGlass,
}

impl Part {
    /// Its name in a worn mesh, and the name it shares with the part
    /// of the other side.
    pub fn names(self) -> (&'static str, Option<&'static str>) {
        match self {
            Part::Torso => ("torso", None),
            Part::Key => ("key", None),
            Part::Emblem => ("emblem", None),
            Part::Mantle => ("mantle", None),
            Part::ArmL => ("arm_l", Some("arm")),
            Part::ArmR => ("arm_r", Some("arm")),
            Part::LegL => ("leg_l", Some("leg")),
            Part::LegR => ("leg_r", Some("leg")),
            Part::WheelsL => ("wheels_l", Some("wheels")),
            Part::WheelsR => ("wheels_r", Some("wheels")),
            Part::Head => ("head", None),
            Part::Glass => ("glass", None),
            Part::EyeL => ("eye_l", Some("eye")),
            Part::EyeR => ("eye_r", Some("eye")),
            Part::Hat => ("hat", None),
            Part::Pennant => ("pennant", None),
            Part::Pole => ("pole", None),
            Part::Lamp => ("lamp", None),
            Part::LampGlass => ("lamp_glass", None),
        }
    }

    /// True for the parts of wood that are steel in a skin.
    fn steel(self) -> bool {
        matches!(
            self,
            Part::ArmL
                | Part::ArmR
                | Part::LegL
                | Part::LegR
                | Part::Pole
        )
    }
}

/// The material a mesh of a robot is drawn with by default.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mat {
    /// The robot's own paint.
    Paint,
    Matte,
    Brass,
    /// Its light, which it colours as it draws it.
    Lit,
}

/// The materials a skin makes for every robot that wears it, or their
/// looks, and the colours of its glass and lamp.
pub struct Swatches<M = Material> {
    /// One for each of the robots' paints, or one for all when the
    /// skin sets the paint.
    paint: Vec<M>,
    brass: Option<M>,
    steel: Option<M>,
    /// In sRGB, as the skin gives it.
    pub glass: Option<Vec3>,
    /// In linear RGB.
    pub lamp: Option<Vec3>,
}

impl Swatches<Pbr> {
    /// The looks of the materials `skin` makes, its colours over
    /// `pal`'s.
    pub fn of(skin: &Skin, pal: &Palette) -> Self {
        let colour = |slot: &str| skin.colour(slot).map(Vec3::from);
        let linear = |slot: &str| colour(slot).map(to_linear);
        // The meshes are made in the palette's colours, which the
        // material's colour multiplies.
        let tint = |want: Vec3, made_in: Vec3| {
            (want / made_in.max(Vec3::splat(1e-3)))
                .min(Vec3::splat(MOST_TINT))
        };
        let paint = match (linear("paint"), skin.finish) {
            (None, None) => Vec::new(),
            (Some(colour), finish) => vec![painted(colour, finish)],
            (None, finish) => Paint::ALL
                .iter()
                .map(|p| painted(pal.paints[*p as usize], finish))
                .collect(),
        };
        let brass = linear("brass").map(|want| Pbr {
            metallic: 0.75,
            roughness: 0.42,
            ..Pbr::colored(rgba(tint(want, pal.brass)))
        });
        let steel = linear("steel").map(|want| Pbr {
            roughness: 0.92,
            ..Pbr::colored(rgba(tint(want, pal.timber)))
        });
        Self {
            paint,
            brass,
            steel,
            glass: colour("glass"),
            lamp: linear("lamp"),
        }
    }
}

impl Swatches {
    /// The materials `skin` makes, its colours over `pal`'s.
    pub fn make(skin: &Skin, pal: &Palette) -> Result<Self, String> {
        let looks = Swatches::<Pbr>::of(skin, pal);
        Ok(Self {
            paint: looks
                .paint
                .into_iter()
                .map(Material::make)
                .collect::<Result<Vec<_>, _>>()?,
            brass: looks.brass.map(Material::make).transpose()?,
            steel: looks.steel.map(Material::make).transpose()?,
            glass: looks.glass,
            lamp: looks.lamp,
        })
    }
}

impl<M> Swatches<M> {
    /// The material for a mesh of `part` drawn by default with
    /// `mat`, on a robot of `paint`, when the skin sets it.
    pub fn material(
        &self,
        part: Part,
        mat: Mat,
        paint: Paint,
    ) -> Option<&M> {
        match mat {
            Mat::Paint => self.paint.get(
                (paint as usize).min(self.paint.len().max(1) - 1),
            ),
            Mat::Brass => self.brass.as_ref(),
            Mat::Matte if part.steel() => self.steel.as_ref(),
            Mat::Matte | Mat::Lit => None,
        }
    }
}

/// Paint of `colour` with `finish`.
fn painted(
    colour: Vec3,
    finish: Option<looks::wardrobe::Finish>,
) -> Pbr {
    let mut pbr = Pbr {
        roughness: 0.92,
        ..Pbr::colored(rgba(colour))
    };
    if let Some(finish) = finish {
        pbr.metallic = finish.metallic;
        pbr.roughness = finish.roughness;
        pbr.emissive = rgba(colour * finish.glow * LIT);
    }
    pbr
}

/// The meshes of a worn `.glb` by the names of the parts they stand
/// for, handed over a few at a time.
#[derive(Default)]
pub struct PartMeshes {
    meshes: Vec<(String, Mesh)>,
    waiting: Vec<(String, MeshData)>,
}

impl PartMeshes {
    /// True once every mesh is handed over.
    pub fn whole(&self) -> bool {
        self.waiting.is_empty()
    }

    /// The mesh that stands for `part`.
    pub fn of(&self, part: Part) -> Option<&Mesh> {
        let (own, shared) = part.names();
        let find = |name: &str| {
            self.meshes
                .iter()
                .find(|(named, _)| named == name)
                .map(|(_, mesh)| mesh)
        };
        find(own).or_else(|| find(shared?))
    }
}

/// What the robots wear, shared by all that wear the same: the
/// materials of each skin and the meshes of each `.glb`, by file.
#[derive(Default)]
pub struct Wear {
    swatches: HashMap<String, Swatches>,
    meshes: HashMap<String, PartMeshes>,
}

impl Wear {
    /// Makes what the looks of `wardrobe`'s agents need and lets go
    /// of what none wears.
    pub fn follow(&mut self, wardrobe: &Wardrobe, pal: &Palette) {
        let (mut skins, mut meshes) = (Vec::new(), Vec::new());
        for look in wardrobe.agent_looks() {
            if let Some((file, skin)) = &look.skin {
                skins.push(file.as_str());
                if !self.swatches.contains_key(file) {
                    match Swatches::make(skin, pal) {
                        Ok(made) => {
                            self.swatches.insert(file.clone(), made);
                        }
                        Err(why) => hamlet_engine_sdk::warn(
                            &format!("looks: {file}: {why}"),
                        ),
                    }
                }
            }
            if let Some(file) = &look.mesh {
                meshes.push(file.as_str());
                if !self.meshes.contains_key(file)
                    && let Some(glb) = wardrobe.glb(file)
                {
                    let waiting = glb
                        .meshes
                        .iter()
                        .filter(|(name, _)| {
                            MESH_PARTS.contains(&name.as_str())
                        })
                        .map(|(name, model)| {
                            let mut mb = Mb::new();
                            model.lay(&mut mb, Default::default());
                            (name.clone(), mb.data())
                        })
                        .collect();
                    self.meshes.insert(
                        file.clone(),
                        PartMeshes {
                            meshes: Vec::new(),
                            waiting,
                        },
                    );
                }
            }
        }
        self.swatches
            .retain(|file, _| skins.contains(&file.as_str()));
        self.meshes
            .retain(|file, _| meshes.contains(&file.as_str()));
    }

    /// Hands over the meshes waiting, within `upload`. True when a
    /// worn mesh became whole.
    pub fn hand_over(&mut self, upload: &mut Upload) -> bool {
        let mut whole = false;
        for parts in self.meshes.values_mut() {
            if parts.whole() {
                continue;
            }
            while let Some((name, data)) = parts.waiting.pop() {
                match upload.mesh(data) {
                    Ok(mesh) => parts.meshes.push((name, mesh)),
                    Err(Unsent::Full(data)) => {
                        parts.waiting.push((name, data));
                        return whole;
                    }
                    Err(Unsent::Refused(why)) => {
                        hamlet_engine_sdk::warn(&format!(
                            "looks: a robot's part: {why}"
                        ));
                    }
                }
            }
            whole = true;
        }
        whole
    }

    /// What `look` dresses a robot in: its skin's materials and its
    /// mesh, once whole.
    pub fn dress(&self, look: &AgentLook) -> Dress<'_> {
        Dress {
            skin: look
                .skin
                .as_ref()
                .and_then(|(file, _)| self.swatches.get(file)),
            mesh: look
                .mesh
                .as_ref()
                .and_then(|file| self.meshes.get(file))
                .filter(|parts| parts.whole()),
        }
    }

    /// True when the mesh `look` wears is still being handed over.
    pub fn waits(&self, look: &AgentLook) -> bool {
        look.mesh.as_ref().is_some_and(|file| {
            self.meshes.get(file).is_some_and(|parts| !parts.whole())
        })
    }
}

/// What a robot is dressed in.
#[derive(Clone, Copy, Default)]
pub struct Dress<'a> {
    pub skin: Option<&'a Swatches>,
    pub mesh: Option<&'a PartMeshes>,
}

/// The playing of a robot's worn clip.
#[derive(Default, Debug)]
pub struct Gesture {
    /// Clock time the clip began, while it plays.
    since: Option<f32>,
    /// Clock time it was asked to begin at.
    asked: Option<f32>,
    /// Clock time from which it may play again while idle.
    next: Option<f32>,
    plays: i32,
}

impl Gesture {
    /// Plays the worn clip from clock time `now`.
    pub fn play(&mut self, now: f32) {
        self.asked = Some(now);
    }

    /// The turn and shift `clip` adds to each of [`PARTS`] at clock
    /// time `now`, eased in and out; none while it does not play. It
    /// begins when asked, and while `idle` once it is due.
    pub fn moves(
        &mut self,
        clip: &Clip,
        now: f32,
        idle: bool,
        seed: u32,
        set: &WearSettings,
    ) -> Option<[(Quat, Vec3); PARTS.len()]> {
        let every = clip.every.max(clip.seconds);
        let mut start = self.asked.take();
        if start.is_none() && idle && self.since.is_none() {
            // A first wait of up to `every`, so that robots that put
            // the clip on together do not play it together.
            let next = *self
                .next
                .get_or_insert_with(|| now + every * dice(0, seed));
            if now >= next {
                start = Some(now);
            }
        }
        if let Some(at) = start {
            self.since = Some(at);
            self.plays += 1;
            let spread = set.emote_spread * dice(self.plays, seed);
            self.next = Some(at + every * (1.0 + spread));
        }
        let tau = now - self.since?;
        if tau >= clip.seconds {
            self.since = None;
            return None;
        }
        if tau < 0.0 {
            return None;
        }
        let blend = set.emote_blend.max(1e-3);
        let w = ramp(0.0, blend, tau)
            * ramp(0.0, blend, clip.seconds - tau);
        let at = clip.at(tau);
        Some(PARTS.map(|name| {
            at.get(name).map_or((Quat::IDENTITY, Vec3::ZERO), |m| {
                let [x, y, z] = m.turn.map(|d| d.to_radians() * w);
                (
                    Quat::from_euler(EulerRot::XYZ, x, y, z),
                    Vec3::from(m.shift) * w,
                )
            })
        }))
    }
}

/// `frames`, in the order of [`PARTS`], each turned after its own
/// turn and moved in its parent's frame by `moves`.
pub fn moved(
    frames: [&mut Frame; PARTS.len()],
    moves: [(Quat, Vec3); PARTS.len()],
) {
    for (frame, (turn, shift)) in frames.into_iter().zip(moves) {
        frame.turn *= turn;
        frame.at += shift;
    }
}

/// The glow of a robot's light with `lamp` in place of the candle.
/// The colours that tell its state stay.
pub fn relight(glow: Vec3, lamp: Option<Vec3>) -> Vec3 {
    let Some(lamp) = lamp else {
        return glow;
    };
    let strength = glow.max_element();
    if strength <= 0.0 {
        return glow;
    }
    let flame = candle(1.0);
    let off = (glow / strength).distance(flame / flame.max_element());
    // Eased by how near the light is to the candle's colour, so that
    // a change to or from the candle does not jump.
    let near = 1.0 - ramp(0.05, 0.3, off);
    let lamp = lamp / lamp.max_element().max(1e-4);
    glow.lerp(lamp * strength, near)
}

#[cfg(test)]
mod tests;
