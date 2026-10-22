//! The Agent Bazaar: a striped pavilion and a ring of five stalls on
//! ground of its own beside the first town, in no hamlet. It is built
//! a step at a time within the realm's budgets, the stall nearest the
//! camera first, and from far away it is a few plain shapes. Its
//! keepers, merchant and shoppers, its cogs and motes move only while
//! the camera is close.

mod build;
mod bustle;
mod figure;
mod keepers;
mod lights;
mod merchant;
mod pitch;
mod shoppers;

use glam::{Vec3, Vec3Swizzles};
use hamlet_engine_sdk::scene::{
    Entity, Material, Mesh, MeshData, Pbr,
};

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::stock::rgba;
use crate::build::upload::{Unsent, Upload};
use crate::folk::chief::look::{Light, Look};
use crate::folk::chief::mascot::wick;
use crate::folk::keeper::Pad;
use crate::places::bazaar::build::{Booth, FAIRY, at_stall, placing};
use crate::places::bazaar::bustle::{Motes, Turning};
use crate::places::bazaar::figure::{Figure, On};
use crate::places::bazaar::keepers::Keeper;
use crate::places::bazaar::lights::Lights;
use crate::places::town::build::FlagSettings;
use crate::realm::Ctx;
use crate::society::Society;
use crate::tuning::WorldTuning;
use crate::view::pick::Thing;
use crate::view::signs::Words;
use crate::world::land::Land;
use crate::world::palette::Palette;

/// Reach of the bazaar from its middle and its height, for the ring
/// about it and the name over it.
const SIZE: (f32, f32) = (11.6, 8.6);

/// The reach of a stall from its middle and its height, for the ring
/// about it and the name over it, and how far before its middle the
/// ring lies.
const STALL: (f32, f32, f32) = (2.3, 3.6, 0.25);

/// The figures of the bazaar: the keepers, in the order of
/// [`Stall::ALL`], the merchant and then the shoppers.
const MERCHANT: usize = 5;
const FIGURES: usize = MERCHANT + 1 + shoppers::LOOKS.len();

/// The bazaar's lights, its people and what moves about it.
#[derive(Clone, PartialEq, Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BazaarSettings {
    /// The light one of its lanterns throws after dark, and how far
    /// it reaches.
    pub lamp: f32,
    pub lamp_reach: f32,
    /// Brightness of the lanterns' glass by day and its gain after
    /// dark, and how many times brighter it burns after a sale.
    pub glass: (f32, f32),
    pub flash: f32,
    /// Brightness of what glows among the wares by day, and its gain
    /// after dark.
    pub gleam: (f32, f32),
    /// A fairy bulb's glow by day, its gain after dark and how many
    /// times brighter it burns after a sale.
    pub bulb: (f32, f32, f32),
    /// The rounds a second the fairy lights' chase runs along a
    /// string, and how many times faster it runs after a sale.
    pub chase: (f32, f32),
    /// The light of a glow over the avenue after dark, how far it
    /// reaches and the seconds it keeps each colour.
    pub glow: f32,
    pub glow_reach: f32,
    pub glow_holds: f32,
    /// The light over the counter of a stall that sold something, at
    /// its brightest, and how far it reaches.
    pub flare: f32,
    pub flare_reach: f32,
    /// Seconds a sale is cheered.
    pub cheers: f32,
    /// Seconds a keeper waves once its stall is chosen, and takes to
    /// ease from one motion into the next.
    pub waves: f32,
    pub blend: f32,
    /// How fast a keeper turns, in radians a second.
    pub turn: f32,
    /// A shopper's walk in units a second, the seconds it takes to
    /// turn, and its size as a share of a chief's mascot.
    pub pace: f32,
    pub shopper_turn: f32,
    pub grown: f32,
    /// The motes of the sparks off the grindstone, the shimmer over
    /// the scrolls, the steam off the tea and the embers over a
    /// counter after a sale: how many, the seconds each lasts, its
    /// size and its light.
    pub sparks: (usize, f32, f32, f32),
    pub shimmer: (usize, f32, f32, f32),
    pub steam: (usize, f32, f32, f32),
    pub embers: (usize, f32, f32, f32),
}

/// A stall of the bazaar.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Stall {
    /// The models agents run.
    Models,
    /// Agents other users made, ready to join a hamlet.
    Community,
    /// Extensions that add features to Hamlet itself.
    Plugins,
    /// Skills for the library.
    Skills,
    /// Whole hamlets, mayor, agents and notes, ready to use.
    Hamlets,
}

impl Stall {
    pub const ALL: [Stall; 5] = [
        Stall::Models,
        Stall::Community,
        Stall::Plugins,
        Stall::Skills,
        Stall::Hamlets,
    ];

    /// The name over it.
    pub fn name(self) -> &'static str {
        match self {
            Stall::Models => "AI Models",
            Stall::Community => "Community Agents",
            Stall::Plugins => "Plugins",
            Stall::Skills => "Skills",
            Stall::Hamlets => "Hamlet Templates",
        }
    }

    /// The name its events give it.
    pub fn short(self) -> &'static str {
        match self {
            Stall::Models => "models",
            Stall::Community => "community",
            Stall::Plugins => "plugins",
            Stall::Skills => "skills",
            Stall::Hamlets => "hamlets",
        }
    }

    /// The stall of [`Stall::short`] `short`.
    pub fn named(short: &str) -> Option<Stall> {
        Stall::ALL.into_iter().find(|stall| stall.short() == short)
    }
}

/// What a mesh handed over is for.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Slot {
    Far,
    Body,
    Props,
    Strings,
    Booth(Stall),
    Glass,
    Gleam,
    Bulb(usize),
    Cog(usize),
    Wick,
    /// Part `1` of figure `0`, in the order of `Parts::each`.
    Part(usize, usize),
    Worn(usize, On),
}

/// The steps of building it, in their order. Each booth is a step of
/// its own.
const FAR: usize = 0;
const BODY: usize = 1;
const BOOTHS: usize = 2;
const PROPS: usize = BOOTHS + Stall::ALL.len();
const STRINGS: usize = PROPS + 1;
const WICK: usize = STRINGS + 1;
const PEOPLE: usize = WICK + 1;
const DONE: usize = PEOPLE + FIGURES;

/// The figures of the bazaar.
struct Folk {
    keepers: Vec<Keeper>,
    merchant: Figure,
    shoppers: Vec<Figure>,
}

impl Folk {
    /// Figure `i`: a keeper, the merchant or a shopper.
    fn figure(&mut self, i: usize) -> Option<&mut Figure> {
        match i {
            _ if i < MERCHANT => {
                self.keepers.get_mut(i).map(|k| &mut k.figure)
            }
            MERCHANT => Some(&mut self.merchant),
            _ => self.shoppers.get_mut(i - MERCHANT - 1),
        }
    }
}

/// The bazaar as drawn.
pub struct Bazaar {
    set: BazaarSettings,
    flag: FlagSettings,
    frame: Frame,
    /// Its whole at its middle, the parent of the rest.
    _root: Entity,
    /// What is drawn near, from far, and the small things drawn and
    /// moved only while the camera is close.
    near: Entity,
    far: Entity,
    small: Entity,
    body: Entity,
    props: Entity,
    strings: Entity,
    booths: [Entity; 5],
    glass: Entity,
    gleam: Entity,
    bulbs: [Entity; FAIRY],
    cogs: Vec<Turning>,
    lights: Lights,
    motes: Motes,
    /// The flame of each light a figure's wick may have, in the
    /// order of [`Light`].
    flames: Vec<Material>,
    folk: Folk,
    /// The steps taken, the booths left to build and where the
    /// strings of lights are tied to each booth built.
    built: usize,
    left: Vec<Stall>,
    ties: [Option<(Vec3, [Vec3; 2])>; 5],
    /// The glass of the lanterns and what glows among the wares, as
    /// each step adds to them.
    glass_mb: Mb,
    gleam_mb: Mb,
    /// Meshes built and left to hand over, those handed over, kept
    /// so that they stay, the wick every figure shares and how
    /// many of the body and the booths are drawn.
    pending: Vec<(Slot, MeshData)>,
    meshes: Vec<Mesh>,
    wick: Option<Mesh>,
    drawn: usize,
    /// True while what is near is shown, and while the camera is
    /// close; none until first shown.
    near_shown: Option<bool>,
    close: Option<bool>,
    /// The latest sale: its stall and its clock time.
    sale: Option<(Stall, f32)>,
    /// The clock time it last moved.
    moved: f32,
}

impl Bazaar {
    /// Pitches the bazaar where the land puts it, its ground claimed
    /// and held, with nothing built yet. None when the land has no
    /// room for it.
    pub fn pitch(
        tuning: &WorldTuning,
        pal: &Palette,
        land: &mut Land,
        society: &mut Society,
    ) -> Result<Option<Self>, String> {
        let Some(frame) = pitch::pitch(land, society) else {
            return Ok(None);
        };
        let set = tuning.bazaar.clone();
        let root = Entity::make(None, 0)?;
        root.set_transform(frame.scene());
        let near = Entity::make(Some(&root), 0)?;
        near.set_visible(false);
        let far = Entity::make(Some(&root), Thing::Bazaar.key())?;
        far.watch(true, true);
        let small = Entity::make(Some(&root), 0)?;
        small.set_visible(false);
        let picked = |thing: Thing| -> Result<Entity, String> {
            let entity = Entity::make(Some(&near), thing.key())?;
            entity.watch(true, true);
            Ok(entity)
        };
        let (body, props, strings) = (
            picked(Thing::Bazaar)?,
            picked(Thing::Bazaar)?,
            picked(Thing::Bazaar)?,
        );
        let [a, b, c, d, e] =
            Stall::ALL.map(|s| picked(Thing::Stall(s)));
        let unshadowed = || -> Result<Entity, String> {
            let entity = Entity::make(Some(&small), 0)?;
            entity.set_shadows(false, true);
            Ok(entity)
        };
        let (glass, gleam) = (unshadowed()?, unshadowed()?);
        let [ba, bb, bc, bd, be, bf] =
            [(); FAIRY].map(|()| unshadowed());
        let flames = [
            Light::Candle,
            Light::Rune,
            Light::Rosy,
            Light::Leaf,
            Light::Moon,
        ]
        .map(|light| {
            Material::make(Pbr {
                unlit: true,
                fog: false,
                ..Pbr::colored(rgba(
                    light.colour(pal) * tuning.chief.wick_light,
                ))
            })
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
        let figure =
            |key: u32, look: Look| Figure::make(&small, key, look);
        let keepers = Stall::ALL
            .into_iter()
            .map(|stall| {
                figure(
                    Thing::Stall(stall).key(),
                    keepers::LOOKS[stall as usize],
                )
                .map(|it| Keeper::new(stall, it))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let merchant = figure(Thing::Bazaar.key(), merchant::LOOK)?;
        let shoppers = shoppers::LOOKS
            .into_iter()
            .map(|look| figure(0, look))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(Self {
            lights: Lights::make(&small, pal)?,
            motes: Motes::make(&small, &set, pal)?,
            set,
            flag: FlagSettings::read()?,
            frame,
            _root: root,
            near,
            far,
            small,
            body,
            props,
            strings,
            booths: [a?, b?, c?, d?, e?],
            glass,
            gleam,
            bulbs: [ba?, bb?, bc?, bd?, be?, bf?],
            cogs: Vec::new(),
            flames,
            folk: Folk {
                keepers,
                merchant,
                shoppers,
            },
            built: 0,
            left: Stall::ALL.to_vec(),
            ties: [None; 5],
            glass_mb: Mb::new(),
            gleam_mb: Mb::new(),
            pending: Vec::new(),
            meshes: Vec::new(),
            wick: None,
            drawn: 0,
            near_shown: None,
            close: None,
            sale: None,
            moved: 0.0,
        }))
    }

    /// The middle of the bazaar on the ground.
    pub fn middle(&self) -> Vec3 {
        self.frame.at
    }

    /// Its paving, which the marks of light on the ground are laid
    /// on.
    pub fn paving(&self) -> Pad {
        Pad {
            at: self.frame.at.xz(),
            radius: build::PAVED,
            top: self.frame.at.y + build::FLOOR,
        }
    }

    /// The ground a ring about `thing` lies on, and its radius, if it
    /// is of the bazaar.
    pub fn ring(&self, thing: Thing) -> Option<(Vec3, f32)> {
        match thing {
            Thing::Bazaar => Some((self.frame.at, SIZE.0)),
            Thing::Stall(stall) => Some((
                self.frame.point(at_stall(stall, Vec3::Z * STALL.2)),
                STALL.0,
            )),
            _ => None,
        }
    }

    /// The signs over the pavilion and each stall.
    pub fn signs(&self) -> Vec<(Thing, Words)> {
        let over = |at: Vec3, says: &str| Words {
            at,
            says: says.to_uppercase(),
        };
        let mut signs = vec![(
            Thing::Bazaar,
            over(self.frame.point(Vec3::Y * SIZE.1), "Agent Bazaar"),
        )];
        for stall in Stall::ALL {
            let at =
                self.frame.point(at_stall(stall, Vec3::Y * STALL.1));
            signs.push((Thing::Stall(stall), over(at, stall.name())));
        }
        signs
    }

    /// The chief bought something at `stall`, at clock time `now`:
    /// its keeper and the merchant wave, its lights flare and embers
    /// rise over its counter.
    pub fn trade(&mut self, stall: Stall, now: f32) {
        self.sale = Some((stall, now));
    }

    /// Builds what is left to build, shows it near or far, and moves
    /// and lights what is about it while the camera is close.
    /// `chosen` is the thing selected.
    pub fn update(
        &mut self,
        ctx: &Ctx,
        chosen: Option<Thing>,
        upload: &mut Upload,
    ) {
        self.hand_over(ctx, upload);
        let away = ctx.eye.distance(self.frame.at);
        let detail = &ctx.tuning.detail;
        let near = away < detail.town_far
            && self.drawn == 1 + Stall::ALL.len();
        if self.near_shown != Some(near) {
            self.near.set_visible(near);
            self.far.set_visible(!near);
            self.near_shown = Some(near);
        }
        let close = near && away < detail.robots_far;
        if self.close != Some(close) {
            self.small.set_visible(close);
            self.close = Some(close);
        }
        let now = ctx.clock;
        let dt = (now - self.moved).clamp(0.0, 0.1);
        self.moved = now;
        if !close {
            return;
        }
        let set = &self.set;
        let sale = self
            .sale
            .filter(|(_, at)| now - at < set.cheers + set.embers.1);
        self.lights.shine(
            set,
            ctx.pal,
            (now, ctx.sky.look.candle),
            sale,
        );
        let cheering = sale.filter(|(_, at)| now - at < set.cheers);
        let size = ctx.tuning.chief.size;
        // The camera, from the bazaar's middle.
        let eye =
            self.frame.turn.inverse() * (ctx.eye - self.frame.at);
        let folk = &mut self.folk;
        for keeper in &mut folk.keepers {
            let called = chosen == Some(Thing::Stall(keeper.stall));
            let cheers =
                cheering.is_some_and(|(s, _)| s == keeper.stall);
            keeper.tend(set, (now, dt), (called, cheers), eye, size);
        }
        merchant::tend(
            &mut folk.merchant,
            now,
            cheering.is_some(),
            size,
        );
        for (round, shopper) in folk.shoppers.iter_mut().enumerate() {
            shoppers::stroll(shopper, round, set, now, size);
        }
        for cog in &self.cogs {
            cog.turn(now);
        }
        let called = chosen == Some(Thing::Stall(Stall::Plugins));
        self.motes.stir(set, now, called, sale, upload);
    }

    /// The meshes of the next step, to hand over. None past the last.
    fn build(&mut self, ctx: &Ctx) -> Option<Vec<(Slot, MeshData)>> {
        let pal = ctx.pal;
        let step = self.built;
        let out = match step {
            FAR => vec![(Slot::Far, build::far(pal).data())],
            BODY => {
                let (body, lamps) =
                    build::body(pal, &self.flag, &mut self.glass_mb);
                // A light in the pavilion and one at the gate.
                let (inside, gate) =
                    lamps.split_at(lamps.len().min(5));
                for spots in [inside, gate] {
                    if !spots.is_empty() {
                        let sum = spots.iter().copied().sum::<Vec3>();
                        self.lights.hang(
                            &self.small,
                            sum / spots.len() as f32,
                        );
                    }
                }
                vec![(Slot::Body, body.data())]
            }
            _ if step < PROPS => self.booth(ctx)?,
            PROPS => vec![(Slot::Props, build::props(pal).data())],
            STRINGS => {
                let ties = self
                    .ties
                    .iter()
                    .flatten()
                    .copied()
                    .collect::<Vec<_>>();
                let (strings, bulbs) = build::strings(pal, &ties);
                let mut out = vec![(Slot::Strings, strings.data())];
                out.extend(
                    bulbs
                        .into_iter()
                        .enumerate()
                        .map(|(k, mb)| (Slot::Bulb(k), mb.data())),
                );
                let glass =
                    std::mem::replace(&mut self.glass_mb, Mb::new());
                let gleam =
                    std::mem::replace(&mut self.gleam_mb, Mb::new());
                out.push((Slot::Glass, glass.data()));
                out.push((Slot::Gleam, gleam.data()));
                out
            }
            WICK => vec![(Slot::Wick, wick().data())],
            _ if step < DONE => self.dressing(pal, step - PEOPLE)?,
            DONE => {
                hamlet_engine_sdk::info("the Agent Bazaar is built");
                Vec::new()
            }
            _ => return None,
        };
        self.built += 1;
        Some(out)
    }

    /// The meshes of the booth left nearest the camera, and its cogs
    /// made to draw them.
    fn booth(&mut self, ctx: &Ctx) -> Option<Vec<(Slot, MeshData)>> {
        let (eye, frame) = (ctx.eye, self.frame);
        let away = |stall: &Stall| {
            frame.point(placing(*stall).0).distance_squared(eye)
        };
        let nearest = (0..self.left.len()).min_by(|a, b| {
            away(&self.left[*a]).total_cmp(&away(&self.left[*b]))
        })?;
        let stall = self.left.swap_remove(nearest);
        let Booth {
            mesh,
            lamp,
            ties,
            cogs,
        } = build::booth(
            ctx.pal,
            &self.flag,
            stall,
            (&mut self.glass_mb, &mut self.gleam_mb),
        );
        self.lights.hang(&self.small, lamp);
        self.ties[stall as usize] = Some(ties);
        let mut out = vec![(Slot::Booth(stall), mesh.data())];
        for cog in cogs {
            let Ok(entity) = Entity::make(Some(&self.small), 0)
            else {
                continue;
            };
            let turning = Turning {
                entity,
                at: cog.at,
                lies: cog.lies,
                rate: cog.rate,
                phase: cog.phase,
            };
            turning.turn(0.0);
            out.push((Slot::Cog(self.cogs.len()), cog.mesh.data()));
            self.cogs.push(turning);
        }
        Some(out)
    }

    /// The meshes of figure `i`: its parts, and what it wears over
    /// them.
    fn dressing(
        &mut self,
        pal: &Palette,
        i: usize,
    ) -> Option<Vec<(Slot, MeshData)>> {
        let mut out = self
            .folk
            .figure(i)?
            .meshes(pal)
            .into_iter()
            .enumerate()
            .map(|(k, mb)| (Slot::Part(i, k), mb.data()))
            .collect::<Vec<_>>();
        let worn = match i {
            _ if i < MERCHANT => {
                let worn = keepers::worn(pal, Stall::ALL[i]);
                vec![
                    (On::Head, worn.head),
                    (On::Hand, worn.held),
                    (On::Glowing, worn.glows),
                ]
            }
            MERCHANT => vec![(On::Head, merchant::turban(pal))],
            _ => Vec::new(),
        };
        out.extend(
            worn.into_iter()
                .filter(|(_, mb)| !mb.is_empty())
                .map(|(on, mb)| (Slot::Worn(i, on), mb.data())),
        );
        Some(out)
    }

    /// Builds what meshes the call has room for, a step at a time,
    /// hands them over, and draws each with what it is for.
    fn hand_over(&mut self, ctx: &Ctx, upload: &mut Upload) {
        let mut handed = false;
        while upload.left() {
            if self.pending.is_empty() {
                let Some(built) = self.build(ctx) else {
                    break;
                };
                self.pending = built;
                continue;
            }
            let (slot, data) = self.pending.remove(0);
            let mesh = match upload.mesh(data) {
                Ok(mesh) => mesh,
                Err(Unsent::Full(data)) => {
                    self.pending.insert(0, (slot, data));
                    break;
                }
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_sdk::warn(&format!(
                        "the bazaar: {why}"
                    ));
                    continue;
                }
            };
            handed = true;
            let matte = &ctx.stock.matte;
            let lights = &self.lights;
            match slot {
                Slot::Far => self.far.draw(&mesh, matte),
                Slot::Body => self.body.draw(&mesh, matte),
                Slot::Props => self.props.draw(&mesh, matte),
                Slot::Strings => self.strings.draw(&mesh, matte),
                Slot::Booth(stall) => {
                    self.booths[stall as usize].draw(&mesh, matte)
                }
                Slot::Glass => self.glass.draw(&mesh, &lights.glass),
                Slot::Gleam => self.gleam.draw(&mesh, &lights.gleam),
                Slot::Bulb(k) => {
                    self.bulbs[k].draw(&mesh, &lights.bulbs[k])
                }
                Slot::Cog(i) => {
                    if let Some(cog) = self.cogs.get(i) {
                        cog.entity.draw(&mesh, matte);
                    }
                }
                Slot::Wick => {
                    self.wick = Some(mesh);
                    continue;
                }
                Slot::Part(i, k) => {
                    if let Some(figure) = self.folk.figure(i) {
                        figure.part(k, mesh);
                    }
                    continue;
                }
                Slot::Worn(i, on) => {
                    if let Some(figure) = self.folk.figure(i) {
                        figure.wear(on, mesh, (matte, &lights.gleam));
                    }
                    continue;
                }
            }
            if matches!(slot, Slot::Body | Slot::Booth(_)) {
                self.drawn += 1;
            }
            self.meshes.push(mesh);
        }
        if !handed {
            return;
        }
        // A figure is dressed once all of its meshes are there.
        let Some(wick) = &self.wick else {
            return;
        };
        let matte = &ctx.stock.matte;
        let flames = &self.flames;
        let dress = |figure: &mut Figure| {
            let flame = &flames[figure.look.light as usize];
            figure.dress(matte, (wick, flame));
        };
        let folk = &mut self.folk;
        for keeper in &mut folk.keepers {
            dress(&mut keeper.figure);
        }
        dress(&mut folk.merchant);
        for shopper in &mut folk.shoppers {
            dress(shopper);
        }
    }
}

#[cfg(test)]
mod tests;
