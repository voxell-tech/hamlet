//! The stills: each a society, a view and a moment.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use hamlet_world::generator::land::Land;
use hamlet_world::generator::layout::{Route, plot};
use hamlet_world::generator::society::{
    AgentId, Pair, RoadLevel, Society, TownId,
};
use hamlet_world::generator::{GenView, Rehearsal, Traffic, house};
use hamlet_world::keeper::KState;
use hamlet_world::keeper::work::{Chore, routine};
use hamlet_world::sky::SkySettings;
use hamlet_world::{Clock, Hour, Stage};

use crate::made_up::{site_of, society};

/// One still: the society to show, where to look from, and what is
/// happening.
#[derive(Default)]
struct Shot {
    name: &'static str,
    /// Its place in a strip of stills with the same name.
    frame: Option<usize>,
    world: Society,
    view: GenView,
    night: bool,
    /// Roads with a robot part of the way along.
    trips: Vec<(Pair, AgentId, f32)>,
    /// The clock time shown, where it matters.
    at: Option<f32>,
    /// Agents put to a chore.
    chores: Vec<(AgentId, Chore)>,
    /// The hour shown, where it is not the middle of the day or of
    /// the night.
    hour: Option<f32>,
}

fn town_view(
    land: &Land,
    world: &Society,
    town: usize,
    dist: f32,
) -> GenView {
    GenView {
        focus: land.sites[world.towns[town].site].at,
        dist,
        ..default()
    }
}

fn shot_list(land: &Land) -> Vec<Shot> {
    let mut shots = Vec::new();
    let leader = |w: &Society, t: usize| w.towns[t].leader;

    let fresh = society(land, &[0], 0);
    shots.push(Shot {
        name: "01-new-town",
        view: town_view(land, &fresh, 0, 44.0),
        world: fresh,
        night: false,
        trips: vec![],
        ..default()
    });

    let young = society(land, &[5], 1);
    shots.push(Shot {
        name: "02-young-town",
        view: town_view(land, &young, 0, 56.0),
        world: young,
        night: false,
        trips: vec![],
        ..default()
    });

    // The pair that proves growth moves nothing: the same town and
    // camera, one agent and one road more.
    let before = society(land, &[11], 3);
    let mut after = before.clone();
    let newcomer = after.add_agent(land, TownId(0)).unwrap();
    after.add_road(newcomer, AgentId(4));
    for (name, world) in
        [("03-before-adding", before), ("04-after-adding", after)]
    {
        shots.push(Shot {
            name,
            view: town_view(land, &world, 0, 66.0),
            world,
            night: false,
            trips: vec![],
            ..default()
        });
    }

    let mut grown = society(land, &[26], 7);
    grown.remove_agent(AgentId(7));
    grown.merge(AgentId(3), AgentId(12));
    grown.set_state(AgentId(5), KState::Waiting);
    grown.set_state(AgentId(16), KState::Fault);
    let walker = AgentId(9);
    let trip = (Pair::new(walker, leader(&grown, 0)), walker, 0.4);
    for (name, night) in
        [("05-grown-town-day", false), ("06-grown-town-night", true)]
    {
        shots.push(Shot {
            name,
            view: town_view(land, &grown, 0, 84.0),
            world: grown.clone(),
            night,
            trips: vec![trip],
            ..default()
        });
    }

    // One town with a road of each level side by side.
    let mut levels = society(land, &[8], 0);
    let chief = leader(&levels, 0);
    for (id, level) in [
        (1, RoadLevel::Overgrown),
        (2, RoadLevel::Dirt),
        (3, RoadLevel::Paved),
        (4, RoadLevel::Rail),
        (5, RoadLevel::Dirt),
        (6, RoadLevel::Paved),
        (7, RoadLevel::Rail),
        (8, RoadLevel::Overgrown),
    ] {
        levels.set_level(Pair::new(AgentId(id), chief), level);
    }
    let at = land.sites[levels.towns[0].site].at;
    for (name, night) in [
        ("07-road-levels-day", false),
        ("08-road-levels-night", true),
    ] {
        shots.push(Shot {
            name,
            view: GenView {
                focus: at,
                dist: 40.0,
                ..default()
            },
            world: levels.clone(),
            night,
            trips: vec![(
                Pair::new(AgentId(3), chief),
                AgentId(3),
                0.5,
            )],
            ..default()
        });
    }
    for (name, level) in [
        ("09-level-overgrown", RoadLevel::Overgrown),
        ("10-level-dirt", RoadLevel::Dirt),
        ("11-level-paved", RoadLevel::Paved),
        ("12-level-rail", RoadLevel::Rail),
    ] {
        let mut world = society(land, &[8], 0);
        world.set_level(Pair::new(AgentId(2), chief), level);
        let a = plot(land, world.towns[0].site, 2);
        // Seen from the side, so no roof stands in the way.
        let side = (at - a.door()).normalize().perp();
        shots.push(Shot {
            name,
            view: GenView {
                focus: a.door().lerp(at, 0.4),
                dist: 24.0,
                pitch: 36.0,
                yaw: side.x.atan2(side.y),
            },
            world,
            night: false,
            trips: vec![],
            ..default()
        });
    }

    let several = society(land, &[14, 9, 18, 6, 11], 4);
    let middle = several
        .towns
        .iter()
        .map(|t| land.sites[t.site].at)
        .sum::<Vec2>()
        / several.towns.len() as f32;
    for (name, night) in [
        ("13-several-towns-day", false),
        ("14-several-towns-night", true),
    ] {
        shots.push(Shot {
            name,
            view: GenView {
                focus: middle,
                dist: 215.0,
                ..default()
            },
            world: several.clone(),
            night,
            trips: vec![],
            ..default()
        });
    }

    // The crossing of a road between towns and the river.
    let bridge = bridge_view(land, &several);
    shots.push(Shot {
        name: "15-bridge",
        view: bridge,
        world: several.clone(),
        night: false,
        trips: vec![],
        ..default()
    });

    let sizes = [16, 9, 12, 20, 7, 14, 10, 18, 8, 12, 15, 6];
    let many = society(land, &sizes, 4);
    shots.push(Shot {
        name: "16-the-world",
        view: GenView {
            focus: Vec2::ZERO,
            dist: 720.0,
            pitch: 38.0,
            ..default()
        },
        world: many.clone(),
        night: false,
        trips: vec![],
        ..default()
    });
    shots.push(Shot {
        name: "17-far-towns",
        view: GenView {
            focus: Vec2::ZERO,
            dist: 330.0,
            pitch: 40.0,
            ..default()
        },
        world: many,
        night: false,
        trips: vec![],
        ..default()
    });

    // A ruin and a house with a wing, close up.
    let ruin = plot(land, grown.towns[0].site, 7);
    shots.push(Shot {
        name: "18-ruin",
        view: GenView {
            focus: ruin.at,
            dist: 24.0,
            pitch: 36.0,
            ..default()
        },
        world: grown.clone(),
        night: false,
        trips: vec![],
        ..default()
    });
    let hall = land.sites[grown.towns[0].site].at;
    for (name, night) in
        [("19-common-day", false), ("20-common-night", true)]
    {
        shots.push(Shot {
            name,
            view: GenView {
                focus: hall,
                dist: 30.0,
                pitch: 38.0,
                ..default()
            },
            world: grown.clone(),
            night,
            trips: vec![trip],
            ..default()
        });
    }
    shots.extend(work_shots(land));

    // The grown town through a day, and its common at dusk from low
    // down, where the sky shows.
    for (name, hour) in [
        ("l-dawn", 6.2),
        ("l-morning", 8.0),
        ("l-dusk-early", 18.3),
        ("l-dusk", 18.9),
        ("l-dusk-late", 19.4),
    ] {
        shots.push(Shot {
            name,
            view: town_view(land, &grown, 0, 84.0),
            world: grown.clone(),
            trips: vec![trip],
            hour: Some(hour),
            ..default()
        });
    }
    // Toward the setting sun.
    for (name, hour) in [
        ("l-low-day", None),
        ("l-low-evening", Some(18.2)),
        ("l-low-dusk", Some(18.6)),
    ] {
        shots.push(Shot {
            name,
            view: GenView {
                focus: hall,
                dist: 46.0,
                pitch: 16.0,
                yaw: 0.42,
            },
            world: grown.clone(),
            hour,
            ..default()
        });
    }
    shots
}

/// The robot of an agent, as the generator seeds it.
fn robot_seed(agent: AgentId) -> u32 {
    agent.0 * 7 + 3
}

/// A close view of an agent's work yard.
fn yard_view(
    land: &Land,
    world: &Society,
    agent: AgentId,
) -> GenView {
    let a = world.agent(agent).unwrap();
    let spot = plot(land, site_of(world, a), a.plot);
    let yard = house::yard_of(land, agent, false, spot.yaw, 0);
    let (p, _) =
        yard.stand(hamlet_world::keeper::work::Station::Beat, 0.5);
    let p = Quat::from_rotation_y(spot.yaw) * p;
    GenView {
        focus: spot.at + Vec2::new(p.x, p.z) * 0.8,
        dist: 13.5,
        pitch: 30.0,
        ..default()
    }
}

/// Stills of robots working alone (docs/working-alone.md):
/// each chore over time, the walk between two chores, and a hamlet
/// at the default zoom with everyone busy.
fn work_shots(land: &Land) -> Vec<Shot> {
    let mut shots = Vec::new();
    let mut world = society(land, &[6], 2);
    let ids =
        world.citizens(TownId(0)).map(|a| a.id).collect::<Vec<_>>();
    for &id in &ids {
        world.set_state(id, KState::Working);
    }
    let star = AgentId(
        std::env::var("TOWN_STAR")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(2),
    );
    let close = yard_view(land, &world, star);
    // Room above for what goes up: steam and the kite.
    let wide = GenView {
        focus: close.focus - Vec2::Y * 1.6,
        dist: 23.0,
        ..close
    };

    let strips: [(&'static str, Chore, &[f32]); 12] = [
        ("w-think", Chore::Ponder, &[0.6, 2.2, 3.6, 5.0, 6.6, 10.2]),
        ("w-read", Chore::Study, &[1.0, 4.5, 4.7, 9.6, 10.4]),
        ("w-jot", Chore::Jot, &[1.0, 1.4, 5.6]),
        ("w-write", Chore::Write, &[0.3, 0.9, 1.5, 3.4, 4.3, 6.2]),
        ("w-run", Chore::Run, &[0.3, 0.9, 1.5, 2.1, 3.3, 4.5]),
        ("w-long-run", Chore::Run, &[7.0, 8.2, 15.4, 16.0, 39.5]),
        ("w-fetch", Chore::Fetch, &[0.4, 1.3, 2.2, 3.0, 4.6, 6.1]),
        ("w-skill", Chore::Recall, &[1.0, 3.0, 3.9]),
        (
            "w-save-skill",
            Chore::Stow,
            &[0.3, 1.1, 1.7, 2.0, 2.12, 2.8],
        ),
        ("w-custom", Chore::Tinker, &[1.3, 1.52, 1.6, 1.8]),
        ("w-reflect", Chore::Reflect, &[1.0, 4.0]),
        ("w-failed", Chore::Oops, &[0.05, 0.25, 0.75, 1.3, 1.8]),
    ];
    for (name, chore, times) in strips {
        for (k, &t) in times.iter().enumerate() {
            shots.push(Shot {
                name,
                frame: Some(k),
                world: world.clone(),
                view: if matches!(chore, Chore::Run | Chore::Fetch) {
                    wide
                } else {
                    close
                },
                at: Some(t),
                chores: vec![(star, chore)],
                ..default()
            });
        }
    }
    for (k, &t) in [0.2, 0.7, 1.1, 1.6, 1.9, 3.2].iter().enumerate() {
        shots.push(Shot {
            name: "w-done",
            frame: Some(k),
            world: world.clone(),
            view: close,
            at: Some(t),
            chores: vec![(star, Chore::Done)],
            ..default()
        });
    }

    // The round a robot keeps when all that is known is that it is
    // busy: the moments either side of the first change of chore
    // after the clock passes 20.
    let seed = robot_seed(star);
    let change = (0..600)
        .map(|i| 20.0 + i as f32 * 0.05)
        .find(|&t| routine(seed, t).1 > 20.0)
        .unwrap_or(20.0);
    for (k, dt) in
        [-1.0f32, 0.4, 1.0, 1.7, 2.4, 3.2, 4.5].iter().enumerate()
    {
        shots.push(Shot {
            name: "w-walk",
            frame: Some(k),
            world: world.clone(),
            view: close,
            at: Some(change + dt),
            ..default()
        });
    }

    // A hamlet where everyone is busy with something else.
    let cast = [
        Chore::Run,
        Chore::Fetch,
        Chore::Write,
        Chore::Ponder,
        Chore::Study,
        Chore::Recall,
        Chore::Run,
    ];
    let chores = ids
        .iter()
        .zip(cast)
        .map(|(&id, chore)| (id, chore))
        .collect::<Vec<_>>();
    for (name, dist, night) in [
        ("w-hamlet-default", 72.0, false),
        ("w-hamlet-near", 46.0, false),
        ("w-hamlet-night", 46.0, true),
    ] {
        shots.push(Shot {
            name,
            world: world.clone(),
            view: GenView {
                pitch: 40.0,
                ..town_view(land, &world, 0, dist)
            },
            night,
            at: Some(8.6),
            chores: chores.clone(),
            ..default()
        });
    }
    // The same hamlet on the round it keeps by itself, at three
    // moments.
    for (k, &t) in [31.0f32, 44.0, 57.0].iter().enumerate() {
        shots.push(Shot {
            name: "w-hamlet-round",
            frame: Some(k),
            world: world.clone(),
            view: GenView {
                pitch: 40.0,
                ..town_view(land, &world, 0, 46.0)
            },
            at: Some(t),
            ..default()
        });
    }
    shots
}

/// A view of the first place a road between towns stands over water.
fn bridge_view(land: &Land, world: &Society) -> GenView {
    for r in &world.roads {
        let end = |id: AgentId| {
            let a = world.agent(id).unwrap();
            hamlet_world::generator::layout::End {
                site: site_of(world, a),
                plot: a.plot,
            }
        };
        let route =
            Route::between(land, end(r.pair.0), end(r.pair.1));
        let wet = (0..route.raised.len())
            .filter(|&i| route.raised[i] > 0.6)
            .collect::<Vec<_>>();
        if let Some(&mid) = wet.get(wet.len() / 2) {
            let p = route.line.pts[mid];
            return GenView {
                focus: Vec2::new(p.x, p.z),
                dist: 38.0,
                pitch: 34.0,
                ..default()
            };
        }
    }
    GenView::default()
}

/// With TOWN_SHOT set: steps through the stills, saves each, then
/// exits.
#[derive(Resource)]
pub struct ShotRun {
    dir: PathBuf,
    index: usize,
    frame: u32,
    /// Stills saved so far.
    taken: usize,
}

impl ShotRun {
    /// A run that saves its stills into `dir`.
    pub fn into(dir: PathBuf) -> Self {
        ShotRun {
            dir,
            index: 0,
            frame: 0,
            taken: 0,
        }
    }
}

pub fn run_shots(
    mut commands: Commands,
    land: Res<Land>,
    mut run: ResMut<ShotRun>,
    mut world: ResMut<Society>,
    mut view: ResMut<GenView>,
    mut stage: ResMut<Stage>,
    mut clock: ResMut<Clock>,
    mut traffic: ResMut<Traffic>,
    mut rehearsal: ResMut<Rehearsal>,
    sky_set: Res<SkySettings>,
    mut hour: ResMut<Hour>,
    mut exit: MessageWriter<AppExit>,
) {
    let shots = shot_list(&land);
    let only = std::env::var("TOWN_ONLY").ok();
    while run.index < shots.len()
        && only.as_ref().is_some_and(|o| {
            !shots[run.index].name.contains(o.as_str())
        })
    {
        run.index += 1;
    }
    let Some(shot) = shots.get(run.index) else {
        // Leave time for the last file to be written.
        run.frame += 1;
        if run.frame > 40 {
            exit.write(AppExit::Success);
        }
        return;
    };
    // The first still waits for pipelines to compile; later ones only
    // for the new meshes.
    let settle = if run.taken == 0 { 220 } else { 70 };
    if run.frame == 0 {
        if *world != shot.world {
            *world = shot.world.clone();
        }
        *view = shot.view;
        stage.night =
            shot.hour.map_or(shot.night, |h| sky_set.night(h));
        hour.0 = shot.hour;
        clock.frozen = true;
        clock.t = shot.at.unwrap_or(0.5);
        traffic.trips.clear();
        rehearsal.0 = shot.chores.iter().copied().collect();
    }
    // Trips need their roads, which exist a frame after the society
    // changes.
    if run.frame == 3 {
        for &(pair, walker, part) in &shot.trips {
            traffic.send_part(pair, walker, part);
        }
    }
    run.frame += 1;
    if run.frame == settle {
        let name = match shot.frame {
            Some(k) => format!("{}-{k:02}.png", shot.name),
            None => format!("{}.png", shot.name),
        };
        let path = run.dir.join(name);
        info!("saving {}", path.display());
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if run.frame >= settle + 8 {
        run.index += 1;
        run.frame = 0;
        run.taken += 1;
    }
}
