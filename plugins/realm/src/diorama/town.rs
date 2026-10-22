//! The town of a template as the realm would found it on a land of
//! its own, grown from the template's name: the common, a house, a
//! yard and a robot for every agent with the mayor's hall first, the
//! roads between them, a few trees and the plinth, in the common's
//! frame with the ground of the town at naught.

use std::f32::consts::TAU;

use glam::{Affine3A, Quat, Vec2, Vec3, Vec3Swizzles};
use hamlet_engine_sdk::scene::{MeshData, Pbr};

use crate::build::frame::Frame;
use crate::build::mesh::{Mb, Rng};
use crate::build::stock::Stock;
use crate::diorama::DioramaSettings;
use crate::diorama::plinth::Plinth;
use crate::diorama::wire::{Look, Stage, Sun};
use crate::folk::keeper::Statue;
use crate::places::house::{self, TownStyle};
use crate::places::road::laid_in_town;
use crate::places::town::build::{FlagSettings, Made, Recipe};
use crate::places::town::common;
use crate::places::town::kit::Part;
use crate::places::town::lights::Lamps;
use crate::society::{Pair, RoadLevel, Society};
use crate::tuning::WorldTuning;
use crate::world::flora;
use crate::world::land::Land;
use crate::world::layout::{HALL_PLOT, plot};
use crate::world::palette::Palette;

/// The looks the town is drawn in, by their places in
/// [`Stage::looks`].
const MATTE: usize = 0;
const BRASS: usize = 1;
const PANES: usize = 2;
const GLASS: usize = 3;
const LIGHTS: usize = 4;
const LOOKS: usize = 5;

/// The town, built.
pub struct Town {
    pub stage: Stage,
    /// The meshes of each thing by their looks, the things in the
    /// order they are best seen arriving.
    pub things: Vec<Vec<(u8, MeshData)>>,
}

/// One thing being built: a mesh for each look, in the town's frame.
#[derive(Default)]
struct Thing([Mb; LOOKS]);

impl Thing {
    fn put(&mut self, look: usize, mesh: &Mb, at: Affine3A) {
        self.0[look].put(mesh, at, Vec3::ONE);
    }

    /// Its meshes as the engine takes them, those with nothing left
    /// out, and the height of its highest point.
    fn data(self) -> (Vec<(u8, MeshData)>, f32) {
        let mut top = 0.0f32;
        let meshes = self
            .0
            .into_iter()
            .enumerate()
            .filter(|(_, mb)| !mb.is_empty())
            .map(|(look, mb)| {
                let data = mb.data();
                for y in data.positions.iter().skip(1).step_by(3) {
                    top = top.max(*y);
                }
                (look as u8, data)
            })
            .collect();
        (meshes, top)
    }
}

impl Town {
    /// The town of `template` with its mayor and `agents` more, or
    /// as many as it has plots for.
    pub fn build(
        tuning: &WorldTuning,
        pal: Palette,
        set: &DioramaSettings,
        template: &str,
        agents: u32,
    ) -> Result<Self, String> {
        let mut land = Land::new(
            seed_of(template),
            tuning.generator.clone(),
            pal,
        );
        let mut society = Society::default();
        let founded = society.found_town(&mut land);
        for _ in 0..agents {
            if society.add_agent(&land, founded).is_none() {
                break;
            }
        }
        let town = society
            .town(founded)
            .ok_or("the town was not founded")?
            .clone();
        let site = town.site;
        let (pal, layout) = (&land.pal, &land.set.town);
        let middle = land.common_frame(site);
        let local = middle.affine().inverse();
        let style = TownStyle::of(&land, site);
        let flag = FlagSettings::read()?;
        let kit = |part: Part| part.build(pal, &land.set.yard);
        let lamp = kit(Part::Lamp);

        let mut common = Thing::default();
        common.put(
            MATTE,
            &common::mesh(&land, site, &style),
            Affine3A::IDENTITY,
        );
        common.put(GLASS, &lamp, Frame::at(layout.lamp).affine());
        let mut things = vec![common];
        let mut homes = Vec::new();
        let (desk, engine, wheel) =
            (kit(Part::Desk), kit(Part::Engine), kit(Part::Wheel));
        // The leader first, then the rest in the order they came.
        for agent in society.citizens(founded) {
            let mut home = Thing::default();
            let spot = plot(&land, site, agent.plot);
            let hall = agent.plot == HALL_PLOT;
            let leader = agent.id == town.leader;
            let yard = house::yard_of(
                &land,
                agent.id,
                hall,
                spot.yaw,
                agent.wings,
            );
            let made = Made::of(
                &land,
                &flag,
                Recipe {
                    agent: agent.id,
                    site,
                    hall,
                    leader,
                    remains: None,
                    wings: agent.wings,
                    yard,
                },
                None,
            );
            let at = local
                * Frame::at(land.on_ground(spot.at))
                    .with_turn(Quat::from_rotation_y(spot.yaw))
                    .affine();
            home.put(MATTE, &made.body, at);
            if let Some((panes, hung)) = made.fittings {
                home.put(PANES, &panes, at);
                home.put(GLASS, &lamp, at * Frame::at(hung).affine());
            }
            let engine_at = at * yard.engine_at().affine();
            home.put(MATTE, &desk, at * yard.desk_at().affine());
            home.put(MATTE, &engine, engine_at);
            home.put(
                MATTE,
                &wheel,
                engine_at * yard.wheel(0.0).affine(),
            );
            if let Some((tree_at, kind, scale)) =
                house::garden_tree(&land, agent.id, &yard)
                && !hall
            {
                let tree =
                    Frame::at(tree_at).with_size(Vec3::splat(scale));
                home.put(
                    MATTE,
                    &kit(Part::Tree(kind)),
                    at * tree.affine(),
                );
            }
            let robot = Statue::on_porch(
                &land,
                agent.id,
                leader,
                &spot,
                set.robot_scale,
            );
            home.put(MATTE, &robot.matte, local);
            home.put(BRASS, &robot.brass, local);
            home.put(LIGHTS, &robot.lights, local);
            if !leader
                && let Some((surface, groove)) = laid_in_town(
                    &land,
                    &society,
                    Pair::new(agent.id, town.leader),
                    RoadLevel::Dirt,
                )
            {
                home.put(MATTE, &surface, local);
                home.0[MATTE].put(&groove, local, pal.bed);
            }
            homes.push(
                local.transform_point3(land.on_ground(spot.at)).xz(),
            );
            things.push(home);
        }

        let reach = homes
            .iter()
            .map(|home| home.length() + layout.house_radius)
            .fold(layout.plaza_radius, f32::max);
        let radius = reach + set.margin;
        let ground = |p: Vec2| {
            let world = middle.point(Vec3::new(p.x, 0.0, p.y));
            land.surface(world.xz()) - middle.at.y
        };
        things.push(trees(
            &land, set, template, &homes, reach, radius, ground,
        ));
        let plinth = Plinth::of(&land, middle, radius, set);
        let mut base = Thing::default();
        base.put(MATTE, &plinth.top, Affine3A::IDENTITY);
        base.put(MATTE, &plinth.sides, Affine3A::IDENTITY);
        base.put(BRASS, &plinth.brass, Affine3A::IDENTITY);
        // The plinth first, so the town is set on it as it comes.
        things.insert(0, base);

        let mut height = 0.0f32;
        let things = things
            .into_iter()
            .map(|thing| {
                let (meshes, top) = thing.data();
                height = height.max(top);
                meshes
            })
            .collect();
        Ok(Self {
            stage: stage(tuning, pal, set, [reach, radius], height),
            things,
        })
    }
}

/// A few trees on the grass about the houses, off them.
fn trees(
    land: &Land,
    set: &DioramaSettings,
    template: &str,
    homes: &[Vec2],
    reach: f32,
    radius: f32,
    ground: impl Fn(Vec2) -> f32,
) -> Thing {
    let mut thing = Thing::default();
    let kinds = (0..flora::KINDS)
        .map(|kind| flora::mesh(&land.pal, kind))
        .collect::<Vec<_>>();
    let mut rng = Rng::seeded(&format!("diorama {template}"));
    let mut planted = 0;
    for _ in 0..set.trees * 8 {
        if planted == set.trees {
            break;
        }
        let at = Vec2::from_angle(rng.f() * TAU)
            * rng.range(reach - 1.0, radius - 1.2);
        if homes.iter().any(|home| home.distance(at) < 4.6) {
            continue;
        }
        let kind = ((rng.f() * flora::KINDS as f32) as usize)
            .min(flora::KINDS - 1);
        let tree = Frame::at(Vec3::new(at.x, ground(at), at.y))
            .with_turn(Quat::from_rotation_y(rng.f() * TAU))
            .with_size(Vec3::splat(rng.range(0.65, 1.05)));
        thing.put(MATTE, &kinds[kind], tree.affine());
        planted += 1;
    }
    thing
}

/// The looks, the plinth's size and the light of the realm's day.
fn stage(
    tuning: &WorldTuning,
    pal: &Palette,
    set: &DioramaSettings,
    [reach, radius]: [f32; 2],
    height: f32,
) -> Stage {
    let day = &tuning.looks.day;
    let mut looks = vec![None; LOOKS];
    for (at, pbr, shadows) in [
        (MATTE, Stock::matte(), true),
        (BRASS, Stock::brass(), true),
        (PANES, Stock::pane(pal, day.candle), false),
        (GLASS, Lamps::glass(pal), false),
        (LIGHTS, Statue::lights_look(), false),
    ] {
        looks[at] = Some(Look::of(pbr, shadows));
    }
    let sun =
        |color: Vec3, lux: f32, toward: Vec3, shadows: bool| Sun {
            color: color.to_array(),
            lux,
            toward: toward.to_array(),
            shadows,
        };
    Stage {
        radius,
        outer: radius + set.lip + set.brass,
        reach,
        depth: set.soil + set.base,
        height,
        looks: looks.into_iter().flatten().collect(),
        sun: sun(day.sun, day.sun_lux, day.sun_dir, true),
        fill: sun(day.fill, day.fill_lux, day.fill_dir(), false),
        ambient: (day.ambient.to_array(), day.ambient_nits),
        exposure: tuning.camera.lens(day, radius).exposure,
    }
}

impl Look {
    /// `pbr`, casting shadows when `shadows`.
    fn of(pbr: Pbr, shadows: bool) -> Self {
        let rgb = |(r, g, b, _): (f32, f32, f32, f32)| [r, g, b];
        Self {
            base: rgb(pbr.base_color),
            emissive: rgb(pbr.emissive),
            roughness: pbr.roughness,
            metallic: pbr.metallic,
            shadows,
        }
    }
}

/// The seed of the land the town of `template` is founded on: the
/// same for the same name.
fn seed_of(template: &str) -> u64 {
    template.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
