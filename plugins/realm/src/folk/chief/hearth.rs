//! A chief given its ground: the claim on it and the ground held, the
//! place and the mascot made, their meshes handed over a few a frame,
//! and the mascot moved and lit.

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_sdk::scene::{
    Entity, Material, Mesh, MeshData, Pbr, Shape,
};

use crate::build::frame::Frame;
use crate::build::mesh::{Mb, smoothstep};
use crate::build::stock::{glow, rgba};
use crate::build::upload::{Unsent, Upload};
use crate::folk::chief::ChiefSettings;
use crate::folk::chief::look::Look;
use crate::folk::chief::mascot::{Rig, parts, wick};
use crate::folk::chief::place::{self, Built};
use crate::folk::chief::wickstep::{
    Charm, Mascot, Phase, Whereabouts, shapes,
};
use crate::places::lantern::lantern_glass;
use crate::realm::Ctx;
use crate::society::{Claim, Place, Society};
use crate::view::pick::Thing;
use crate::world::flora;
use crate::world::land::Land;
use crate::world::palette::Palette;

/// A light of its own colour that no shading dims.
fn lit(colour: Vec3, strength: f32) -> Pbr {
    Pbr {
        unlit: true,
        fog: false,
        ..Pbr::colored(rgba(colour * strength))
    }
}

/// A flat band of radius 1, its inner edge at 0.9, lying on the
/// ground.
fn annulus() -> Mb {
    let mut mb = Mb::new();
    let sides = 32;
    let ring = (0..sides)
        .map(|i| {
            let about = Quat::from_rotation_y(
                i as f32 / sides as f32 * std::f32::consts::TAU,
            );
            (
                mb.v(about * Vec3::X * 0.9, Vec3::Y, Vec3::ONE),
                mb.v(about * Vec3::X, Vec3::Y, Vec3::ONE),
            )
        })
        .collect::<Vec<_>>();
    for i in 0..sides {
        let (a, b) = (ring[i], ring[(i + 1) % sides]);
        mb.quad(a.0, a.1, b.1, b.0);
    }
    mb
}

/// What a mesh handed over is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Body,
    Panes,
    Ring,
    Glass,
    Tree(usize),
    /// A part of the mascot, in the order of `Parts::each`.
    Part(usize),
    Wick,
    /// A shape of the circle of runes.
    Rune(usize),
}

/// The meshes handed over, each kept so that it stays.
#[derive(Default)]
struct Meshes {
    place: Vec<Mesh>,
    parts: [Option<Mesh>; 8],
    wick: Option<Mesh>,
    runes: [Option<Mesh>; 2],
    /// True once the mascot has its meshes, and the runes theirs.
    dressed: bool,
    shaped: bool,
}

/// A chief's place and its mascot, as drawn.
pub struct Hearth {
    set: ChiefSettings,
    look: Look,
    /// The colour of its light.
    light: Vec3,
    /// The fastest a robot scurries, which the mascot keeps up with.
    fastest: f32,
    frame: Frame,
    /// The place's entity, which draws the house and the grounds.
    place: Entity,
    panes: Entity,
    orb: Entity,
    ring: Entity,
    glass: Entity,
    trees: [Entity; 3],
    ring_look: Material,
    glass_look: Material,
    flame: Material,
    /// The orb's look and its mesh, kept so that they stay.
    _orb: (Material, Mesh),
    /// The steps of [`Hearth::build`] taken, the meshes they built
    /// that are left to hand over, and those handed over.
    built: usize,
    pending: Vec<(Slot, MeshData)>,
    meshes: Meshes,
    /// The candle the gate's lantern was last lit for.
    candle: Option<f32>,
    mascot: Mascot,
    rig: Rig,
    charm: Charm,
    /// The frame the mascot was last stood at, and its lamp's
    /// lumens.
    stood: Option<Frame>,
    lumens: Option<f32>,
}

impl Hearth {
    /// The chief's place and mascot on the spot the land puts it,
    /// its ground claimed against the towns and held level. None
    /// when the land has no room for it.
    pub fn settle(
        set: &ChiefSettings,
        pal: &Palette,
        look: Look,
        fastest: f32,
        land: &mut Land,
        society: &mut Society,
    ) -> Result<Option<Self>, String> {
        let taken = society
            .towns
            .iter()
            .map(|town| land.sites[town.site].at)
            .chain(society.claims.iter().map(|claim| claim.at))
            .collect::<Vec<_>>();
        let Some(at) = place::stand(land, &taken) else {
            return Ok(None);
        };
        // No town is founded or set down on a chief's ground.
        society.claims.push(Claim {
            place: Place::Chief,
            at,
            radius: land.set.town.keep_apart * 0.5,
        });
        let mut rounds = land.held();
        rounds.push((at, set.grounds));
        rounds.sort_by(|a, b| {
            a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y))
        });
        land.hold(&rounds);
        // The door looks toward the first town.
        let first = land
            .sites
            .iter()
            .find(|site| site.buildable)
            .map_or(Vec2::ZERO, |site| site.at);
        let toward = (first - at).normalize_or(Vec2::Y);
        let yaw = toward.x.atan2(toward.y);
        let frame = Frame::at(land.on_ground(at))
            .with_turn(Quat::from_rotation_y(yaw));

        let light = look.light.colour(pal);
        let place = Entity::make(None, Thing::Hearth.key())?;
        place.set_transform(frame.scene());
        place.watch(true, true);
        let child = || Entity::make(Some(&place), 0);
        let (panes, orb, ring, glass) =
            (child()?, child()?, child()?, child()?);
        for unshadowed in [&panes, &orb, &ring, &glass] {
            unshadowed.set_shadows(false, true);
        }
        let trees = [child()?, child()?, child()?];
        let orb_look = Material::make(lit(light, set.orb_light))?;
        let ring_look =
            Material::make(glow(light, set.waystone_light))?;
        let glass_look = Material::make(Pbr {
            roughness: 0.4,
            ..Pbr::colored(rgba(pal.glass_lit))
        })?;
        let orb_mesh = Mesh::shape(Shape::Sphere(1.0))?;
        orb.draw(&orb_mesh, &orb_look);

        // The mascot, on the waystone and looking out of the gate.
        let stone = place::spots(set)[0].at;
        let stone =
            frame.point(Vec3::new(stone.x, 0.0, stone.y)).xz();
        let flame = Material::make(lit(light, set.wick_light))?;
        let mascot = Mascot::new(
            land.on_ground(stone),
            yaw,
            0.0,
            set.linger.0,
        );
        let rig = Rig::make(Thing::Mascot.key())?;
        rig.light(light, 0.0, set.lamp_range);
        let charm = Charm::make(set, &flame)?;
        Ok(Some(Self {
            set: set.clone(),
            look,
            light,
            fastest,
            frame,
            place,
            panes,
            orb,
            ring,
            glass,
            trees,
            ring_look,
            glass_look,
            flame,
            _orb: (orb_look, orb_mesh),
            built: 0,
            pending: Vec::new(),
            meshes: Meshes::default(),
            candle: None,
            mascot,
            rig,
            charm,
            stood: None,
            lumens: None,
        }))
    }

    /// Where the mascot stands.
    pub fn mascot(&self) -> Vec3 {
        self.mascot.at
    }

    /// The middle of the house.
    pub fn house(&self) -> Vec3 {
        let house = place::house_at(&self.set);
        self.frame.point(Vec3::new(house.x, 0.0, house.y))
    }

    /// Builds what is left to build, and moves and lights the mascot.
    pub fn update(
        &mut self,
        ctx: &Ctx,
        dt: f32,
        upload: &mut Upload,
    ) {
        self.hand_over(ctx, upload);
        let now = ctx.clock;
        let candle = ctx.sky.look.candle;
        if self.candle != Some(candle) {
            let warm =
                ctx.pal.wicks[0] * self.set.lantern_light * candle;
            self.glass_look.set(Pbr {
                emissive: rgba(warm),
                roughness: 0.4,
                ..Pbr::colored(rgba(ctx.pal.glass_lit))
            });
            self.candle = Some(candle);
        }
        let about = Whereabouts {
            land: ctx.land,
            home: &self.frame,
            beside: ctx.beside,
            eye: ctx.eye,
            fastest: self.fastest,
        };
        for end in self.mascot.step(&self.set, &about, now, dt) {
            self.charm.cast(
                &self.set,
                self.light,
                end.ground,
                now,
                end.arrives,
            );
        }
        if self.charm.playing() {
            self.charm.play(&self.set, now);
        }
        let pose = self.mascot.pose(&self.set, now);
        self.rig.place(&pose, self.set.size, self.look.hat.crest());
        let stood = Frame::at(self.mascot.at)
            .with_turn(Quat::from_rotation_y(self.mascot.yaw));
        if self.stood != Some(stood) {
            self.rig.root.set_transform(stood.scene());
            self.stood = Some(stood);
        }
        // The lamp flares as the mascot arrives.
        let flare = match self.mascot.phase {
            Phase::In { began, long } => {
                let u = ((now - began) / long).clamp(0.0, 1.0);
                1.0 + self.set.flare * (1.0 - smoothstep(0.1, 0.7, u))
            }
            Phase::Dark { .. } => 0.0,
            _ => 1.0,
        };
        let lumens = self.set.lamp * (0.25 + 0.75 * candle) * flare;
        if self.lumens != Some(lumens) {
            self.rig.light(self.light, lumens, self.set.lamp_range);
            self.lumens = Some(lumens);
        }
    }

    /// The meshes of step `step` of the place and the mascot, to hand
    /// over: the place, with its lights and trees set where they
    /// stand, then the mascot, then its wick and runes. None past the
    /// last.
    fn build(
        &self,
        step: usize,
        land: &Land,
        pal: &Palette,
    ) -> Option<Vec<(Slot, MeshData)>> {
        match step {
            0 => Some(self.build_place(land, pal)),
            1 => Some(
                parts(pal, self.look)
                    .each()
                    .into_iter()
                    .enumerate()
                    .map(|(i, mb)| (Slot::Part(i), mb.data()))
                    .collect(),
            ),
            2 => Some(
                shapes()
                    .into_iter()
                    .enumerate()
                    .map(|(i, mb)| (Slot::Rune(i), mb.data()))
                    .chain([(Slot::Wick, wick().data())])
                    .collect(),
            ),
            _ => None,
        }
    }

    /// The meshes of the place, and its lights and trees set where
    /// they stand.
    fn build_place(
        &self,
        land: &Land,
        pal: &Palette,
    ) -> Vec<(Slot, MeshData)> {
        let floor = self.frame.at.y;
        let frame = self.frame;
        let Built {
            body,
            panes,
            lights,
            trees,
        } = place::build(pal, &self.set, self.look, |local| {
            let at = frame.point(Vec3::new(local.x, 0.0, local.y));
            land.surface(at.xz()) - floor
        });
        let (orb_at, orb_size) = lights.orb;
        self.orb.set_transform(
            Frame::at(orb_at)
                .with_size(Vec3::splat(orb_size))
                .scene(),
        );
        let (ring_at, ring_size) = lights.ring;
        self.ring.set_transform(
            Frame::at(ring_at)
                .with_size(Vec3::splat(ring_size))
                .scene(),
        );
        self.glass.set_transform(Frame::at(lights.lantern).scene());
        let mut out = vec![
            (Slot::Body, body.data()),
            (Slot::Panes, panes.data()),
            (Slot::Ring, annulus().data()),
            (Slot::Glass, lantern_glass(0.1, 0.24).data()),
        ];
        for (i, tree) in trees.iter().enumerate() {
            self.trees[i].set_transform(
                Frame::at(tree.at)
                    .with_size(Vec3::splat(tree.scale))
                    .scene(),
            );
            out.push((
                Slot::Tree(i),
                flora::mesh(pal, tree.kind).data(),
            ));
        }
        out
    }

    /// Builds what meshes the call has room for, a step at a time,
    /// hands them over, and draws each with what it is for.
    fn hand_over(&mut self, ctx: &Ctx, upload: &mut Upload) {
        while upload.left() {
            if self.pending.is_empty() {
                let Some(built) =
                    self.build(self.built, ctx.land, ctx.pal)
                else {
                    break;
                };
                self.pending = built;
                self.built += 1;
                continue;
            }
            let pending = &mut self.pending;
            let (slot, data) = pending.remove(0);
            let mesh = match upload.mesh(data) {
                Ok(mesh) => mesh,
                Err(Unsent::Full(data)) => {
                    pending.insert(0, (slot, data));
                    break;
                }
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_sdk::warn(&format!(
                        "the Hearth: {why}"
                    ));
                    continue;
                }
            };
            let stock = ctx.stock;
            match slot {
                Slot::Body => self.place.draw(&mesh, &stock.matte),
                Slot::Panes => self.panes.draw(&mesh, &stock.windows),
                Slot::Ring => self.ring.draw(&mesh, &self.ring_look),
                Slot::Glass => {
                    self.glass.draw(&mesh, &self.glass_look)
                }
                Slot::Tree(i) => {
                    self.trees[i].draw(&mesh, &stock.matte)
                }
                Slot::Part(i) => {
                    self.meshes.parts[i] = Some(mesh);
                    continue;
                }
                Slot::Wick => {
                    self.meshes.wick = Some(mesh);
                    continue;
                }
                Slot::Rune(i) => {
                    self.meshes.runes[i] = Some(mesh);
                    continue;
                }
            }
            self.meshes.place.push(mesh);
        }
        // The mascot is dressed, and the runes shaped, once all of
        // theirs are there.
        let meshes = &mut self.meshes;
        let parts = meshes.parts.iter().flatten().collect::<Vec<_>>();
        if !meshes.dressed
            && parts.len() == meshes.parts.len()
            && let Some(wick) = &meshes.wick
        {
            self.rig.dress(
                &parts,
                &ctx.stock.matte,
                (wick, &self.flame),
            );
            meshes.dressed = true;
        }
        let runes = meshes.runes.iter().flatten().collect::<Vec<_>>();
        if !meshes.shaped && runes.len() == meshes.runes.len() {
            self.charm.shape(&runes);
            meshes.shaped = true;
        }
    }
}
