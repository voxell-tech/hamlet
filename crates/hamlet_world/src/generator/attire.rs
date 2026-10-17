//! What a hamlet's houses wear (docs/looks.md): a skin's colours in
//! place of the palette's, a mesh in place of their walls, and an
//! emote about every one of them.

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use bevy::camera::visibility::VisibilitySystems;
use bevy::ecs::system::RunSystemOnce;
use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::light::NotShadowCaster;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_model::wardrobe::{Emote, Skin, rgb};
use hamlet_vfx::flag::{FlagMaterial, FlagSettings, Waving};

use super::Kit;
use super::building::{HouseRecipe, build_house};
use super::house::{self, Bulk, Fit, Spots};
use super::land::Land;
use super::loading::fit_out;
use super::society::{AgentId, TownId};
use crate::keeper::KState;
use crate::meshes::Mb;
use crate::palette::Palette;
use crate::sky::SkySettings;
use crate::{Clock, Sky, Step, dress};

/// The agent a house raised on its own takes its looks from.
const RAISED: AgentId = AgentId(1 << 21);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Attires>()
        .init_resource::<AttireSettings>()
        .add_observer(deck)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                puff.run_if(any_with_component::<Puff>),
                sway.run_if(any_with_component::<Swing>),
                fall.run_if(any_with_component::<Petal>),
                flit.run_if(any_with_component::<Firefly>),
                spin.run_if(any_with_component::<Pinwheel>),
                glow.run_if(resource_changed::<Sky>),
            )
                .in_set(Step::Scene),
        )
        .add_systems(
            PostUpdate,
            keep_going
                .run_if(
                    any_with_component::<Puff>
                        .or_else(any_with_component::<Swing>)
                        .or_else(any_with_component::<Petal>)
                        .or_else(any_with_component::<Firefly>)
                        .or_else(any_with_component::<Pinwheel>),
                )
                .after(
                    VisibilitySystems::MarkNewlyHiddenEntitiesInvisible,
                ),
        );
}

/// How the emotes of hamlets move and show.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
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
    /// The most pinwheels on one house, a pinwheel's radius and the
    /// stick it stands on above the eave.
    pub pinwheels: u32,
    pub pinwheel_radius: f32,
    pub pinwheel_stick: f32,
    /// The share of their glow fireflies keep by day, so that they
    /// are seen at all.
    pub firefly_day: f32,
}

/// What the houses of each town wear, by town.
#[derive(Resource, Default)]
pub struct Attires {
    /// Each town's attire and the count it was put on at.
    worn: HashMap<TownId, (u64, Attire)>,
    made: u64,
}

impl Attires {
    pub fn get(&self, town: TownId) -> Option<&Attire> {
        self.worn.get(&town).map(|(_, attire)| attire)
    }

    /// True when `town` wears `attire`, or wears nothing and it is
    /// bare.
    pub fn wears(&self, town: TownId, attire: &Attire) -> bool {
        match self.get(town) {
            Some(worn) => worn == attire,
            None => attire.is_bare(),
        }
    }

    /// Puts `attire` on the houses of `town`. A bare one takes off
    /// what they wear.
    pub fn wear(&mut self, town: TownId, attire: Attire) {
        if self.wears(town, &attire) {
            return;
        }
        if attire.is_bare() {
            self.worn.remove(&town);
            return;
        }
        self.made += 1;
        self.worn.insert(town, (self.made, attire));
    }

    /// The towns that wear something.
    pub fn towns(&self) -> impl Iterator<Item = TownId> + '_ {
        self.worn.keys().copied()
    }

    /// A number that changes each time what `town` wears does; 0
    /// while it wears nothing.
    pub(super) fn version(&self, town: TownId) -> u64 {
        self.worn.get(&town).map_or(0, |(made, _)| *made)
    }
}

/// What a hamlet's houses wear.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attire {
    pub skin: Option<Skin>,
    pub mesh: Option<Shell>,
    /// Smoke, lanterns, banners, petals, fireflies or pinwheels.
    pub emote: Option<Emote>,
}

impl Attire {
    pub fn is_bare(&self) -> bool {
        self.skin.is_none()
            && self.mesh.is_none()
            && self.emote.is_none()
    }

    /// The palette its houses are built in, when its skin changes
    /// that of the world, `pal`.
    pub fn palette(&self, pal: &Palette) -> Option<Palette> {
        self.skin.as_ref().map(|skin| skinned(pal, skin))
    }

    /// What one of its houses is built wearing.
    pub fn fit(&self) -> Fit {
        let banner = match &self.emote {
            Some(Emote::Banners(banners)) => Some((
                banners.colours.each_ref().map(|hex| linear(hex)),
                banners.long.max(0.1),
            )),
            _ => None,
        };
        Fit {
            shell: self.mesh.as_ref().map(|shell| shell.spots),
            banner,
        }
    }
}

/// A hamlet's mesh, loaded: its walls and panes, its spots and its
/// bulk in the frame it is authored in.
#[derive(Clone, Debug, PartialEq)]
pub struct Shell {
    pub walls: Handle<Mesh>,
    pub panes: Option<Handle<Mesh>>,
    pub spots: Spots,
    pub bulk: Bulk,
}

impl Shell {
    /// The mesh of `gltf`, once it is loaded and holds `walls`. The
    /// spots it does not mark are put where docs/looks.md says.
    pub fn of(
        gltf: &Handle<Gltf>,
        gltfs: &Assets<Gltf>,
        gltf_meshes: &Assets<GltfMesh>,
        nodes: &Assets<GltfNode>,
        meshes: &Assets<Mesh>,
    ) -> Option<Shell> {
        let walls = dress::part(gltf, "walls", gltfs, gltf_meshes)?;
        let bulk = bulk(meshes.get(&walls)?)?;
        let panes = dress::part(gltf, "panes", gltfs, gltf_meshes);
        let spot = |name: &str| {
            dress::anchor(gltf, name, gltfs, nodes)
                .map(|at| at.translation)
        };
        let top = Vec3::new(0.0, bulk.max.y, 0.0);
        let door =
            spot("door").unwrap_or(Vec3::new(0.0, 0.0, bulk.max.z));
        let lamp =
            spot("lamp").unwrap_or(door + Vec3::new(0.6, 1.0, 0.3));
        let eaves = spot("eaves").unwrap_or(Vec3::new(
            0.0,
            lamp.y + 0.5,
            bulk.max.z,
        ));
        Some(Shell {
            walls,
            panes,
            spots: Spots {
                door,
                lamp,
                banner: spot("banner").unwrap_or(top),
                chimney: spot("chimney").unwrap_or(top),
                eaves,
            },
            bulk,
        })
    }
}

/// The bounds and the mean colour of `mesh`.
fn bulk(mesh: &Mesh) -> Option<Bulk> {
    let Some(VertexAttributeValues::Float32x3(at)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let (min, max) = at.iter().map(|p| Vec3::from(*p)).fold(
        (Vec3::INFINITY, Vec3::NEG_INFINITY),
        |(min, max), p| (min.min(p), max.max(p)),
    );
    if at.is_empty() {
        return None;
    }
    let tint = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(colours))
            if !colours.is_empty() =>
        {
            colours
                .iter()
                .map(|c| Vec3::new(c[0], c[1], c[2]))
                .sum::<Vec3>()
                / colours.len() as f32
        }
        _ => Vec3::splat(0.8),
    };
    Some(Bulk { min, max, tint })
}

/// `pal` with the colours `skin` sets for a hamlet in place of its
/// own: `roof`, `drift` and `banner` in every town's style, and
/// `livery` for every door and shutter.
pub fn skinned(pal: &Palette, skin: &Skin) -> Palette {
    let mut pal = pal.clone();
    for slot in skin.colours.keys() {
        let Some(c) = dress::colour(skin, slot) else {
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
    let [r, g, b] = rgb(hex).unwrap_or([1.0; 3]);
    LinearRgba::from(Srgba::rgb(r, g, b)).to_vec3()
}

/// Builds one house wearing `attire` at once, and spawns it at `at`
/// with its windows, door lantern, banner and emote, as the world
/// draws a house of a hamlet that wears it. `hall` makes it the
/// leader's hall. It needs the generator's plugin. Returns the
/// house.
pub fn raise(
    world: &mut World,
    attire: &Attire,
    hall: bool,
    at: Transform,
) -> Entity {
    let land = world.resource::<Land>().clone();
    let pal = attire.palette(&land.pal);
    let yard = house::yard_of(&land, RAISED, hall, 0.0, 0);
    let recipe = HouseRecipe {
        agent: RAISED,
        site: 0,
        hall,
        leader: hall,
        remains: None,
        wings: 0,
        yard,
    };
    let pal = pal.as_ref().unwrap_or(&land.pal);
    let mut built =
        Some(build_house(&land, pal, recipe, attire.fit()));
    let attire = attire.clone();
    let raise = move |mut commands: Commands,
                      kit: Res<Kit>,
                      sky: Res<SkySettings>,
                      clock: Res<Clock>,
                      mut meshes: ResMut<Assets<Mesh>>,
                      mut flags: Option<(
        ResMut<Waving>,
        ResMut<Assets<FlagMaterial>>,
        Res<FlagSettings>,
    )>| {
        let built = built.take().expect("a house is raised once");
        let house = commands
            .spawn((
                Mesh3d(meshes.add(built.body)),
                MeshMaterial3d(kit.matte.clone()),
                at,
                Visibility::Inherited,
            ))
            .id();
        if let Some(fittings) = built.fittings {
            fit_out(
                &mut commands,
                house,
                fittings,
                &attire,
                &kit,
                &sky,
                KState::Rest,
                &mut meshes,
                flags.as_mut().map(|(waving, cloth, set)| {
                    (&mut **waving, &mut **cloth, &**set)
                }),
                clock.t,
            );
        }
        house
    };
    world
        .run_system_once(raise)
        .expect("raising a house needs the generator's plugin")
}

/// The emote a house wears, and where on it smoke leaves and
/// lanterns hang.
#[derive(Component, Clone, Debug)]
pub(super) struct Emoting {
    pub(super) emote: Emote,
    pub(super) chimney: Vec3,
    pub(super) eaves: [Vec3; 2],
}

/// The meshes every emote shares, and a material for each colour
/// worn.
#[derive(Resource)]
pub(super) struct EmoteKit {
    mote: Handle<Mesh>,
    lantern: Handle<Mesh>,
    /// A petal: a small flat oval.
    petal: Handle<Mesh>,
    smoke: Vec<(Vec3, Handle<StandardMaterial>)>,
    /// By paper and light.
    paper: Vec<([Vec3; 2], Handle<StandardMaterial>)>,
    /// By colour and glow, the glow after dark.
    fireflies: Vec<((Vec3, f32), Handle<StandardMaterial>)>,
    /// By colours: the vertex colours are the sails'.
    pinwheels: Vec<([Vec3; 2], Handle<Mesh>)>,
    stick: Handle<Mesh>,
    /// Plain colours, for petals.
    plain: Vec<(Vec3, Handle<StandardMaterial>)>,
}

/// A puff of smoke over a chimney: where it leaves, how high it
/// goes and how big it is, the seconds it lives and how far into
/// its life it was at naught.
#[derive(Component, Clone, Copy)]
struct Puff {
    from: Vec3,
    rise: f32,
    size: f32,
    life: f32,
    phase: f32,
}

impl Puff {
    fn at(&self, t: f32) -> Transform {
        let u = (t / self.life + self.phase).fract();
        let drift = Vec3::new(0.25, 0.0, 0.1) * u * u;
        Transform::from_translation(
            self.from + Vec3::Y * self.rise * u + drift,
        )
        .with_scale(Vec3::splat(
            self.size * (0.5 + 0.8 * u) * (1.0 - u * u * u),
        ))
    }
}

/// A lantern swinging from its string: radians either way, and a
/// time of its own.
#[derive(Component, Clone, Copy)]
struct Swing {
    sway: f32,
    phase: f32,
}

pub(super) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    set: Res<AttireSettings>,
) {
    commands.insert_resource(EmoteKit {
        mote: meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),
        lantern: meshes.add(lantern(&set).build()),
        petal: meshes.add(petal()),
        smoke: Vec::new(),
        paper: Vec::new(),
        fireflies: Vec::new(),
        pinwheels: Vec::new(),
        stick: meshes.add(stick(&set)),
        plain: Vec::new(),
    });
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
    mb.lathe(middle, &paper, 10, |_, s| {
        if s % 2 == 0 {
            Vec3::ONE
        } else {
            Vec3::splat(0.9)
        }
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
fn petal() -> Mesh {
    let mut mb = Mb::new();
    mb.rbox(
        Vec3::ZERO,
        Vec3::new(0.5, 0.04, 0.32),
        0.03,
        Quat::IDENTITY,
        Vec3::ONE,
        0.0,
    );
    mb.build()
}

/// A pinwheel's stick, its foot at the origin.
fn stick(set: &AttireSettings) -> Mesh {
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
    mb.build()
}

/// A pinwheel facing +Z about its hub at the origin: four sails in
/// `colours` by turns, and the hub.
fn pinwheel(set: &AttireSettings, colours: [Vec3; 2]) -> Mesh {
    let r = set.pinwheel_radius;
    let mut mb = Mb::new();
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2;
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
    mb.build()
}

impl EmoteKit {
    /// A matte material of `colour`, shared.
    fn plain(
        &mut self,
        colour: Vec3,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        if let Some((_, made)) =
            self.plain.iter().find(|(c, _)| *c == colour)
        {
            return made.clone();
        }
        let made = materials.add(StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::from_vec3(
                colour,
            )),
            perceptual_roughness: 0.9,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        self.plain.push((colour, made.clone()));
        made
    }

    /// A firefly's material of `colour`, lit by `glow` after dark.
    fn firefly(
        &mut self,
        (colour, glow): (Vec3, f32),
        candle: f32,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        if let Some((_, made)) = self
            .fireflies
            .iter()
            .find(|((c, g), _)| *c == colour && *g == glow)
        {
            return made.clone();
        }
        let made = materials.add(StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::from_vec3(
                colour,
            )),
            emissive: LinearRgba::from_vec3(colour * glow * candle),
            unlit: false,
            ..default()
        });
        self.fireflies.push(((colour, glow), made.clone()));
        made
    }

    /// The pinwheel of `colours`, shared.
    fn pinwheel(
        &mut self,
        colours: [Vec3; 2],
        set: &AttireSettings,
        meshes: &mut Assets<Mesh>,
    ) -> Handle<Mesh> {
        if let Some((_, made)) =
            self.pinwheels.iter().find(|(c, _)| *c == colours)
        {
            return made.clone();
        }
        let made = meshes.add(pinwheel(set, colours));
        self.pinwheels.push((colours, made.clone()));
        made
    }

    fn smoke(
        &mut self,
        colour: Vec3,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        if let Some((_, made)) =
            self.smoke.iter().find(|(c, _)| *c == colour)
        {
            return made.clone();
        }
        let made = materials.add(StandardMaterial {
            base_color: Color::LinearRgba(
                LinearRgba::from_vec3(colour).with_alpha(0.6),
            ),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        });
        self.smoke.push((colour, made.clone()));
        made
    }

    fn paper(
        &mut self,
        colours: [Vec3; 2],
        glow: Vec3,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        if let Some((_, made)) =
            self.paper.iter().find(|(c, _)| *c == colours)
        {
            return made.clone();
        }
        let made = materials.add(StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::from_vec3(
                colours[0],
            )),
            emissive: LinearRgba::from_vec3(glow),
            perceptual_roughness: 0.8,
            ..default()
        });
        self.paper.push((colours, made.clone()));
        made
    }
}

/// Hangs a house's emote about it: puffs over its chimney, or
/// lanterns under its eave. Banners are built with the house.
fn deck(
    add: On<Add, Emoting>,
    mut commands: Commands,
    houses: Query<&Emoting>,
    kit: Option<ResMut<EmoteKit>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    set: Res<AttireSettings>,
    sky: Option<Res<Sky>>,
    clock: Option<Res<Clock>>,
) {
    let (Ok(emoting), Some(mut kit)) = (houses.get(add.entity), kit)
    else {
        return;
    };
    let house = add.entity;
    let now = clock.map_or(0.0, |clock| clock.t);
    match &emoting.emote {
        Emote::Smoke(smoke) => {
            let material =
                kit.smoke(linear(&smoke.colour), &mut materials);
            let rate = smoke.rate.max(0.05);
            let n = ((rate * set.puff_life).ceil() as u32)
                .clamp(1, set.puffs.max(1));
            for i in 0..n {
                let puff = Puff {
                    from: emoting.chimney,
                    rise: smoke.rise,
                    size: smoke.size,
                    life: n as f32 / rate,
                    phase: i as f32 / n as f32,
                };
                commands.spawn((
                    puff,
                    Mesh3d(kit.mote.clone()),
                    MeshMaterial3d(material.clone()),
                    puff.at(now),
                    NotShadowCaster,
                    Visibility::Inherited,
                    ChildOf(house),
                ));
            }
        }
        Emote::Lanterns(lanterns) => {
            let colours =
                [linear(&lanterns.paper), linear(&lanterns.light)];
            let candle = sky.map_or(0.0, |sky| sky.look.candle);
            let material = kit.paper(
                colours,
                colours[1] * set.lantern_glow * candle,
                &mut materials,
            );
            let n = lanterns.count.min(set.lanterns);
            let [a, b] = emoting.eaves;
            for i in 0..n {
                let u = (i as f32 + 0.5) / n as f32;
                commands.spawn((
                    Swing {
                        sway: lanterns.sway.to_radians(),
                        phase: i as f32 * 1.7,
                    },
                    Mesh3d(kit.lantern.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(a.lerp(b, u)),
                    NotShadowCaster,
                    Visibility::Inherited,
                    ChildOf(house),
                ));
            }
        }
        Emote::Petals(petals) => {
            let [a, b] = emoting.eaves;
            let n = petals.count.min(set.petals);
            for i in 0..n {
                let colour = linear(&petals.colours[i as usize % 2]);
                let material = kit.plain(colour, &mut materials);
                let u = hash(i, 1);
                let along = a.lerp(b, u);
                let petal = Petal {
                    from: along
                        + Vec3::new(
                            0.0,
                            0.2,
                            0.15 + 0.5 * hash(i, 2),
                        ),
                    size: petals.size,
                    life: petals.fall.max(0.5),
                    phase: hash(i, 3),
                    swirl: 0.3 + 0.4 * hash(i, 4),
                };
                commands.spawn((
                    petal,
                    Mesh3d(kit.petal.clone()),
                    MeshMaterial3d(material),
                    petal.at(now),
                    NotShadowCaster,
                    Visibility::Inherited,
                    ChildOf(house),
                ));
            }
        }
        Emote::Fireflies(fireflies) => {
            let candle = sky.map_or(0.0, |sky| sky.look.candle);
            let material = kit.firefly(
                (linear(&fireflies.colour), fireflies.glow),
                candle.max(set.firefly_day),
                &mut materials,
            );
            let [a, b] = emoting.eaves;
            // Out past the walls, which reach about as far as the
            // eave.
            let walls = a.x.abs().max(b.x.abs()).max(a.z.abs());
            let n = fireflies.count.min(set.fireflies);
            for i in 0..n {
                let fly = Firefly {
                    near: walls + 0.25,
                    reach: set.firefly_reach
                        * (0.4 + 0.6 * hash(i, 5)),
                    high: 0.3 + a.y * 0.8 * hash(i, 6),
                    speed: 0.15 + 0.2 * hash(i, 7),
                    phase: std::f32::consts::TAU * hash(i, 8),
                };
                commands.spawn((
                    fly,
                    Mesh3d(kit.mote.clone()),
                    MeshMaterial3d(material.clone()),
                    fly.at(now),
                    NotShadowCaster,
                    Visibility::Inherited,
                    ChildOf(house),
                ));
            }
        }
        Emote::Pinwheels(pinwheels) => {
            let colours =
                pinwheels.colours.each_ref().map(|c| linear(c));
            let mesh = kit.pinwheel(colours, &set, &mut meshes);
            // White: the vertex colours are the colour.
            let material = kit.plain(Vec3::ONE, &mut materials);
            let [a, b] = emoting.eaves;
            let n = pinwheels.count.min(set.pinwheels).max(1);
            for i in 0..n {
                let u = (i as f32 + 0.5) / n as f32;
                let foot = a.lerp(b, u) + Vec3::Z * 0.05;
                commands.spawn((
                    Mesh3d(kit.stick.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(foot),
                    NotShadowCaster,
                    Visibility::Inherited,
                    ChildOf(house),
                ));
                commands.spawn((
                    Pinwheel {
                        speed: pinwheels.speed,
                        phase: i as f32 * 0.9,
                    },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(
                        foot + Vec3::Y * set.pinwheel_stick,
                    ),
                    NotShadowCaster,
                    Visibility::Inherited,
                    ChildOf(house),
                ));
            }
        }
        Emote::Banners(_) | Emote::Clip(_) => {}
    }
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

/// A petal drifting down from under the eave: where it leaves, its
/// size, the seconds it takes to fall, how far into its fall it was
/// at naught and how widely it swings.
#[derive(Component, Clone, Copy)]
struct Petal {
    from: Vec3,
    size: f32,
    life: f32,
    phase: f32,
    swirl: f32,
}

impl Petal {
    fn at(&self, t: f32) -> Transform {
        let u = (t / self.life + self.phase).fract();
        let swing = (u * 9.0 + self.phase * 6.0).sin();
        let at = self.from
            + Vec3::new(
                swing * self.swirl,
                -(self.from.y - 0.02) * u,
                u * 0.6,
            );
        Transform::from_translation(at)
            .with_rotation(
                Quat::from_rotation_z(swing * 0.8)
                    * Quat::from_rotation_y(
                        u * 7.0 + self.phase * 3.0,
                    ),
            )
            // Born and gone small, never a pop.
            .with_scale(Vec3::splat(
                self.size
                    * (u * 12.0).min(1.0)
                    * ((1.0 - u) * 12.0).min(1.0),
            ))
    }
}

/// A firefly wandering about its house: how far out and how high,
/// its speed and its own time.
#[derive(Component, Clone, Copy)]
struct Firefly {
    /// The nearest it comes to the house's middle, and how much
    /// further out it wanders.
    near: f32,
    reach: f32,
    high: f32,
    speed: f32,
    phase: f32,
}

/// A firefly's size.
const FIREFLY: f32 = 0.07;

impl Firefly {
    fn at(&self, t: f32) -> Transform {
        let a = t * self.speed + self.phase;
        // A loop about the house that wanders in and out and up and
        // down as it goes.
        let r =
            self.near + self.reach * (0.5 + 0.5 * (a * 2.3).sin());
        let at = Vec3::new(
            r * a.cos(),
            self.high + 0.25 * (a * 3.1).sin(),
            r * a.sin(),
        );
        Transform::from_translation(at).with_scale(Vec3::splat(
            FIREFLY
                * (0.7 + 0.3 * (t * 5.0 + self.phase).sin().abs()),
        ))
    }
}

/// A pinwheel turning: turns a second and its own start.
#[derive(Component, Clone, Copy)]
struct Pinwheel {
    speed: f32,
    phase: f32,
}

impl Pinwheel {
    /// Its turn at `t`, clockwise as seen from the front.
    fn turn(&self, t: f32) -> Quat {
        Quat::from_rotation_z(
            -(std::f32::consts::TAU * self.speed * t + self.phase),
        )
    }
}

/// Moves the puffs that are seen along their way up.
fn puff(
    clock: Res<Clock>,
    mut puffs: Query<(&Puff, &mut Transform, &ViewVisibility)>,
    mut told: Local<Option<f32>>,
) {
    if told.replace(clock.t) == Some(clock.t) {
        return;
    }
    for (puff, mut at, seen) in &mut puffs {
        if seen.get() {
            *at = puff.at(clock.t);
        }
    }
}

/// Swings the lanterns that are seen.
fn sway(
    clock: Res<Clock>,
    set: Res<AttireSettings>,
    mut lanterns: Query<(&Swing, &mut Transform, &ViewVisibility)>,
    mut told: Local<Option<f32>>,
) {
    if told.replace(clock.t) == Some(clock.t) {
        return;
    }
    for (swing, mut at, seen) in &mut lanterns {
        if seen.get() {
            let a = swing.sway
                * (clock.t * set.sway_speed + swing.phase).sin();
            at.rotation = Quat::from_rotation_z(a);
        }
    }
}

/// Lets the petals that are seen fall.
fn fall(
    clock: Res<Clock>,
    mut petals: Query<(&Petal, &mut Transform, &ViewVisibility)>,
    mut told: Local<Option<f32>>,
) {
    if told.replace(clock.t) == Some(clock.t) {
        return;
    }
    for (petal, mut at, seen) in &mut petals {
        if seen.get() {
            *at = petal.at(clock.t);
        }
    }
}

/// Lets the fireflies that are seen wander.
fn flit(
    clock: Res<Clock>,
    mut flies: Query<(&Firefly, &mut Transform, &ViewVisibility)>,
    mut told: Local<Option<f32>>,
) {
    if told.replace(clock.t) == Some(clock.t) {
        return;
    }
    for (fly, mut at, seen) in &mut flies {
        if seen.get() {
            *at = fly.at(clock.t);
        }
    }
}

/// Turns the pinwheels that are seen about their hubs.
fn spin(
    clock: Res<Clock>,
    mut wheels: Query<(&Pinwheel, &mut Transform, &ViewVisibility)>,
    mut told: Local<Option<f32>>,
) {
    if told.replace(clock.t) == Some(clock.t) {
        return;
    }
    for (wheel, mut at, seen) in &mut wheels {
        if seen.get() {
            at.rotation = wheel.turn(clock.t);
        }
    }
}

/// Lights the lanterns' paper after dark, as the windows burn.
fn glow(
    sky: Res<Sky>,
    set: Res<AttireSettings>,
    kit: Option<Res<EmoteKit>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(kit) = kit else {
        return;
    };
    for ([_, light], handle) in &kit.paper {
        let glow = LinearRgba::from_vec3(
            *light * set.lantern_glow * sky.look.candle,
        );
        if let Some(mut paper) = materials.get_mut(handle)
            && paper.emissive != glow
        {
            paper.emissive = glow;
        }
    }
    let candle = sky.look.candle.max(set.firefly_day);
    for ((colour, strength), handle) in &kit.fireflies {
        let glow =
            LinearRgba::from_vec3(*colour * *strength * candle);
        if let Some(mut fly) = materials.get_mut(handle)
            && fly.emissive != glow
        {
            fly.emissive = glow;
        }
    }
}

/// Keeps frames coming while smoke or a lantern moves in view.
fn keep_going(
    clock: Res<Clock>,
    moving: Query<
        &ViewVisibility,
        Or<(
            With<Puff>,
            With<Swing>,
            With<Petal>,
            With<Firefly>,
            With<Pinwheel>,
        )>,
    >,
    mut told: Local<Option<f32>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if told.replace(clock.t) == Some(clock.t) {
        return;
    }
    if moving.iter().any(|seen| seen.get()) {
        redraw.write(RequestRedraw);
    }
}
