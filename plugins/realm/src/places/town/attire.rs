//! What a hamlet's houses wear (docs/looks.md, "How a hamlet wears
//! them"): a skin's colours in place of the palette's, a mesh in
//! place of their walls, and an emote about every one of them, its
//! moving parts copies of one mesh moved on `tick` while the town is
//! seen near.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Quat, Vec3};
use hamlet_engine_sdk::scene::{
    AlphaMode, Entity, Instances, Material, Mesh, MeshData, Pbr,
    Shape, copies,
};
use looks::wardrobe::{Emote, Skin, rgb};

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::stock::{Stock, rgba};
use crate::build::upload::{Unsent, Upload};
use crate::places::house::{Bulk, Fit, Shell, Spots};
use crate::wardrobe::glb::Glb;
use crate::wardrobe::{HamletLook, Wardrobe};
use crate::world::palette::Palette;
use crate::world::tint::to_linear;

/// How the emotes of hamlets move and show.
#[derive(
    Clone, Copy, PartialEq, Debug, Default, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct AttireSettings {
    /// Seconds a puff of smoke takes to rise and fade.
    pub puff_life: f32,
    /// The most puffs in the air over one chimney.
    pub puffs: u32,
    /// The radius and height of a paper lantern, and the string it
    /// hangs from under the eave.
    pub lantern_radius: f32,
    pub lantern_height: f32,
    pub lantern_string: f32,
    /// The most lanterns on one house.
    pub lanterns: u32,
    /// Swings of a lantern a second, in radians.
    pub sway_speed: f32,
    /// How brightly a lantern's light glows after dark.
    pub lantern_glow: f32,
    /// The most petals falling about one house.
    pub petals: u32,
    /// The most fireflies about one house, and how far beyond its
    /// eaves they wander.
    pub fireflies: u32,
    pub firefly_reach: f32,
    /// The share of their glow fireflies keep by day, so that they
    /// are seen at all.
    pub firefly_day: f32,
    /// The most pinwheels on one house, a pinwheel's radius and the
    /// stick it stands on above the eave.
    pub pinwheels: u32,
    pub pinwheel_radius: f32,
    pub pinwheel_stick: f32,
}

/// What a hamlet's houses wear, ready to build them in.
#[derive(Clone, Debug, PartialEq)]
pub struct Attire {
    /// What it was made from.
    pub look: HamletLook,
    /// The palette its houses are built in, when a skin changes it.
    pub pal: Option<Palette>,
    pub shell: Option<Shell>,
}

impl Attire {
    /// `look` as its houses wear it, its mesh read from `wardrobe`
    /// and its skin over `pal`.
    pub fn of(
        look: HamletLook,
        wardrobe: &Wardrobe,
        pal: &Palette,
    ) -> Self {
        let shell = look
            .mesh
            .as_ref()
            .and_then(|file| wardrobe.glb(file))
            .and_then(Shell::of);
        Self {
            pal: look.skin.as_ref().map(|skin| skinned(pal, skin)),
            shell,
            look,
        }
    }

    /// What one of its houses is built wearing.
    pub fn fit(&self) -> Fit<'_> {
        let banner = match &self.look.emote {
            Some(Emote::Banners(banners)) => Some((
                banners.colours.each_ref().map(|hex| linear(hex)),
                banners.long.max(0.1),
            )),
            _ => None,
        };
        Fit {
            shell: self.shell.as_ref(),
            banner,
        }
    }
}

impl Shell {
    /// The mesh of `glb`, when it holds `walls`. The spots it does
    /// not mark are put where docs/looks.md says.
    pub fn of(glb: &Glb) -> Option<Self> {
        let walls = glb.mesh("walls")?.clone();
        let (min, max) = walls.bounds();
        let bulk = Bulk {
            min,
            max,
            tint: walls.tint(),
        };
        let top = Vec3::new(0.0, max.y, 0.0);
        let door =
            glb.spot("door").unwrap_or(Vec3::new(0.0, 0.0, max.z));
        let lamp = glb
            .spot("lamp")
            .unwrap_or(door + Vec3::new(0.6, 1.0, 0.3));
        let eaves = glb.spot("eaves").unwrap_or(Vec3::new(
            0.0,
            lamp.y + 0.5,
            max.z,
        ));
        Some(Self {
            panes: glb.mesh("panes").cloned(),
            spots: Spots {
                door,
                lamp,
                banner: glb.spot("banner").unwrap_or(top),
                chimney: glb.spot("chimney").unwrap_or(top),
                eaves,
            },
            bulk,
            walls,
        })
    }
}

/// `pal` with the colours `skin` sets for a hamlet in place of its
/// own: `roof`, `drift` and `banner` in every town's style, and
/// `livery` for every door and shutter.
pub fn skinned(pal: &Palette, skin: &Skin) -> Palette {
    let mut pal = pal.clone();
    for slot in skin.colours.keys() {
        let Some(c) =
            skin.colour(slot).map(|c| to_linear(Vec3::from(c)))
        else {
            continue;
        };
        match slot.as_str() {
            "plaster" => pal.plaster = c,
            "plaster_worn" => pal.plaster_worn = c,
            "whitewash" => pal.whitewash = c,
            "ochre" => pal.ochre = c,
            "limestone" => pal.limestone = c,
            "limestone_shade" => pal.limestone_shade = c,
            "timber" => pal.timber = c,
            "timber_dark" => pal.timber_dark = c,
            "terracotta" => pal.terracotta = c,
            "slate_roof" => pal.slate_roof = c,
            "moss" => pal.moss = c,
            "roof" => pal.towns.iter_mut().for_each(|t| t.roof = c),
            "drift" => pal.towns.iter_mut().for_each(|t| t.drift = c),
            "banner" => {
                pal.towns.iter_mut().for_each(|t| t.banner = c)
            }
            "livery" => pal.livery.fill(c),
            _ => {}
        }
    }
    pal
}

/// `"#rrggbb"` as the world's meshes are coloured: linear RGB.
fn linear(hex: &str) -> Vec3 {
    to_linear(Vec3::from(rgb(hex).unwrap_or([1.0; 3])))
}

/// A house an emote hangs about: its frame in the realm, and the top
/// of its chimney and the ends of its front eave in its own frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Perch {
    pub frame: Frame,
    pub chimney: Vec3,
    pub eaves: [Vec3; 2],
}

/// A kind of moving part of an emote.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Puff,
    Lantern,
    /// The petals of one of the two colours.
    Petal(usize),
    Firefly,
    Pinwheel,
    /// A pinwheel's stick, which stands still.
    Stick,
}

/// A number from 0 to 1 that is the same for the same `i` and `salt`.
fn hash(i: u32, salt: u32) -> f32 {
    let mut x =
        i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// A firefly's size.
const FIREFLY: f32 = 0.07;

/// Where each part of kind `kind` of `emote` is about one house at
/// clock time `t`, in the house's frame.
pub fn parts(
    emote: &Emote,
    kind: Kind,
    perch: &Perch,
    t: f32,
    set: &AttireSettings,
) -> Vec<Frame> {
    let [a, b] = perch.eaves;
    match (emote, kind) {
        (Emote::Smoke(smoke), Kind::Puff) => {
            let rate = smoke.rate.max(0.05);
            let n = ((rate * set.puff_life).ceil() as u32)
                .clamp(1, set.puffs.max(1));
            let life = n as f32 / rate;
            (0..n)
                .map(|i| {
                    let u = (t / life + i as f32 / n as f32).fract();
                    let drift = Vec3::new(0.25, 0.0, 0.1) * u * u;
                    Frame::at(
                        perch.chimney
                            + Vec3::Y * smoke.rise * u
                            + drift,
                    )
                    .with_size(Vec3::splat(
                        smoke.size
                            * (0.5 + 0.8 * u)
                            * (1.0 - u * u * u),
                    ))
                })
                .collect()
        }
        (Emote::Lanterns(lanterns), Kind::Lantern) => {
            let n = lanterns.count.min(set.lanterns);
            let sway = lanterns.sway.to_radians();
            (0..n)
                .map(|i| {
                    let u = (i as f32 + 0.5) / n as f32;
                    let swing = sway
                        * (t * set.sway_speed + i as f32 * 1.7).sin();
                    Frame::at(a.lerp(b, u))
                        .with_turn(Quat::from_rotation_z(swing))
                })
                .collect()
        }
        (Emote::Petals(petals), Kind::Petal(colour)) => {
            let n = petals.count.min(set.petals);
            (0..n)
                .filter(|i| *i as usize % 2 == colour)
                .map(|i| {
                    petal(
                        a.lerp(b, hash(i, 1))
                            + Vec3::new(
                                0.0,
                                0.2,
                                0.15 + 0.5 * hash(i, 2),
                            ),
                        (petals.size, petals.fall.max(0.5)),
                        (hash(i, 3), 0.3 + 0.4 * hash(i, 4)),
                        t,
                    )
                })
                .collect()
        }
        (Emote::Fireflies(fireflies), Kind::Firefly) => {
            // Out past the walls, which reach about as far as the
            // eave.
            let walls = a.x.abs().max(b.x.abs()).max(a.z.abs());
            let n = fireflies.count.min(set.fireflies);
            (0..n)
                .map(|i| {
                    firefly(
                        (
                            walls + 0.25,
                            set.firefly_reach
                                * (0.4 + 0.6 * hash(i, 5)),
                        ),
                        0.3 + a.y * 0.8 * hash(i, 6),
                        (0.15 + 0.2 * hash(i, 7), TAU * hash(i, 8)),
                        t,
                    )
                })
                .collect()
        }
        (
            Emote::Pinwheels(pinwheels),
            Kind::Pinwheel | Kind::Stick,
        ) => {
            let n = pinwheels.count.min(set.pinwheels).max(1);
            (0..n)
                .map(|i| {
                    let u = (i as f32 + 0.5) / n as f32;
                    let foot = a.lerp(b, u) + Vec3::Z * 0.05;
                    match kind {
                        Kind::Stick => Frame::at(foot),
                        _ => Frame::at(
                            foot + Vec3::Y * set.pinwheel_stick,
                        )
                        .with_turn(
                            Quat::from_rotation_z(
                                -(TAU * pinwheels.speed * t
                                    + i as f32 * 0.9),
                            ),
                        ),
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// A petal falling from `from`: its size and the seconds its fall
/// takes, how far into its fall it was at naught and how widely it
/// swings, at `t`.
fn petal(
    from: Vec3,
    (size, life): (f32, f32),
    (phase, swirl): (f32, f32),
    t: f32,
) -> Frame {
    let u = (t / life + phase).fract();
    let swing = (u * 9.0 + phase * 6.0).sin();
    let at = from
        + Vec3::new(swing * swirl, -(from.y - 0.02) * u, u * 0.6);
    Frame::at(at)
        .with_turn(
            Quat::from_rotation_z(swing * 0.8)
                * Quat::from_rotation_y(u * 7.0 + phase * 3.0),
        )
        // Born and gone small, never a pop.
        .with_size(Vec3::splat(
            size * (u * 12.0).min(1.0) * ((1.0 - u) * 12.0).min(1.0),
        ))
}

/// A firefly wandering in loops about its house: the nearest it comes
/// to the house's middle and how much further out it wanders, its
/// height, and its speed and own time, at `t`.
fn firefly(
    (near, reach): (f32, f32),
    high: f32,
    (speed, phase): (f32, f32),
    t: f32,
) -> Frame {
    let a = t * speed + phase;
    let r = near + reach * (0.5 + 0.5 * (a * 2.3).sin());
    let at = Vec3::new(
        r * a.cos(),
        high + 0.25 * (a * 3.1).sin(),
        r * a.sin(),
    );
    Frame::at(at).with_size(Vec3::splat(
        FIREFLY * (0.7 + 0.3 * (t * 5.0 + phase).sin().abs()),
    ))
}

/// A kind of moving part with its mesh, and the material of its own
/// with the colour it glows after dark, or none for the shared matte.
type Drawn<'a> = (Kind, &'a Mesh, Option<(Pbr, Vec3)>);

/// One kind of moving part, drawn as copies, and whether it was
/// last given any copy.
struct Set {
    kind: Kind,
    copies: Instances,
    shown: bool,
}

/// A hamlet's emote about its houses.
pub struct Emotes {
    emote: Emote,
    sets: Vec<Set>,
    /// The emote's own materials, and the colour each glows with
    /// after dark at its strongest.
    looks: Vec<(Material, Pbr, Vec3)>,
    _meshes: Vec<Mesh>,
    /// The houses the still parts were last laid about.
    laid: Vec<Perch>,
    candle: f32,
}

/// The meshes of an emote, built, as they are handed over.
pub struct Making {
    emote: Emote,
    meshes: Vec<(Kind, MeshData)>,
    made: Vec<(Kind, Mesh)>,
}

impl Making {
    /// Begins to make what `emote` hangs about its houses. None for
    /// banners, which are built with the houses.
    pub fn of(emote: &Emote, set: &AttireSettings) -> Option<Self> {
        Some(Self {
            emote: emote.clone(),
            meshes: meshes(emote, set)?
                .into_iter()
                .map(|(kind, mb)| (kind, mb.data()))
                .collect(),
            made: Vec::new(),
        })
    }

    /// Hands its meshes over within `upload`, and once all are there,
    /// makes the emote under `parent`.
    pub fn hand(
        &mut self,
        parent: &Entity,
        (stock, set, candle): (&Stock, &AttireSettings, f32),
        upload: &mut Upload,
    ) -> Option<Result<Emotes, String>> {
        while let Some((kind, data)) = self.meshes.pop() {
            match upload.mesh(data) {
                Ok(mesh) => self.made.push((kind, mesh)),
                Err(Unsent::Full(data)) => {
                    self.meshes.push((kind, data));
                    return None;
                }
                Err(Unsent::Refused(why)) => return Some(Err(why)),
            }
        }
        Some(Emotes::make(
            parent,
            &self.emote,
            std::mem::take(&mut self.made),
            (stock, set, candle),
        ))
    }
}

/// The meshes of the kinds of moving part `emote` hangs about its
/// houses, besides the mote, which is a sphere of radius 1. None for
/// banners, which are built with the houses.
pub fn meshes(
    emote: &Emote,
    set: &AttireSettings,
) -> Option<Vec<(Kind, Mb)>> {
    Some(match emote {
        Emote::Smoke(_) | Emote::Fireflies(_) => Vec::new(),
        Emote::Lanterns(_) => vec![(Kind::Lantern, lantern(set))],
        Emote::Petals(_) => vec![(Kind::Petal(0), petal_mesh())],
        Emote::Pinwheels(pinwheels) => vec![
            (
                Kind::Pinwheel,
                pinwheel(
                    set,
                    pinwheels.colours.each_ref().map(|c| linear(c)),
                ),
            ),
            (Kind::Stick, stick(set)),
        ],
        Emote::Banners(_) | Emote::Clip(_) => return None,
    })
}

impl Kind {
    /// The kind whose mesh of [`meshes`] this kind is drawn with, or
    /// none for the mote.
    pub fn mesh(self) -> Option<Kind> {
        match self {
            Kind::Puff | Kind::Firefly => None,
            Kind::Petal(_) => Some(Kind::Petal(0)),
            kind => Some(kind),
        }
    }
}

/// Each kind of moving part `emote` draws, with the look of a
/// material of its own and the colour it glows after dark at its
/// strongest, or none for the shared matte.
pub fn looks(
    emote: &Emote,
    set: &AttireSettings,
) -> Vec<(Kind, Option<(Pbr, Vec3)>)> {
    match emote {
        Emote::Smoke(smoke) => {
            let mut colour = rgba(linear(&smoke.colour));
            colour.3 = 0.6;
            vec![(
                Kind::Puff,
                Some((
                    Pbr {
                        alpha_mode: AlphaMode::Blend,
                        roughness: 1.0,
                        ..Pbr::colored(colour)
                    },
                    Vec3::ZERO,
                )),
            )]
        }
        Emote::Lanterns(lanterns) => vec![(
            Kind::Lantern,
            Some((
                Pbr {
                    roughness: 0.8,
                    ..Pbr::colored(rgba(linear(&lanterns.paper)))
                },
                linear(&lanterns.light) * set.lantern_glow,
            )),
        )],
        Emote::Petals(petals) => petals
            .colours
            .iter()
            .enumerate()
            .map(|(k, colour)| {
                (
                    Kind::Petal(k),
                    Some((
                        Pbr {
                            roughness: 0.9,
                            double_sided: true,
                            ..Pbr::colored(rgba(linear(colour)))
                        },
                        Vec3::ZERO,
                    )),
                )
            })
            .collect(),
        Emote::Fireflies(fireflies) => {
            let colour = linear(&fireflies.colour);
            vec![(
                Kind::Firefly,
                Some((
                    Pbr::colored(rgba(colour)),
                    colour * fireflies.glow,
                )),
            )]
        }
        Emote::Pinwheels(_) => {
            vec![(Kind::Pinwheel, None), (Kind::Stick, None)]
        }
        Emote::Banners(_) | Emote::Clip(_) => Vec::new(),
    }
}

/// How strongly what `emote` hangs about its houses glows, with
/// candles burning `candle` strong: fireflies a little by day too.
pub fn strength(
    emote: &Emote,
    candle: f32,
    set: &AttireSettings,
) -> f32 {
    match emote {
        Emote::Fireflies(_) => candle.max(set.firefly_day),
        _ => candle,
    }
}

impl Emotes {
    fn make(
        parent: &Entity,
        emote: &Emote,
        made: Vec<(Kind, Mesh)>,
        (stock, set, candle): (&Stock, &AttireSettings, f32),
    ) -> Result<Self, String> {
        let mesh_of = |kind: Kind| {
            made.iter()
                .find(|(k, _)| *k == kind)
                .map(|(_, mesh)| mesh)
        };
        let mote = Mesh::shape(Shape::Sphere(1.0))?;
        // Each kind with its mesh and either a material of its own,
        // with the colour it glows after dark, or the shared matte.
        let mut kinds: Vec<Drawn> = Vec::new();
        for (kind, own) in looks(emote, set) {
            let mesh = match kind.mesh() {
                Some(made) => mesh_of(made),
                None => Some(&mote),
            };
            if let Some(mesh) = mesh {
                kinds.push((kind, mesh, own));
            }
        }
        let mut looks = Vec::new();
        let mut sets = Vec::new();
        for (kind, mesh, own) in kinds {
            let look = match own {
                Some((pbr, glows)) => {
                    let material = Material::make(pbr)?;
                    looks.push((material, pbr, glows));
                    Some(looks.len() - 1)
                }
                None => None,
            };
            let material = match look {
                Some(i) => &looks[i].0,
                None => &stock.matte,
            };
            sets.push(Set {
                kind,
                copies: Instances::make(
                    Some(parent),
                    mesh,
                    material,
                    0,
                )?,
                shown: true,
            });
        }
        let mut meshes = made
            .into_iter()
            .map(|(_, mesh)| mesh)
            .collect::<Vec<_>>();
        meshes.push(mote);
        let mut emotes = Self {
            emote: emote.clone(),
            sets,
            looks,
            _meshes: meshes,
            laid: Vec::new(),
            candle: -1.0,
        };
        emotes.light(candle, set);
        Ok(emotes)
    }

    /// Lights what glows after dark as `candle` says, fireflies a
    /// little by day too.
    pub fn light(&mut self, candle: f32, set: &AttireSettings) {
        if candle == self.candle {
            return;
        }
        self.candle = candle;
        let strength = strength(&self.emote, candle, set);
        for (material, pbr, glows) in &self.looks {
            if *glows != Vec3::ZERO {
                material.set(Pbr {
                    emissive: rgba(*glows * strength),
                    ..*pbr
                });
            }
        }
    }

    /// Moves the parts about `perches` to clock time `now`, or shows
    /// none while `shown` is false. A part that stands still is laid
    /// again only when the houses change.
    pub fn stir(
        &mut self,
        perches: &[Perch],
        (now, shown): (f32, bool),
        set: &AttireSettings,
        upload: &mut Upload,
    ) {
        let houses_moved = self.laid != perches;
        let mut laid = true;
        for part in &mut self.sets {
            let still = part.kind == Kind::Stick;
            if still && !houses_moved && part.shown == shown {
                continue;
            }
            if !shown && !part.shown {
                continue;
            }
            let placed = match shown {
                true => perches
                    .iter()
                    .flat_map(|perch| {
                        parts(&self.emote, part.kind, perch, now, set)
                            .into_iter()
                            .map(|at| perch.frame.then(at).scene())
                    })
                    .collect::<Vec<_>>(),
                false => Vec::new(),
            };
            if upload.copies(&part.copies, &copies(&placed)) {
                part.shown = shown;
            } else {
                laid = false;
            }
        }
        if laid {
            self.laid = perches.to_vec();
        }
    }
}

/// A paper lantern hung from its string's top at the origin, in
/// white, for its material to colour.
fn lantern(set: &AttireSettings) -> Mb {
    let (r, h, string) =
        (set.lantern_radius, set.lantern_height, set.lantern_string);
    let mut mb = Mb::new();
    let cord = Vec3::splat(0.08);
    mb.tube(
        Vec3::ZERO,
        -Vec3::Y * string,
        0.006,
        0.006,
        3,
        cord,
        false,
    );
    let middle = -Vec3::Y * (string + h * 0.5);
    let paper = [
        (r * 0.45, -h * 0.5),
        (r * 0.85, -h * 0.38),
        (r, -h * 0.15),
        (r, h * 0.15),
        (r * 0.85, h * 0.38),
        (r * 0.45, h * 0.5),
    ];
    // Ribs darker than the paper between them.
    mb.lathe(middle, &paper, 10, |_, s| match s % 2 {
        0 => Vec3::ONE,
        _ => Vec3::splat(0.9),
    });
    for end in [-0.5f32, 0.5] {
        mb.lathe(
            middle + Vec3::Y * h * end,
            &[
                (r * 0.5, 0.0),
                (r * 0.5, 0.03 * end.signum()),
                (0.0, 0.03 * end.signum()),
            ],
            8,
            |_, _| cord,
        );
    }
    mb
}

/// A petal of size 1: a flat oval, white for its material to colour.
fn petal_mesh() -> Mb {
    let mut mb = Mb::new();
    mb.rbox(
        Vec3::ZERO,
        Vec3::new(0.5, 0.04, 0.32),
        0.03,
        Quat::IDENTITY,
        Vec3::ONE,
        0.0,
    );
    mb
}

/// A pinwheel's stick, its foot at the origin.
fn stick(set: &AttireSettings) -> Mb {
    let mut mb = Mb::new();
    mb.tube(
        Vec3::ZERO,
        Vec3::new(0.0, set.pinwheel_stick, -0.02),
        0.008,
        0.008,
        4,
        Vec3::splat(0.35),
        false,
    );
    mb
}

/// A pinwheel facing +Z about its hub at the origin: four sails in
/// `colours` by turns, and the hub.
fn pinwheel(set: &AttireSettings, colours: [Vec3; 2]) -> Mb {
    let r = set.pinwheel_radius;
    let mut mb = Mb::new();
    for k in 0..4 {
        let a = k as f32 * FRAC_PI_2;
        let out = Vec3::new(a.cos(), a.sin(), 0.0);
        let side = Vec3::new(-a.sin(), a.cos(), 0.0);
        // A sail from the hub to its tip, its trailing edge tipped
        // back.
        mb.rbox(
            out * r * 0.5 + side * r * 0.18,
            Vec3::new(r * 0.5, r * 0.2, 0.006),
            0.003,
            Quat::from_rotation_z(a) * Quat::from_rotation_x(0.35),
            colours[k % 2],
            0.0,
        );
    }
    mb.tube(
        -Vec3::Z * 0.01,
        Vec3::Z * 0.03,
        r * 0.12,
        r * 0.12,
        6,
        Vec3::splat(0.9),
        true,
    );
    mb
}

#[cfg(test)]
mod tests;
