//! Each town as the society holds it: its common, a house per agent
//! with its yard, and one plain mesh of the whole from far. What is
//! drawn is brought in line with the society only when its shape
//! changes, and a house is built again only when what it is built
//! from does. Houses are built a few a frame; one that replaces
//! another stands up as the other goes.

mod build;
mod common;
mod flag;
mod kit;
mod lights;
mod meter;
mod works;

use std::collections::{HashMap, HashSet, VecDeque};

use glam::{Quat, Vec2, Vec3};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{Mesh, SceneClick};

use crate::bridge::Bridge;
use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::upload::{Draw, Unsent};
use crate::folk::keeper::{KState, Robot, Robots};
use crate::fx::magic::Shifted;
use crate::places::house::{self, TownStyle};
use crate::places::shift::{Part, Shift, Way};
use crate::places::town::build::{Built, Recipe};
use crate::places::town::flag::Flags;
use crate::places::town::kit::{Kit, Part as KitPart};
use crate::places::town::lights::{Glow, Lamps};
use crate::places::town::meter::{Meter, Meters};
use crate::places::town::works::{Bench, Benches};
use crate::places::yard::Yard;
use crate::realm::{Ctx, Shared};
use crate::society::{Agent, AgentId, Society, Town, TownId};
use crate::sound::{Cue, Sound};
use crate::view::pick::Thing;
use crate::world::land::{Land, LandData};
use crate::world::layout::{HALL_PLOT, plot};
use crate::world::palette::Palette;

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
    /// What it is built from, and its meshes once its turn to be
    /// built comes: while they are handed over, then as they are
    /// drawn.
    recipe: Recipe,
    built: Option<Built>,
    drawn: bool,
    /// Its door lantern, with the state it shows and where it hangs
    /// in the realm.
    lamp: Option<(Entity, KState, Vec3)>,
    /// Its banner, with where it hangs in the house's frame.
    banner: Option<(Entity, Vec3)>,
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
    common_mesh: Option<Option<Handle<Mesh>>>,
    lamp: Entity,
    bench: Bench,
    /// Its budget meter, and the light it throws.
    meter: Meter,
    meter_glow: Option<Glow>,
    /// The whole as one mesh, shown far, and its window panes.
    far: Entity,
    far_panes: Entity,
    /// True while its houses are shown, false while its far mesh is,
    /// none until it is first shown.
    near_shown: Option<bool>,
}

/// The towns drawn.
#[derive(Resource)]
pub struct Towns {
    kit: Kit,
    lamps: Lamps,
    flags: Flags,
    benches: Benches,
    meters: Meters,
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
    /// The sounds of the houses whose part began this frame.
    cues: Vec<Cue>,
    /// The parts that began this frame, for the magic.
    shifts: Vec<Shifted>,
}

impl Towns {
    /// No towns yet, their kit to be built as they update.
    pub fn make(
        pal: &Palette,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        Ok(Self {
            kit: Kit::new(),
            lamps: Lamps::make(pal, draw)?,
            flags: Flags::make(draw)?,
            benches: Benches::make(pal, draw)?,
            meters: Meters::make(pal, draw)?,
            towns: HashMap::new(),
            houses: HashMap::new(),
            replaced: HashMap::new(),
            leaving: Vec::new(),
            jobs: VecDeque::new(),
            stale: HashSet::new(),
            shape: None,
            cues: Vec::new(),
            shifts: Vec::new(),
        })
    }

    /// Brings what is drawn in line with the society and with how
    /// each hamlet works, while the call has room. `taking_leave`
    /// says whether an agent's robot is taking its leave.
    pub fn update(
        &mut self,
        ctx: &Ctx,
        robots: &Robots,
        bridge: &Bridge,
        taking_leave: impl Fn(AgentId) -> bool,
        draw: &mut Draw,
    ) {
        self.lamps.light(ctx.pal, ctx.sky, ctx.clock, draw);
        if !self.kit.fill(ctx.pal, &ctx.tuning.generator.yard, draw) {
            return;
        }
        let shape = (ctx.society.shape(), ctx.land.places());
        if self.shape != Some(shape) {
            self.sync(ctx, draw);
            self.shape = Some(shape);
        }
        self.build(ctx, draw);
        self.far(ctx, draw);
        self.commons(ctx, draw);
        self.shift(ctx, &taking_leave, draw);
        self.detail(ctx, draw);
        self.states(ctx, draw);
        self.engines(robots, draw);
        self.benches(ctx, bridge, draw);
        self.wave(ctx, draw);
        self.spill(ctx, draw);
    }

    /// Turns each banner down the wind, whichever way its house
    /// faces.
    fn wave(&mut self, ctx: &Ctx, draw: &mut Draw) {
        let blown = self.flags.downwind(ctx.clock);
        let houses = self
            .houses
            .values()
            .chain(self.replaced.values())
            .chain(&self.leaving);
        for house in houses {
            if let Some((banner, at)) = house.banner {
                let turn = house.frame.turn.inverse() * blown;
                draw.place(banner, Frame::at(at).with_turn(turn));
            }
        }
    }

    /// Takes out what the society no longer holds, and draws what is
    /// new or changed.
    fn sync(&mut self, ctx: &Ctx, draw: &mut Draw) {
        let (society, land) = (ctx.society, ctx.land);
        // Towns gone, or founded anew on another site.
        self.towns.retain(|id, group| {
            let kept = society
                .town(*id)
                .is_some_and(|t| t.site == group.site);
            if !kept {
                draw.despawn(group.near);
                draw.despawn(group.far);
            }
            kept
        });
        let towns = &self.towns;
        self.houses.retain(|id, house| {
            let kept = towns.contains_key(&house.town)
                && society.agent(*id).is_some();
            if !kept {
                draw.despawn(house.entity);
            }
            kept
        });
        let houses = &self.houses;
        self.replaced.retain(|id, house| {
            let kept = houses.contains_key(id);
            if !kept {
                draw.despawn(house.entity);
            }
            kept
        });
        self.leaving.retain(|house| {
            let kept = towns.contains_key(&house.town);
            if !kept {
                draw.despawn(house.entity);
            }
            kept
        });
        self.jobs.retain(|agent| houses.contains_key(agent));
        let (founded, carried) = self.towns(ctx, draw);
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
            self.house(ctx, agent, town, sig, founded, carried, draw);
        }
    }

    /// Makes the groups and the common of each town that is new, and
    /// moves the common of one set down elsewhere. The towns first
    /// drawn, and those carried.
    fn towns(
        &mut self,
        ctx: &Ctx,
        draw: &mut Draw,
    ) -> (HashSet<TownId>, HashSet<TownId>) {
        let land = ctx.land;
        let (mut founded, mut carried) =
            (HashSet::new(), HashSet::new());
        for town in &ctx.society.towns {
            let centre = land.sites[town.site].at;
            if let Some(group) = self.towns.get_mut(&town.id) {
                if group.at != centre {
                    group.at = centre;
                    draw.place(
                        group.common,
                        land.common_frame(town.site),
                    );
                    self.stale.insert(town.id);
                    carried.insert(town.id);
                }
                continue;
            }
            let group = TownDrawn::make(
                ctx,
                town,
                (&self.benches, &self.meters),
                &self.kit,
                draw,
            );
            self.towns.insert(town.id, group);
            self.stale.insert(town.id);
            founded.insert(town.id);
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
        draw: &mut Draw,
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
                if let Some(old) = self.replaced.insert(agent.id, was)
                {
                    draw.despawn(old.entity);
                }
            } else {
                draw.despawn(was.entity);
            }
        }
        let frame = Frame::at(land.on_ground(spot.at))
            .with_turn(Quat::from_rotation_y(spot.yaw));
        let entity = draw
            .spawn(Some(group.near), Thing::House(agent.id).key());
        draw.place(entity, frame);
        draw.show(entity, false);
        let mut made = House {
            agent: agent.id,
            town: town.id,
            sig,
            entity,
            frame,
            recipe: Recipe {
                agent: agent.id,
                site: town.site,
                hall,
                leader: sig.leader,
                remains: agent.remains(&land.set.house),
                wings: agent.wings,
                yard,
            },
            built: None,
            drawn: false,
            lamp: None,
            banner: None,
            props: Vec::new(),
            yard,
            engine: None,
            shift: None,
            way,
            wait,
            seen: false,
        };
        if !agent.archived {
            self.furnish(ctx, &mut made, &yard, draw);
        }
        if let Some((at, kind, scale)) =
            house::garden_tree(land, agent.id, &yard)
            && !hall
            && let Some(mesh) = self.kit.get(KitPart::Tree(kind))
        {
            let tree = draw.spawn(Some(made.entity), PLAIN);
            let rest = Frame::at(at).with_size(Vec3::splat(scale));
            draw.place(tree, rest);
            draw.draw(tree, mesh, &ctx.stock.matte);
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
    fn furnish(
        &self,
        ctx: &Ctx,
        house: &mut House,
        yard: &Yard,
        draw: &mut Draw,
    ) {
        let kit = &self.kit;
        let (Some(desk_mesh), Some(engine_mesh)) =
            (kit.get(KitPart::Desk), kit.get(KitPart::Engine))
        else {
            return;
        };
        let mut part = |mesh, rest: Frame, under| {
            let entity = draw.spawn(Some(under), PLAIN);
            draw.place(entity, rest);
            draw.draw(entity, mesh, &ctx.stock.matte);
            entity
        };
        let desk = part(desk_mesh, yard.desk_at(), house.entity);
        let engine =
            part(engine_mesh, yard.engine_at(), house.entity);
        let wheel = kit
            .get(KitPart::Wheel)
            .map(|mesh| part(mesh, yard.wheel(0.0), engine));
        // Steam shows the light of work.
        if let (Some(wheel), Some(ring_mesh)) =
            (wheel, kit.get(KitPart::Ring))
        {
            let rings = [(); 2].map(|_| {
                let ring = draw.spawn(Some(house.entity), PLAIN);
                draw.draw(
                    ring,
                    ring_mesh,
                    self.lamps.of(KState::Working),
                );
                draw.shadows(ring, false, false);
                draw.show(ring, false);
                ring
            });
            house.engine = Some(Engine {
                wheel,
                rings,
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

    /// Builds the houses waiting, one at a time and nearest the
    /// camera first, while the frame has time.
    fn build(&mut self, ctx: &Ctx, draw: &mut Draw) {
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
        while draw.left() {
            let Some(&agent) = self.jobs.front() else {
                break;
            };
            let Some(house) = self.houses.get_mut(&agent) else {
                self.jobs.pop_front();
                continue;
            };
            let recipe = house.recipe;
            let built = house.built.get_or_insert_with(|| {
                Built::of(ctx.land, &self.flags.set, recipe)
            });
            if !built.hand(draw) {
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
                &self.flags,
                state,
                old.is_some(),
                draw,
            );
            match (old, house.way) {
                (Some(mut old), Some(way)) => {
                    let (_, goes) = way.parts();
                    old.shift = Some(
                        Shift::new(goes, true).after(house.wait.0),
                    );
                    self.leaving.push(old);
                }
                (Some(old), None) => draw.despawn(old.entity),
                (None, _) => {}
            }
        }
    }

    /// Builds the far mesh of each town that changed.
    fn far(&mut self, ctx: &Ctx, draw: &mut Draw) {
        let (land, society) = (ctx.land, ctx.society);
        let stale = self.stale.iter().copied().collect::<Vec<_>>();
        for id in stale {
            if !draw.left() {
                return;
            }
            let Some(group) = self.towns.get_mut(&id) else {
                self.stale.remove(&id);
                continue;
            };
            let (mb, panes) = far_mesh(land, society, id, group.site);
            let (body, panes) =
                match (mb.upload(draw), panes.upload(draw)) {
                    (Ok(body), Ok(panes)) => (body, panes),
                    (Err(Unsent::Refused(why)), _)
                    | (_, Err(Unsent::Refused(why))) => {
                        hamlet_engine_bevy::warn(&format!(
                            "a town from far: {why}"
                        ));
                        self.stale.remove(&id);
                        continue;
                    }
                    _ => return,
                };
            draw.draw(group.far, &body, &ctx.stock.matte);
            draw.draw(group.far_panes, &panes, &ctx.stock.windows);
            self.stale.remove(&id);
        }
    }

    /// Builds the common of each town that has none yet.
    fn commons(&mut self, ctx: &Ctx, draw: &mut Draw) {
        for group in self.towns.values_mut() {
            if group.common_mesh.is_some() {
                continue;
            }
            if !draw.left() {
                return;
            }
            let style = TownStyle::of(ctx.land, group.site);
            match common::mesh(ctx.land, group.site, &style)
                .upload(draw)
            {
                Ok(mesh) => {
                    draw.draw(group.common, &mesh, &ctx.stock.matte);
                    group.common_mesh = Some(Some(mesh));
                }
                Err(Unsent::Full(_)) => return,
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_bevy::warn(&format!(
                        "a common: {why}"
                    ));
                    group.common_mesh = Some(None);
                }
            }
            if let Some(mesh) = self.kit.get(KitPart::Lamp) {
                draw.draw(
                    group.lamp,
                    mesh,
                    self.lamps.of(KState::Rest),
                );
            }
        }
    }

    /// Plays every house that is coming in or going out.
    fn shift(
        &mut self,
        ctx: &Ctx,
        taking_leave: &impl Fn(AgentId) -> bool,
        draw: &mut Draw,
    ) {
        let (set, now) = (&ctx.tuning.house_motion, ctx.clock);
        let mut begun = Vec::new();
        for house in self.houses.values_mut() {
            let leaves = taking_leave(house.agent);
            house.play(set, now, leaves, &mut begun, draw);
        }
        self.leaving.retain_mut(|house| {
            let leaves = taking_leave(house.agent);
            let playing =
                house.play(set, now, leaves, &mut begun, draw);
            if !playing {
                draw.despawn(house.entity);
            }
            playing
        });
        // A house that rises is heard standing up, and one that falls
        // shuddering, or its motes setting out for the house it was
        // merged into.
        for (agent, part, began, from) in begun {
            self.shifts.push(Shifted {
                agent,
                part,
                began,
                from,
            });
            let at = from.at;
            let sound = match part {
                Part::Rise => Sound::HouseRise,
                Part::Fall
                    if ctx
                        .society
                        .agent(agent)
                        .is_some_and(|gone| gone.into.is_some()) =>
                {
                    Sound::MergeAway
                }
                Part::Fall => Sound::HouseShudder,
                Part::Clear | Part::Rest => continue,
            };
            self.cues.push(Cue {
                sound,
                at,
                turn: agent.0,
            });
        }
    }

    /// Shows each town's houses near the camera and its far mesh past
    /// `town_far`. A town keeps its far mesh until its common and
    /// every house it has are built.
    fn detail(&mut self, ctx: &Ctx, draw: &mut Draw) {
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
                draw.show(group.near, near);
                draw.show(group.far, !near);
            }
        }
    }

    /// Shows what each agent is doing in its door lantern.
    fn states(&mut self, ctx: &Ctx, draw: &mut Draw) {
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
                draw.draw(*lamp, mesh, self.lamps.of(state));
            }
        }
    }

    /// Turns the wheel of each engine whose robot works it, from when
    /// the robot reached it, and sends up its steam.
    fn engines(&mut self, robots: &Robots, draw: &mut Draw) {
        for house in self.houses.values_mut() {
            let Some(engine) = &mut house.engine else {
                continue;
            };
            let yard = house.yard;
            let ran = robots.robot(house.agent).and_then(Robot::ran);
            if ran.is_none() && engine.ran.is_some() {
                for ring in engine.rings {
                    draw.show(ring, false);
                }
            }
            engine.ran = ran;
            let Some(x) = ran else {
                continue;
            };
            engine.turn = x * CRANK_RATE;
            draw.place(engine.wheel, yard.wheel(engine.turn));
            for (i, ring) in engine.rings.into_iter().enumerate() {
                match yard.ring(i, x) {
                    Some(at) => {
                        draw.place(ring, at);
                        draw.show(ring, true);
                    }
                    None => draw.show(ring, false),
                }
            }
        }
    }

    /// Has every lantern of a town shown near throw light after dark,
    /// the nearest the camera first while there are lights to give.
    fn spill(&mut self, ctx: &Ctx, draw: &mut Draw) {
        let (set, land) = (&ctx.tuning.sky, ctx.land);
        let candle = ctx.sky.look.candle;
        let lumens = set.door_lumens * candle;
        let door = |at, state| {
            Glow::lantern(at, state, lumens, set.door_reach)
        };
        let towns = &self.towns;
        let doors = self
            .houses
            .values()
            .filter(|house| {
                house.seen
                    && towns.get(&house.town).is_some_and(|group| {
                        group.near_shown == Some(true)
                    })
            })
            .filter_map(|house| house.lamp.as_ref())
            .filter(|(_, state, _)| *state != KState::Failed)
            .map(|(_, state, at)| door(*at, *state));
        let mut commons = Vec::new();
        for group in
            towns.values().filter(|g| g.near_shown == Some(true))
        {
            let frame = land.common_frame(group.site);
            commons.push(door(
                frame.point(land.set.town.lamp),
                KState::Rest,
            ));
            commons.extend(group.meter_glow);
            commons.extend(self.benches.glow(
                &group.bench,
                &frame,
                candle,
            ));
        }
        let lit = doors.chain(commons).collect();
        self.lamps.spill(lit, ctx.eye, draw);
    }

    /// Keeps each common's workbench as its hamlet works now, and its
    /// meter showing its budget.
    fn benches(
        &mut self,
        ctx: &Ctx,
        bridge: &Bridge,
        draw: &mut Draw,
    ) {
        let dt = self.benches.tick(ctx.clock);
        for group in self.towns.values_mut() {
            self.benches.show(&mut group.bench, bridge, dt, draw);
            let frame = ctx.land.common_frame(group.site);
            group.meter_glow = self.meters.show(
                &mut group.meter,
                ctx,
                bridge,
                &frame,
                draw,
            );
        }
    }
}

impl TownDrawn {
    fn make(
        ctx: &Ctx,
        town: &Town,
        (benches, meters): (&Benches, &Meters),
        kit: &Kit,
        draw: &mut Draw,
    ) -> Self {
        let near = draw.spawn(None, PLAIN);
        draw.show(near, false);
        // From far, the whole town answers as its common.
        let far = draw.spawn(None, Thing::Common(town.id).key());
        draw.shadows(far, true, true);
        draw.watch(far);
        let far_panes = draw.spawn(Some(far), PLAIN);
        draw.shadows(far_panes, false, true);
        let common =
            draw.spawn(Some(near), Thing::Common(town.id).key());
        draw.place(common, ctx.land.common_frame(town.site));
        draw.watch(common);
        let lamp = draw.spawn(Some(common), PLAIN);
        draw.place(lamp, Frame::at(ctx.land.set.town.lamp));
        draw.shadows(lamp, false, true);
        let bench = benches
            .bench(ctx.land, kit, ctx.stock, town.id, common, draw);
        let meter = meters
            .meter(ctx.land, kit, ctx.stock, town.id, common, draw);
        Self {
            meter,
            meter_glow: None,
            site: town.site,
            at: ctx.land.sites[town.site].at,
            near,
            common,
            common_mesh: None,
            lamp,
            bench,
            far,
            far_panes,
            near_shown: None,
        }
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
        flags: &Flags,
        state: KState,
        replaces: bool,
        draw: &mut Draw,
    ) {
        let Some(built) = &self.built else {
            return;
        };
        if let Some((cloth, at)) = &built.banner
            && let Some(mesh) = cloth.mesh()
        {
            match flags.material(
                house::BANNER.x,
                self.frame.point(*at),
                draw,
            ) {
                Ok(material) => {
                    let entity = draw.spawn(Some(self.entity), PLAIN);
                    draw.place(entity, Frame::at(*at));
                    draw.draw(entity, mesh, &material);
                    draw.shadows(entity, false, true);
                    self.banner = Some((entity, *at));
                }
                Err(why) => {
                    hamlet_engine_bevy::warn(&format!(
                        "a banner: {why}"
                    ));
                }
            }
        }
        if let Some(body) = built.body.mesh() {
            draw.draw(self.entity, body, &ctx.stock.matte);
            // An archived house answers only as a ruin.
            draw.watch(self.entity);
        }
        if let Some((panes, lamp)) = &built.fittings {
            if let Some(mesh) = panes.mesh() {
                let entity = draw.spawn(Some(self.entity), PLAIN);
                draw.draw(entity, mesh, &ctx.stock.windows);
                draw.shadows(entity, false, true);
            }
            if let Some(mesh) = kit.get(KitPart::Lamp) {
                let entity = draw.spawn(Some(self.entity), PLAIN);
                draw.place(entity, Frame::at(*lamp));
                draw.draw(entity, mesh, lamps.of(state));
                draw.shadows(entity, false, true);
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
            None => self.reveal(draw),
        }
    }

    /// Shows it, unless it is shown already.
    fn reveal(&mut self, draw: &mut Draw) {
        if !self.seen {
            self.seen = true;
            draw.show(self.entity, true);
        }
    }

    /// Plays its part a frame further, at clock time `now`. False
    /// once it is over and it goes.
    /// `taking_leave` is true while its agent's robot takes its
    /// leave.
    fn play(
        &mut self,
        set: &crate::places::shift::HouseMotion,
        now: f32,
        taking_leave: bool,
        begun: &mut Vec<(AgentId, Part, f32, Frame)>,
        draw: &mut Draw,
    ) -> bool {
        let Some(shift) = &mut self.shift else {
            return true;
        };
        let part = shift.part;
        let (since, from) = match shift.began {
            Some(began) => began,
            None => {
                // A house stands until its robot has taken its leave.
                if taking_leave
                    && matches!(part, Part::Fall | Part::Rest)
                {
                    return true;
                }
                let began = (now + shift.wait, self.frame);
                shift.began = Some(began);
                begun.push((self.agent, part, began.0, self.frame));
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
        draw.place(self.entity, at);
        if over || t >= part.waits(set) {
            self.reveal(draw);
        }
        let mut order = 0;
        for prop in &self.props {
            let rest = prop.rest;
            match (part.comes(), pops(prop)) {
                // The one of the house that comes is in sight.
                (false, false) => draw.show(prop.entity, false),
                (false, true) => {}
                // Held as it stands, whatever the house under it
                // does.
                (true, false) => {
                    let scale = at.size / from.size;
                    draw.place(
                        prop.entity,
                        Frame {
                            at: rest.at / scale,
                            turn: rest.turn,
                            size: rest.size / scale,
                        },
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
                    draw.place(prop.entity, rest.with_size(size));
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

/// Brings the towns drawn in line with the society, and switches the
/// mode of a hamlet whose clockwork is clicked.
pub fn build_towns(
    mut towns: ResMut<Towns>,
    robots: Res<Robots>,
    shared: Shared,
    bridge: Res<Bridge>,
    mut clicks: MessageReader<SceneClick>,
    keys: Query<&SceneKey>,
    mut cues: MessageWriter<Cue>,
    mut shifted: MessageWriter<Shifted>,
    mut draw: Draw,
) {
    for click in clicks.read() {
        if let Ok(key) = keys.get(click.0.entity)
            && works::switch(key.0, &bridge, &mut draw.commands)
        {
            cues.write(Cue {
                sound: Sound::Clockwork,
                at: click.0.point,
                turn: key.0,
            });
        }
    }
    let floor = shared.floor();
    towns.update(
        &shared.ctx(&floor),
        &robots,
        &bridge,
        |agent| robots.taking_leave(agent),
        &mut draw,
    );
    cues.write_batch(towns.cues.drain(..));
    shifted.write_batch(towns.shifts.drain(..));
}
