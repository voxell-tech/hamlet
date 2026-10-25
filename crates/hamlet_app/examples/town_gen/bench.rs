//! Frame times for worlds of several sizes and for the lighting
//! effects.

#![expect(
    clippy::print_stdout,
    reason = "it prints its measurements"
)]

use std::time::{Duration, Instant};

use bevy::prelude::*;
use hamlet_realm::generator::land::Land;
use hamlet_realm::generator::noise::key;
use hamlet_realm::generator::society::Society;
use hamlet_realm::generator::{
    GeneratorOptions, GeneratorView, Tally, Trips,
};
use hamlet_realm::sky::SkySettings;
use hamlet_realm::{Clock, Hour, Stage};

use crate::made_up::{effects, society};

/// With TOWN_BENCH set: measures frame time with vertical sync off,
/// for worlds of three sizes.
#[derive(Resource)]
pub struct BenchRun {
    /// True when it times the lighting effects, false for the sizes.
    fx: bool,
    /// Frames let pass before a phase is measured.
    warm: u32,
    /// Frames a phase is measured over.
    measured: u32,
    phase: usize,
    frame: u32,
    started: Option<Instant>,
    /// The moment the world was last replaced, and the milliseconds
    /// it then took to be built.
    replaced: Option<Instant>,
    built: Option<f64>,
}

impl BenchRun {
    /// A run that times the lighting effects if `fx`, the sizes
    /// otherwise.
    pub fn of(fx: bool) -> Self {
        BenchRun {
            fx,
            warm: 240,
            measured: 900,
            phase: 0,
            frame: 0,
            started: None,
            replaced: None,
            built: None,
        }
    }
}

/// Time the main thread spends in a frame's own schedules, which
/// leaves out drawing and waiting for the display.
#[derive(Resource, Default)]
pub struct MainTime {
    began: Option<Instant>,
    total: Duration,
    /// The longest frame since this was last cleared.
    slowest: Duration,
}

pub fn main_began(mut time: ResMut<MainTime>) {
    let now = Instant::now();
    if let Some(last) = time.began {
        time.slowest = time.slowest.max(now - last);
    }
    time.began = Some(now);
}

pub fn main_ended(mut time: ResMut<MainTime>) {
    if let Some(began) = time.began {
        time.total += began.elapsed();
    }
}

pub fn run_bench(
    land: Res<Land>,
    tally: Res<Tally>,
    mut main: ResMut<MainTime>,
    mut run: ResMut<BenchRun>,
    mut world: ResMut<Society>,
    mut view: ResMut<GeneratorView>,
    mut stage: ResMut<Stage>,
    mut clock: ResMut<Clock>,
    mut traffic: ResMut<Trips>,
    mut options: ResMut<GeneratorOptions>,
    mut sky_set: ResMut<SkySettings>,
    mut hour: ResMut<Hour>,
    mut exit: MessageWriter<AppExit>,
) {
    let small = [8usize];
    let medium = [14, 9, 18, 6, 11];
    let large = [20usize; 19];
    let huge = [24usize; 30];
    // Name, town sizes, camera distance, night, robots drawn.
    let phases: [(&str, &[usize], f32, bool, bool); 13] = [
        ("warm-up, not a measurement", &small, 70.0, false, true),
        ("small, town view, day", &small, 70.0, false, true),
        ("small, town view, night", &small, 70.0, true, true),
        ("medium, town view, day", &medium, 70.0, false, true),
        ("medium, several towns, day", &medium, 215.0, false, true),
        ("large, town view, day", &large, 70.0, false, true),
        ("large, town view, night", &large, 70.0, true, true),
        ("large, several towns, day", &large, 215.0, false, true),
        ("large, whole world, day", &large, 720.0, false, true),
        (
            "large, town view, day, no robots",
            &large,
            70.0,
            false,
            false,
        ),
        (
            "large, whole world, day, no robots",
            &large,
            720.0,
            false,
            false,
        ),
        ("huge, town view, day", &huge, 70.0, false, true),
        ("huge, whole world, day", &huge, 720.0, false, true),
    ];
    // With TOWN_BENCH=fx: the large world's first town at one hour
    // after another, with one lighting effect at a time. Name, hour,
    // effects.
    let lit: [(&str, f32, &str); 9] = [
        ("warm-up, not a measurement", 12.5, ""),
        ("day", 12.5, ""),
        ("day, occlusion", 12.5, "occlusion"),
        ("day, sky", 12.5, "atmosphere"),
        ("dusk", 18.6, ""),
        ("dusk, shafts", 18.6, "shafts"),
        ("dusk, sky", 18.6, "atmosphere"),
        ("night", 0.5, ""),
        ("night, no lantern light", 0.5, "nolamps"),
    ];
    let phase = if run.fx {
        lit.get(run.phase).map(|&(name, hour, fx)| {
            (name, &large[..], 70.0, hour, true, fx)
        })
    } else {
        phases.get(run.phase).map(
            |&(name, sizes, dist, night, robots)| {
                (name, sizes, dist, sky_set.held(night), robots, "")
            },
        )
    };
    let Some((name, sizes, dist, at, robots, fx)) = phase else {
        exit.write(AppExit::Success);
        return;
    };
    // This system waits for the world to be built, so its second
    // frame is the first with everything in place.
    if run.frame == 1
        && let Some(replaced) = run.replaced.take()
    {
        run.built = Some(replaced.elapsed().as_secs_f64() * 1000.0);
    }
    if run.frame == 0 {
        let wanted = society(&land, sizes, 4);
        run.built = None;
        if *world != wanted {
            *world = wanted;
            run.replaced = Some(Instant::now());
            main.slowest = Duration::ZERO;
        }
        if options.robots != robots {
            options.robots = robots;
            world.set_changed();
        }
        *view = GeneratorView {
            focus: if dist > 300.0 {
                Vec2::ZERO
            } else {
                land.sites[world.towns[0].site].at
            },
            dist,
            ..default()
        };
        // The hour is held, so a timing does not depend on when it
        // was taken.
        hour.0 = Some(at);
        stage.night = sky_set.is_night(at);
        if run.fx {
            *sky_set = effects(fx);
        }
        clock.frozen = false;
        traffic.trips.clear();
    }
    // Keep a few messages on the road, as a working town has.
    if run.frame % 120 == 5 {
        let roads = world.roads.len();
        for k in 0..6u32 {
            let r = &world.roads
                [(key(&[run.frame, k]) % roads as u32) as usize];
            traffic.send(r.pair, r.pair.0);
        }
    }
    run.frame += 1;
    if run.frame == run.warm {
        run.started = Some(Instant::now());
        main.total = Duration::ZERO;
    }
    if run.frame == run.warm + run.measured {
        let ms = run.started.unwrap().elapsed().as_secs_f64()
            * 1000.0
            / run.measured as f64;
        let main_ms =
            main.total.as_secs_f64() * 1000.0 / run.measured as f64;
        println!(
            "bench: {name}: {ms:.2} ms per frame ({:.0} fps), \
             {main_ms:.3} ms of it on the main thread; {} towns, {} \
             houses, {} robots, {} roads of {:.0} units, {} trees; \
             triangles: houses {}, roads {}; megabytes: houses \
             {:.1}, roads {:.1}{}",
            1000.0 / ms,
            tally.towns,
            tally.houses,
            tally.robots,
            tally.roads,
            tally.road_length,
            tally.trees,
            tally.house_tris,
            tally.road_tris,
            tally.house_bytes as f64 / 1e6,
            tally.road_bytes as f64 / 1e6,
            run.built.map_or(String::new(), |ms| format!(
                "; built in {ms:.0} ms, slowest frame since {:.0} ms",
                main.slowest.as_secs_f64() * 1000.0
            )),
        );
        run.phase += 1;
        run.frame = 0;
    }
}
