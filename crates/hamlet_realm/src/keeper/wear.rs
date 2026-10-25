//! What a robot wears (docs/looks.md): a skin over its materials, a
//! mesh in place of its parts and a clip played on top of its motion.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::gltf::{Gltf, GltfMesh};
use bevy::prelude::*;
use hamlet_vfx::inlay::{self, LIT};

use super::kit::parts::Part;
use super::kit::{self, Kit};
use super::pose::{dice, ramp};
use super::{Keeper, Motion, animate};
use crate::dress::{self, Dressing, Look, PlayEmote};
use crate::palette::Palette;
use crate::wardrobe::{Clip, Emote, PARTS, Skin, Target};
use crate::{Clock, Step};

/// The colours of the head's glass, unlit and lit, in sRGB.
const GLASS_DARK: Vec3 = Vec3::new(0.16, 0.15, 0.14);
const GLASS_LIT: Vec3 = Vec3::new(0.5, 0.46, 0.38);

/// The most a skin may brighten a colour its meshes are made in.
const MOST_TINT: f32 = 8.0;

pub fn plugin(app: &mut App) {
    app.add_observer(play_asked).add_systems(
        Update,
        (
            follow.run_if(
                resource_exists_and_changed::<Dressing>
                    .or_eager(any_match_filter::<Changed<Wearer>>)
                    .or_eager(any_component_removed::<Wearer>),
            ),
            fit,
        )
            .chain()
            .in_set(Step::Robots)
            .before(animate),
    );
}

/// What a robot wears. Put it on a robot's root, or give the robot a
/// [`Wearer`] to have it wear what that agent wears.
#[derive(Component, Clone, Debug, Default, PartialEq)]
#[require(Gesture, Fitted)]
pub struct Wears {
    pub skin: Option<Skin>,
    /// A `.glb` whose meshes, named as in
    /// [`MESH_PARTS`](super::MESH_PARTS), replace the robot's parts.
    pub mesh: Option<Handle<Gltf>>,
    pub clip: Option<Clip>,
}

impl Wears {
    pub fn of(look: &Look) -> Self {
        Wears {
            skin: look.skin.clone(),
            mesh: look.mesh.clone(),
            clip: match &look.emote {
                Some(Emote::Clip(clip)) => Some(clip.clone()),
                _ => None,
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.skin.is_none()
            && self.mesh.is_none()
            && self.clip.is_none()
    }
}

/// The name of the agent whose looks a robot wears.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Wearer(pub String);

/// The playing of a robot's worn clip.
#[derive(Component, Default, Debug)]
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
    /// time `now`, eased in and out; None while it does not play. It
    /// begins when asked, and while `idle` once it is due.
    pub(super) fn moves(
        &mut self,
        clip: &Clip,
        now: f32,
        idle: bool,
        seed: u32,
        motion: &Motion,
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
            let spread = motion.emote_spread * dice(self.plays, seed);
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
        let blend = motion.emote_blend.max(1e-3);
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

/// How a skin colours a mesh of a robot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Slot {
    Paint,
    Brass,
    Steel,
    /// Left as it is.
    Plain,
    /// The robot's own light, which it colours as it draws it.
    Lit,
}

/// A mesh of a robot as it was spawned.
pub(super) struct Shape {
    entity: Entity,
    part: Part,
    slot: Slot,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

impl Shape {
    pub fn new(
        entity: Entity,
        part: Part,
        slot: Slot,
        mesh: &Handle<Mesh>,
        material: &Handle<StandardMaterial>,
    ) -> Self {
        Shape {
            entity,
            part,
            slot,
            mesh: mesh.clone(),
            material: material.clone(),
        }
    }
}

/// What of a robot's [`Wears`] is on it.
#[derive(Component, Default)]
struct Fitted {
    /// The skin worn, by its key.
    skin: Option<u64>,
    /// The worn mesh is still loading.
    waiting: bool,
}

/// A material a skin makes, shared by every robot wearing it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Swatch {
    /// Over one of the robots' paints.
    Paint(u8),
    Brass,
    Steel,
}

/// Puts on each named robot what its agent wears.
fn follow(
    dressing: Option<Res<Dressing>>,
    robots: Query<(Entity, Ref<Wearer>, Option<&Wears>)>,
    mut unnamed: RemovedComponents<Wearer>,
    mut commands: Commands,
) {
    for robot in unnamed.read() {
        if robots.get(robot).is_err() {
            commands.entity(robot).try_remove::<Wears>();
        }
    }
    let Some(dressing) = dressing else {
        return;
    };
    let all = dressing.is_changed();
    for (robot, wearer, wears) in &robots {
        if !all && !wearer.is_changed() {
            continue;
        }
        let want = dressing
            .look(Target::Agent, &wearer.0)
            .map(Wears::of)
            .unwrap_or_default();
        match (want.is_empty(), wears) {
            (true, Some(_)) => {
                commands.entity(robot).try_remove::<Wears>();
            }
            (false, Some(wears)) if *wears == want => {}
            (false, _) => {
                commands.entity(robot).try_insert(want);
            }
            (true, None) => {}
        }
    }
}

fn play_asked(
    asked: On<PlayEmote>,
    clock: Res<Clock>,
    mut robots: Query<(&Wearer, &mut Gesture)>,
) {
    for (wearer, mut gesture) in &mut robots {
        if wearer.0 == asked.event().0 {
            gesture.play(clock.t);
        }
    }
}

/// The assets [`fit`] reads.
#[derive(bevy::ecs::system::SystemParam)]
struct Stores<'w> {
    kit: Res<'w, Kit>,
    pal: Res<'w, Palette>,
    gltfs: Option<Res<'w, Assets<Gltf>>>,
    parts: Option<Res<'w, Assets<GltfMesh>>>,
    meshes: Res<'w, Assets<Mesh>>,
    server: Option<Res<'w, AssetServer>>,
}

/// Puts a robot's [`Wears`] on its meshes, and takes them off once it
/// is removed.
fn fit(
    stores: Stores,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut swatches: Local<
        HashMap<(u64, Swatch), Handle<StandardMaterial>>,
    >,
    mut worn: Query<(&mut Keeper, Ref<Wears>, &mut Fitted)>,
    mut bare: Query<
        (Entity, &mut Keeper),
        (With<Fitted>, Without<Wears>),
    >,
    mut shapes: Query<(
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Visibility,
    )>,
    mut commands: Commands,
) {
    for (mut keeper, wears, mut fitted) in &mut worn {
        if !wears.is_changed() && !fitted.waiting {
            continue;
        }
        let skin = wears.skin.as_ref().map(|skin| (key(skin), skin));
        let mut waiting = false;
        let swap = |part: Part, waiting: &mut bool| {
            let gltf = wears.mesh.as_ref()?;
            let (Some(gltfs), Some(parts)) =
                (&stores.gltfs, &stores.parts)
            else {
                return None;
            };
            if gltfs.get(gltf).is_none() {
                *waiting =
                    stores.server.as_ref().is_none_or(|server| {
                        !server.load_state(gltf).is_failed()
                    });
                return None;
            }
            let (own, shared) = part.names();
            let mesh = dress::part(gltf, own, gltfs, parts).or_else(
                || dress::part(gltf, shared?, gltfs, parts),
            )?;
            if !stores.meshes.contains(&mesh) {
                *waiting = true;
                return None;
            }
            Some(mesh)
        };
        let who = keeper.who;
        let mut skinned = |slot: Slot| {
            let (key, skin) = skin?;
            let swatch = match slot {
                Slot::Paint => Swatch::Paint(who.paint as u8),
                Slot::Brass => Swatch::Brass,
                Slot::Steel => Swatch::Steel,
                Slot::Plain | Slot::Lit => return None,
            };
            if let Some(made) = swatches.get(&(key, swatch)) {
                return Some(made.clone());
            }
            let made = swatch_of(swatch, skin, &stores, &materials)?;
            let made = materials.add(made);
            swatches.insert((key, swatch), made.clone());
            Some(made)
        };
        let mut done = 0u32;
        for shape in &keeper.parts.shapes {
            let first = done & (1 << shape.part as u32) == 0;
            done |= 1 << shape.part as u32;
            let (mesh, material, shown) =
                match swap(shape.part, &mut waiting) {
                    Some(mesh) if first => {
                        let material = match shape.slot {
                            Slot::Lit => shape.material.clone(),
                            _ => stores.kit.matte.clone(),
                        };
                        (mesh, material, true)
                    }
                    Some(_) => (
                        shape.mesh.clone(),
                        shape.material.clone(),
                        false,
                    ),
                    None => (
                        shape.mesh.clone(),
                        skinned(shape.slot).unwrap_or_else(|| {
                            shape.material.clone()
                        }),
                        true,
                    ),
                };
            put(&mut shapes, shape.entity, mesh, material, shown);
        }
        fitted.waiting = waiting;
        let key = skin.map(|(key, _)| key);
        if fitted.skin != key {
            fitted.skin = key;
            // Lights it again, in the skin's colours.
            keeper.lit_as = Vec3::splat(-1.0);
        }
    }
    for (robot, mut keeper) in &mut bare {
        for shape in &keeper.parts.shapes {
            put(
                &mut shapes,
                shape.entity,
                shape.mesh.clone(),
                shape.material.clone(),
                true,
            );
        }
        keeper.lit_as = Vec3::splat(-1.0);
        commands.entity(robot).try_remove::<(Fitted, Gesture)>();
    }
}

fn put(
    shapes: &mut Query<(
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Visibility,
    )>,
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    shown: bool,
) {
    let Ok((mut on, mut drawn, mut visibility)) =
        shapes.get_mut(entity)
    else {
        return;
    };
    if on.0 != mesh {
        on.0 = mesh;
    }
    if drawn.0 != material {
        drawn.0 = material;
    }
    visibility.set_if_neq(if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

/// The material `skin` makes for `swatch`, if it sets it.
fn swatch_of(
    swatch: Swatch,
    skin: &Skin,
    stores: &Stores,
    materials: &Assets<StandardMaterial>,
) -> Option<StandardMaterial> {
    let pal = &stores.pal;
    // The meshes are made in the palette's colours, which the
    // material's colour multiplies.
    let tint =
        |slot: &str, made_in: Vec3, of: &Handle<StandardMaterial>| {
            let want = dress::colour(skin, slot)?;
            let by = (want / made_in.max(Vec3::splat(1e-3)))
                .min(Vec3::splat(MOST_TINT));
            let mut made = materials.get(of)?.clone();
            made.base_color = Color::linear_rgb(by.x, by.y, by.z);
            Some(made)
        };
    match swatch {
        Swatch::Paint(paint) => {
            let colour = dress::colour(skin, "paint");
            if colour.is_none() && skin.finish.is_none() {
                return None;
            }
            let colour = colour.unwrap_or(pal.paints[paint as usize]);
            let mut made = kit::matte(colour);
            if let Some(finish) = skin.finish {
                made.metallic = finish.metallic;
                made.perceptual_roughness = finish.roughness;
                let glow = colour * finish.glow * LIT;
                made.emissive =
                    LinearRgba::rgb(glow.x, glow.y, glow.z);
            }
            Some(made)
        }
        Swatch::Brass => tint("brass", pal.brass, &stores.kit.brass),
        Swatch::Steel => tint("steel", pal.timber, &stores.kit.matte),
    }
}

/// A skin's key, the same for skins of the same colours and finish.
fn key(skin: &Skin) -> u64 {
    let mut hasher = DefaultHasher::new();
    skin.colours.hash(&mut hasher);
    if let Some(finish) = skin.finish {
        [finish.metallic, finish.roughness, finish.glow]
            .map(f32::to_bits)
            .hash(&mut hasher);
    }
    hasher.finish()
}

/// The glow of a robot's light in `skin`: its `lamp` colour in place
/// of the candle. The colours that tell its state stay.
pub(super) fn relight(glow: Vec3, skin: Option<&Skin>) -> Vec3 {
    let Some(lamp) =
        skin.and_then(|skin| dress::colour(skin, "lamp"))
    else {
        return glow;
    };
    let strength = glow.max_element();
    if strength <= 0.0 {
        return glow;
    }
    let candle = inlay::candle();
    let off =
        (glow / strength).distance(candle / candle.max_element());
    // Eased by how near the light is to the candle's colour, so that
    // a change to or from the candle does not jump.
    let near = 1.0 - ramp(0.05, 0.3, off);
    let lamp = lamp / lamp.max_element().max(1e-4);
    glow.lerp(lamp * strength, near)
}

/// The colour of a robot's glass in `skin`, lit by `lit` from 0 to 1.
pub(super) fn glass(skin: Option<&Skin>, lit: f32) -> Color {
    let (dark, bright) =
        match skin.and_then(|skin| skin.colour("glass")) {
            Some(c) => {
                (Vec3::from(c), Vec3::from(c).lerp(Vec3::ONE, 0.35))
            }
            None => (GLASS_DARK, GLASS_LIT),
        };
    let c = dark.lerp(bright, lit);
    Color::srgb(c.x, c.y, c.z)
}
