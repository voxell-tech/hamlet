//! Each town as the society holds it: its common, a house per agent
//! with its yard, and one plain mesh of the whole from far. What is
//! drawn is brought in line with the society only when its shape
//! changes, and a house is built again only when what it is built
//! from does. Houses are built a few a frame; one that replaces
//! another stands up as the other goes.

mod build;
mod common;
mod kit;
mod lights;

use std::collections::{HashMap, HashSet, VecDeque};

use glam::{Quat, Vec2, Vec3};
use hamlet_engine_sdk::api::agents::Doing;
use hamlet_engine_sdk::scene::{Entity, Mesh};

use self::build::{Built, FlagSettings, Recipe};
use self::kit::{Kit, Part as KitPart};
use self::lights::Lamps;
use crate::frame::Frame;
use crate::house::{self, TownStyle};
use crate::keeper::KState;
use crate::land::{Land, LandData};
use crate::layout::{HALL_PLOT, plot};
use crate::mesh::Mb;
use crate::pick::Thing;
use crate::realm::Ctx;
use crate::shift::{Part, Shift, Way};
use crate::society::{Agent, AgentId, Society, Town, TownId};
use crate::upload::{Unsent, Upload};
use crate::yard::Yard;

/// Key of the entities nobody points at.
const PLAIN: u32 = 0;

/// Speed of a worked wheel, in radians a second.
const CRANK_RATE: f32 = 5.2;

/// What a house is built from, besides its agent. A house is built
/// again when it changes.
#[derive(Clone, Copy, PartialEq, Debug)]
struct HouseSig {
    site: usize,
    /// The place of its town.
    at: Vec2,
    plot: u32,
    archived: bool,
    cleared: bool,
    leader: bool,
    wings: u8,
}

/// A thing that stands about a house, under it.
struct Prop {
    entity: Entity,
    /// Its place at rest, in the house's frame.
    rest: Frame,
    /// True when it outlasts the house: it stands while the house
    /// comes or goes around it.
    stays: bool,
}

/// The engine of a house's yard: its wheel and its rings of steam.
struct Engine {
    wheel: Entity,
    rings: [Entity; 2],
    /// The wheel's turn, and the seconds it has run.
    turn: f32,
    ran: Option<f32>,
}

/// A house as it is drawn.
struct House {
    agent: AgentId,
    town: TownId,
    sig: HouseSig,
    entity: Entity,
    /// Its place at rest.
    frame: Frame,
    /// Its meshes while they are being handed over, then as they
    /// are drawn.
    built: Option<Built>,
    drawn: bool,
    /// Its window panes, and its door lantern with the state it
    /// shows and where it hangs in the realm.
    panes: Option<Entity>,
    lamp: Option<(Entity, KState, Vec3)>,
    props: Vec<Prop>,
    yard: Yard,
    engine: Option<Engine>,
    shift: Option<Shift>,
    /// The way it changes from what it replaces.
    way: Option<Way>,
    /// Seconds before what it replaces goes, and before it comes.
    wait: (f32, f32),
    /// True while it is shown.
    seen: bool,
}

/// A town as it is drawn.
struct TownDrawn {
    site: usize,
    /// Its place when it was last drawn.
    at: Vec2,
    /// Parent of the houses and the common, shown near.
    near: Entity,
    common: Entity,
    /// Its mesh once it is drawn; none for one the engine refused.
    common_mesh: Option<Option<Mesh>>,
    _lamp: Entity,
    /// The whole as one mesh, shown far, and its window panes.
    far: Entity,
    far_panes: Entity,
    far_meshes: Option<(Mesh, Mesh)>,
    /// True while its houses are shown, false while its far mesh is,
    /// none until it is first shown.
    near_shown: Option<bool>,
}

/// The towns drawn.
pub struct Towns {
    kit: Kit,
    lamps: Lamps,
    flag: FlagSettings,
    towns: HashMap<TownId, TownDrawn>,
    houses: HashMap<AgentId, House>,
    /// The house each house being built replaces, drawn until the
    /// new one is.
    replaced: HashMap<AgentId, House>,
    /// Houses that are going, playing their part until they are
    /// gone.
    leaving: Vec<House>,
    /// Houses to build, oldest first.
    jobs: VecDeque<AgentId>,
    /// Towns whose far mesh is to be built anew.
    stale: HashSet<TownId>,
    /// The society's shape and the land's places when last brought
    /// in line.
    shape: Option<(u64, u64)>,
}

/// The towns' methods on [`Entity`].
trait EntityExt {
    /// Shows it or hides it as `want` says, where `seen` says it is
    /// not so already.
    fn show_as(&self, seen: &mut bool, want: bool);
}

impl EntityExt for Entity {
    fn show_as(&self, seen: &mut bool, want: bool) {
        if *seen != want {
            *seen = want;
            self.set_visible(want);
        }
    }
}

impl Towns {
    pub fn make(
        ctx: &Ctx,
        upload: &mut Upload,
    ) -> Result<Self, String> {
        let mut kit = Kit::new();
        kit.fill(ctx.pal, &ctx.tuning.generator.yard, upload);
        Ok(Self {
            kit,
            lamps: Lamps::make(ctx.pal)?,
            flag: FlagSettings::read()?,
            towns: HashMap::new(),
            houses: HashMap::new(),
            replaced: HashMap::new(),
            leaving: Vec::new(),
            jobs: VecDeque::new(),
            stale: HashSet::new(),
            shape: None,
        })
    }

    /// Brings what is drawn in line with the society, within
    /// `upload`.
    pub fn update(&mut self, ctx: &Ctx, upload: &mut Upload) {
        self.lamps.light(ctx.pal, ctx.sky, ctx.clock);
        if !self.kit.fill(ctx.pal, &ctx.tuning.generator.yard, upload)
        {
            return;
        }
        let shape = (ctx.society.shape(), ctx.land.places());
        if self.shape != Some(shape) {
            self.sync(ctx);
            self.shape = Some(shape);
        }
        self.build(ctx, upload);
        self.far(ctx, upload);
        self.commons(ctx, upload);
        self.shift(ctx);
        self.detail(ctx);
        self.states(ctx);
        self.engines(ctx);
        self.spill(ctx);
    }

    /// Takes out what the society no longer holds, and draws what is
    /// new or changed.
    fn sync(&mut self, ctx: &Ctx) {
        let (society, land) = (ctx.society, ctx.land);
        // Towns gone, or founded anew on another site.
        self.towns.retain(|id, group| {
            society.town(*id).is_some_and(|t| t.site == group.site)
        });
        let towns = &self.towns;
        self.houses
            .retain(|_, house| towns.contains_key(&house.town));
        self.houses.retain(|id, _| society.agent(*id).is_some());
        let houses = &self.houses;
        self.replaced.retain(|id, _| houses.contains_key(id));
        self.leaving.retain(|house| towns.contains_key(&house.town));
        self.jobs.retain(|agent| houses.contains_key(agent));
        let (founded, carried) = self.towns(ctx);
        for agent in &society.agents {
            let Some(town) = society.town(agent.town) else {
                continue;
            };
            if !self.towns.contains_key(&town.id) {
                continue;
            }
            let sig = HouseSig {
                site: town.site,
                at: land.sites[town.site].at,
                plot: agent.plot,
                archived: agent.archived,
                cleared: agent.cleared,
                leader: town.leader == agent.id,
                wings: agent.wings,
            };
            if self.houses.get(&agent.id).map(|h| h.sig) == Some(sig)
            {
                continue;
            }
            let founded = founded.contains(&town.id);
            let carried = carried.contains(&town.id);
            self.house(ctx, agent, town, sig, founded, carried);
        }
    }

    /// Makes the groups and the common of each town that is new, and
    /// moves the common of one set down elsewhere. The towns first
    /// drawn, and those carried.
    fn towns(
        &mut self,
        ctx: &Ctx,
    ) -> (HashSet<TownId>, HashSet<TownId>) {
        let land = ctx.land;
        let (mut founded, mut carried) =
            (HashSet::new(), HashSet::new());
        for town in &ctx.society.towns {
            let centre = land.sites[town.site].at;
            if let Some(group) = self.towns.get_mut(&town.id) {
                if group.at != centre {
                    group.at = centre;
                    group.common.set_transform(
                        land.common_frame(town.site).scene(),
                    );
                    self.stale.insert(town.id);
                    carried.insert(town.id);
                }
                continue;
            }
            match TownDrawn::make(ctx, town) {
                Ok(group) => {
                    self.towns.insert(town.id, group);
                    self.stale.insert(town.id);
                    founded.insert(town.id);
                }
                Err(why) => {
                    hamlet_engine_sdk::warn(&format!(
                        "a town: {why}"
                    ));
                }
            }
        }
        (founded, carried)
    }

    /// Makes the house of `agent` as `sig` has it, in place of the
    /// one drawn for it.
    fn house(
        &mut self,
        ctx: &Ctx,
        agent: &Agent,
        town: &Town,
        sig: HouseSig,
        founded: bool,
        carried: bool,
    ) {
        let (land, motion) = (ctx.land, &ctx.tuning.house_motion);
        let Some(group) = self.towns.get(&town.id) else {
            return;
        };
        let hall = agent.plot == HALL_PLOT;
        let spot = plot(land, town.site, agent.plot);
        let yard = house::yard_of(
            land,
            agent.id,
            hall,
            spot.yaw,
            agent.wings,
        );
        let was = self.houses.remove(&agent.id);
        let moved = was.as_ref().is_some_and(|was| {
            (was.sig.site, was.sig.plot, was.sig.at)
                != (sig.site, sig.plot, sig.at)
        });
        // The houses of a town that is set down stand up one after
        // another, from the hall outward.
        let wait = if carried {
            let turn = agent.plot as f32 * motion.move_gap;
            (turn, turn + motion.move_after)
        } else {
            (0.0, 0.0)
        };
        let way = match (
            was.as_ref().map(|was| was.sig.archived),
            sig.archived,
        ) {
            _ if founded => None,
            (None | Some(true), false) => Some(Way::In),
            (Some(false), false) if moved => Some(Way::In),
            (Some(false), true) => Some(Way::Out),
            _ => None,
        };
        // The house it replaces stays drawn until the new one is
        // built. One not built yet gives way to what it was to
        // replace.
        if let Some(was) = was {
            self.stale.insert(was.town);
            if was.drawn {
                self.replaced.insert(agent.id, was);
            }
        }
        let frame = Frame::at(land.on_ground(spot.at))
            .with_turn(Quat::from_rotation_y(spot.yaw));
        let Ok(entity) = Entity::make(
            Some(&group.near),
            Thing::House(agent.id).key(),
        ) else {
            return;
        };
        entity.set_transform(frame.scene());
        entity.set_visible(false);
        let mut made = House {
            agent: agent.id,
            town: town.id,
            sig,
            entity,
            frame,
            built: Some(Built::of(
                land,
                &self.flag,
                Recipe {
                    agent: agent.id,
                    site: town.site,
                    hall,
                    leader: sig.leader,
                    remains: agent.remains(&land.set.house),
                    wings: agent.wings,
                    yard,
                },
            )),
            drawn: false,
            panes: None,
            lamp: None,
            props: Vec::new(),
            yard,
            engine: None,
            shift: None,
            way,
            wait,
            seen: false,
        };
        if !agent.archived {
            self.furnish(ctx, &mut made, &yard);
        }
        if let Some((at, kind, scale)) =
            house::garden_tree(land, agent.id, &yard)
            && !hall
            && let Some(mesh) = self.kit.get(KitPart::Tree(kind))
            && let Ok(tree) = Entity::make(Some(&made.entity), PLAIN)
        {
            let rest = Frame::at(at).with_size(Vec3::splat(scale));
            tree.set_transform(rest.scene());
            tree.draw(mesh, &ctx.stock.matte);
            made.props.push(Prop {
                entity: tree,
                rest,
                stays: true,
            });
        }
        self.jobs.push_back(agent.id);
        self.houses.insert(agent.id, made);
        self.stale.insert(town.id);
    }

    /// Stands the desk and the engine in the yard of `house`.
    fn furnish(&self, ctx: &Ctx, house: &mut House, yard: &Yard) {
        let kit = &self.kit;
        let stock = ctx.stock;
        let part = |mesh: KitPart, rest: Frame, under: &Entity| {
            let entity = Entity::make(Some(under), PLAIN).ok()?;
            entity.set_transform(rest.scene());
            entity.draw(kit.get(mesh)?, &stock.matte);
            Some(entity)
        };
        let (Some(desk), Some(engine)) = (
            part(KitPart::Desk, yard.desk_at(), &house.entity),
            part(KitPart::Engine, yard.engine_at(), &house.entity),
        ) else {
            return;
        };
        let wheel = part(KitPart::Wheel, yard.wheel(0.0), &engine);
        // Steam shows the light of work.
        let rings = [(); 2].map(|_| {
            let ring =
                Entity::make(Some(&house.entity), PLAIN).ok()?;
            ring.draw(
                kit.get(KitPart::Ring)?,
                self.lamps.of(KState::Working),
            );
            ring.set_shadows(false, false);
            ring.set_visible(false);
            Some(ring)
        });
        if let (Some(wheel), [Some(a), Some(b)]) = (wheel, rings) {
            house.engine = Some(Engine {
                wheel,
                rings: [a, b],
                turn: 0.0,
                ran: None,
            });
        }
        for (entity, rest) in
            [(desk, yard.desk_at()), (engine, yard.engine_at())]
        {
            house.props.push(Prop {
                entity,
                rest,
                stays: false,
            });
        }
    }

    /// Builds the houses waiting, nearest the camera first, while the
    /// call has room.
    fn build(&mut self, ctx: &Ctx, upload: &mut Upload) {
        let eye = ctx.eye;
        let houses = &self.houses;
        self.jobs.make_contiguous().sort_by(|a, b| {
            let away = |agent: &AgentId| {
                houses.get(agent).map_or(f32::MAX, |h| {
                    h.frame.at.distance_squared(eye)
                })
            };
            away(a).total_cmp(&away(b))
        });
        while upload.left() {
            let Some(&agent) = self.jobs.front() else {
                break;
            };
            let Some(house) = self.houses.get_mut(&agent) else {
                self.jobs.pop_front();
                continue;
            };
            let done = house
                .built
                .as_mut()
                .is_none_or(|built| built.hand(upload));
            if !done {
                break;
            }
            self.jobs.pop_front();
            let state = ctx
                .society
                .agent(agent)
                .map_or(KState::Rest, |a| a.state);
            let old = self.replaced.remove(&agent);
            house.finish(
                ctx,
                &self.kit,
                &self.lamps,
                state,
                old.is_some(),
            );
            if let (Some(mut old), Some(way)) = (old, house.way) {
                let (_, goes) = way.parts();
                old.shift =
                    Some(Shift::new(goes, true).after(house.wait.0));
                self.leaving.push(old);
            }
            self.stale.insert(house.town);
        }
    }

    /// Builds the far mesh of each town that changed.
    fn far(&mut self, ctx: &Ctx, upload: &mut Upload) {
        let (land, society) = (ctx.land, ctx.society);
        let stale = self.stale.iter().copied().collect::<Vec<_>>();
        for id in stale {
            if !upload.left() {
                return;
            }
            let Some(group) = self.towns.get_mut(&id) else {
                self.stale.remove(&id);
                continue;
            };
            let (mb, panes) = far_mesh(land, society, id, group.site);
            let (body, panes) =
                match (mb.upload(upload), panes.upload(upload)) {
                    (Ok(body), Ok(panes)) => (body, panes),
                    (Err(Unsent::Refused(why)), _)
                    | (_, Err(Unsent::Refused(why))) => {
                        hamlet_engine_sdk::warn(&format!(
                            "a town from far: {why}"
                        ));
                        self.stale.remove(&id);
                        continue;
                    }
                    _ => return,
                };
            group.far.draw(&body, &ctx.stock.matte);
            group.far_panes.draw(&panes, &ctx.stock.windows);
            group.far_meshes = Some((body, panes));
            self.stale.remove(&id);
        }
    }

    /// Builds the common of each town that has none yet.
    fn commons(&mut self, ctx: &Ctx, upload: &mut Upload) {
        for group in self.towns.values_mut() {
            if group.common_mesh.is_some() {
                continue;
            }
            if !upload.left() {
                return;
            }
            let style = TownStyle::of(ctx.land, group.site);
            match common::mesh(ctx.land, group.site, &style)
                .upload(upload)
            {
                Ok(mesh) => {
                    group.common.draw(&mesh, &ctx.stock.matte);
                    group.common_mesh = Some(Some(mesh));
                }
                Err(Unsent::Full(_)) => return,
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_sdk::warn(&format!(
                        "a common: {why}"
                    ));
                    group.common_mesh = Some(None);
                }
            }
            if let Some(mesh) = self.kit.get(KitPart::Lamp) {
                group._lamp.draw(mesh, self.lamps.of(KState::Rest));
            }
        }
    }

    /// Plays every house that is coming in or going out.
    fn shift(&mut self, ctx: &Ctx) {
        let (set, now) = (&ctx.tuning.house_motion, ctx.clock);
        for house in self.houses.values_mut() {
            house.play(set, now);
        }
        self.leaving.retain_mut(|house| house.play(set, now));
    }

    /// Shows each town's houses near the camera and its far mesh past
    /// `town_far`. A town keeps its far mesh until its common and
    /// every house it has are built.
    fn detail(&mut self, ctx: &Ctx) {
        let land = ctx.land;
        for (id, group) in &mut self.towns {
            let c = land.sites[group.site].at;
            let d = ctx.eye.distance(Vec3::new(
                c.x,
                land.set.land.site_y,
                c.y,
            ));
            let bare = self.houses.values().any(|h| {
                h.town == *id
                    && !h.drawn
                    && h.way.is_none()
                    && !self.replaced.contains_key(&h.agent)
            });
            let built = group.common_mesh.is_some() && !bare;
            let near = d < ctx.tuning.detail.town_far && built;
            if group.near_shown != Some(near) {
                group.near_shown = Some(near);
                group.near.set_visible(near);
                group.far.set_visible(!near);
            }
        }
    }

    /// Shows what each agent is doing in its door lantern.
    fn states(&mut self, ctx: &Ctx) {
        for house in self.houses.values_mut() {
            let Some((lamp, shown, _)) = &mut house.lamp else {
                continue;
            };
            let state = ctx
                .society
                .agent(house.agent)
                .map_or(KState::Rest, |a| a.state);
            if *shown != state
                && let Some(mesh) = self.kit.get(KitPart::Lamp)
            {
                *shown = state;
                lamp.draw(mesh, self.lamps.of(state));
            }
        }
    }

    /// Turns the wheel of each engine whose agent runs a command, and
    /// sends up its steam.
    fn engines(&mut self, ctx: &Ctx) {
        let now = ctx.clock;
        for house in self.houses.values_mut() {
            let Some(engine) = &mut house.engine else {
                continue;
            };
            let running =
                ctx.society.agent(house.agent).is_some_and(|a| {
                    a.state == KState::Working && a.work == Doing::Run
                });
            let yard = house.yard;
            match (running, engine.ran) {
                (true, None) => engine.ran = Some(now),
                (false, Some(_)) => {
                    engine.ran = None;
                    for ring in &engine.rings {
                        ring.set_visible(false);
                    }
                }
                _ => {}
            }
            let Some(began) = engine.ran else {
                continue;
            };
            let x = now - began;
            engine.turn = x * CRANK_RATE;
            engine
                .wheel
                .set_transform(yard.wheel(engine.turn).scene());
            for (i, ring) in engine.rings.iter().enumerate() {
                match yard.ring(i, x) {
                    Some(at) => {
                        ring.set_transform(at.scene());
                        ring.set_visible(true);
                    }
                    None => ring.set_visible(false),
                }
            }
        }
    }

    /// Has the lanterns nearest the camera throw light after dark.
    fn spill(&mut self, ctx: &Ctx) {
        let set = &ctx.tuning.sky;
        let lumens = set.door_lumens * ctx.sky.look.candle;
        let mut lit = self
            .houses
            .values()
            .filter(|house| house.seen)
            .filter_map(|house| {
                house
                    .lamp
                    .as_ref()
                    .map(|(_, state, at)| (*at, *state))
            })
            .chain(
                self.towns
                    .values()
                    .filter(|g| g.near_shown == Some(true))
                    .map(|group| {
                        (
                            ctx.land
                                .common_frame(group.site)
                                .point(ctx.land.set.town.lamp),
                            KState::Rest,
                        )
                    }),
            )
            .filter(|(_, state)| *state != KState::Failed)
            .collect::<Vec<_>>();
        let eye = ctx.eye;
        lit.sort_by(|a, b| {
            a.0.distance_squared(eye)
                .total_cmp(&b.0.distance_squared(eye))
        });
        self.lamps.spill(&lit, lumens, set.door_reach);
    }
}

impl TownDrawn {
    fn make(ctx: &Ctx, town: &Town) -> Result<Self, String> {
        let near = Entity::make(None, PLAIN)?;
        near.set_visible(false);
        let far = Entity::make(None, PLAIN)?;
        far.set_shadows(true, true);
        let far_panes = Entity::make(Some(&far), PLAIN)?;
        far_panes.set_shadows(false, true);
        let common =
            Entity::make(Some(&near), Thing::Common(town.id).key())?;
        common
            .set_transform(ctx.land.common_frame(town.site).scene());
        common.watch(true, true);
        let lamp = Entity::make(Some(&common), PLAIN)?;
        lamp.set_transform(Frame::at(ctx.land.set.town.lamp).scene());
        lamp.set_shadows(false, true);
        Ok(Self {
            site: town.site,
            at: ctx.land.sites[town.site].at,
            near,
            common,
            common_mesh: None,
            _lamp: lamp,
            far,
            far_panes,
            far_meshes: None,
            near_shown: None,
        })
    }
}

impl House {
    /// Draws its meshes once they are all handed over, with its
    /// panes and its door lantern, and sets its part going.
    /// `replaces` is true when it takes the place of a house
    /// drawn before.
    fn finish(
        &mut self,
        ctx: &Ctx,
        kit: &Kit,
        lamps: &Lamps,
        state: KState,
        replaces: bool,
    ) {
        let Some(built) = &self.built else {
            return;
        };
        if let Some(body) = built.body.mesh() {
            self.entity.draw(body, &ctx.stock.matte);
            // An archived house answers only as a ruin.
            self.entity.watch(true, true);
        }
        if let Some((panes, lamp)) = &built.fittings {
            if let Some(mesh) = panes.mesh()
                && let Ok(entity) =
                    Entity::make(Some(&self.entity), PLAIN)
            {
                entity.draw(mesh, &ctx.stock.windows);
                entity.set_shadows(false, true);
                self.panes = Some(entity);
            }
            if let Some(mesh) = kit.get(KitPart::Lamp)
                && let Ok(entity) =
                    Entity::make(Some(&self.entity), PLAIN)
            {
                entity.set_transform(Frame::at(*lamp).scene());
                entity.draw(mesh, lamps.of(state));
                entity.set_shadows(false, true);
                self.lamp =
                    Some((entity, state, self.frame.point(*lamp)));
            }
        }
        self.drawn = true;
        match self.way {
            Some(way) => {
                let (comes, _) = way.parts();
                self.shift = Some(
                    Shift::new(comes, replaces).after(self.wait.1),
                );
            }
            None => self.entity.show_as(&mut self.seen, true),
        }
    }

    /// Plays its part a frame further, at clock time `now`. False
    /// once it is over and it goes.
    fn play(
        &mut self,
        set: &crate::shift::HouseMotion,
        now: f32,
    ) -> bool {
        let Some(shift) = &mut self.shift else {
            return true;
        };
        let part = shift.part;
        let (since, from) = match shift.began {
            Some(began) => began,
            None => {
                let began = (now + shift.wait, self.frame);
                shift.began = Some(began);
                began
            }
        };
        let t = now - since;
        if t < 0.0 {
            return true;
        }
        let replaces = shift.replaces;
        let pops = |prop: &Prop| !(prop.stays && replaces);
        let popping =
            self.props.iter().filter(|prop| pops(prop)).count();
        let over = t >= part.length(set, popping);
        if over && !part.comes() {
            return false;
        }
        let at = if over {
            self.shift = None;
            from
        } else {
            part.pose(set, t, from)
        };
        self.entity.set_transform(at.scene());
        if over || t >= part.waits(set) {
            self.entity.show_as(&mut self.seen, true);
        }
        let mut order = 0;
        for prop in &self.props {
            let rest = prop.rest;
            match (part.comes(), pops(prop)) {
                // The one of the house that comes is in sight.
                (false, false) => prop.entity.set_visible(false),
                (false, true) => {}
                // Held as it stands, whatever the house under it
                // does.
                (true, false) => {
                    let scale = at.size / from.size;
                    prop.entity.set_transform(
                        Frame {
                            at: rest.at / scale,
                            turn: rest.turn,
                            size: rest.size / scale,
                        }
                        .scene(),
                    );
                }
                (true, true) => {
                    if part != Part::Rise {
                        continue;
                    }
                    let size = if over {
                        rest.size
                    } else {
                        rest.size * set.popped(t, order)
                    };
                    order += 1;
                    prop.entity
                        .set_transform(rest.with_size(size).scene());
                }
            }
        }
        true
    }
}

impl LandData {
    /// The frame of the common of the town at `site`, about its
    /// middle.
    fn common_frame(&self, site: usize) -> Frame {
        let centre = self.sites[site].at;
        Frame::at(Vec3::new(centre.x, self.set.land.site_y, centre.y))
            .with_turn(Quat::from_rotation_y(-self.sites[site].turn))
    }
}

/// The far mesh of the town `id` on `site`, and its window panes.
fn far_mesh(
    land: &Land,
    society: &Society,
    id: TownId,
    site: usize,
) -> (Mb, Mb) {
    let style = TownStyle::of(land, site);
    let (mut mb, mut panes) = (Mb::new(), Mb::new());
    mb.at(land.common_frame(site).affine());
    common::far(&mut mb, land);
    for agent in society.agents.iter().filter(|a| a.town == id) {
        let spot = plot(land, site, agent.plot);
        let xf = glam::Affine3A::from_rotation_translation(
            Quat::from_rotation_y(spot.yaw),
            land.on_ground(spot.at),
        );
        house::far(
            &mut mb,
            &mut panes,
            xf,
            land,
            agent.id,
            &style,
            agent.plot == HALL_PLOT,
            agent.remains(&land.set.house),
        );
    }
    (mb, panes)
}
