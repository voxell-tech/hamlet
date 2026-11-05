//! The endless world from far-apart places, with no window
//! (docs/endless-world.md): many towns, the land a long way from any
//! of them, and a town picked up and set down somewhere else.
//!
//! Stills: `cargo run --release --example far_lands -- --out <dir>`
//! Timing: `cargo run --release --example far_lands -- --bench`

#![expect(
    clippy::print_stdout,
    reason = "it prints its usage and its measurements"
)]

#[path = "../common/offscreen.rs"]
mod offscreen;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::audio::AudioPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hamlet_realm::generator::land::Land;
use hamlet_realm::generator::society::{Ground, Society, TownId};
use hamlet_realm::generator::{GeneratorView, Loading, Tally};
use hamlet_realm::moving::{self, Carrying};
use hamlet_realm::pick::{Centre, Glide, Orbit, Reach};
use hamlet_realm::sky::{CameraSettings, SkySettings};
use hamlet_realm::sound::SoundSettings;
use hamlet_realm::{Clock, Hour, RealmPlugin, Stage, WorldCamera};

use self::offscreen::Target;

/// The frame, in pixels.
const SIZE: UVec2 = UVec2::new(1360, 860);

/// Seconds a frame lasts.
const FRAME: f32 = 1.0 / 60.0;

/// One thing the run does, in order.
#[derive(Clone)]
enum Step {
    /// Founds towns with this many houses each.
    Found(Vec<usize>),
    /// Looks at a place from a distance, turned and tilted.
    Look {
        at: Spot,
        dist: f32,
        yaw: f32,
        pitch: f32,
    },
    Night(bool),
    /// Saves a still under this name once everything is built.
    Shot(&'static str),
    /// Waits until everything is built.
    Settle,
    /// Saves a still of this very frame.
    Snap(&'static str),
    /// Takes a town in hand and points at a place.
    Carry(u32, Spot),
    /// Sets a town down at a place, as the record would.
    Move(u32, Spot),
    PutBack,
    /// Lets the clock run this many seconds, or holds it.
    Run(f32),
    Hold,
    /// Slides the view this far over so many frames and reports the
    /// frames' times.
    Slide(&'static str, Vec2, u32),
    /// Reports what is drawn.
    Count(&'static str),
}

/// A place in the world, some of them found when they are needed.
#[derive(Clone, Copy)]
enum Spot {
    City(Vec2),
    /// A place measured from the middle of the hamlet island.
    Hamlet(Vec2),
    /// Halfway between the middles of the two islands.
    Mid,
    /// The site of a town, and so far to its side.
    Town(u32, Vec2),
    /// The nearest ground to a point, measured from the middle of the
    /// hamlet island, that the carried or moved town fits on.
    FitNear(Vec2),
    /// The nearest water to a point, measured as that is.
    WaterNear(Vec2),
    /// The last place that was found.
    Last,
}

#[derive(Resource)]
struct Run {
    steps: Vec<Step>,
    at: usize,
    /// Frames spent on the step.
    frames: u32,
    /// Frames since everything was built.
    settled: u32,
    /// The clock time a `Run` began at.
    since: f32,
    timing: bool,
    out: Option<PathBuf>,
    last: Vec2,
    town: u32,
    began: Instant,
    times: Vec<f32>,
    shots: u32,
}

fn look(at: Spot, dist: f32) -> Step {
    Step::Look {
        at,
        dist,
        yaw: 0.0,
        pitch: 40.0,
    }
}

/// The stills: both islands, the home ground, the hamlet island,
/// many towns, and a move.
fn stills() -> Vec<Step> {
    use Spot::*;
    use Step::*;
    let home = Town(0, Vec2::ZERO);
    vec![
        Found(vec![9, 5, 12, 6, 8, 4, 7, 10]),
        Hold,
        Look {
            at: Mid,
            dist: 2300.0,
            yaw: 0.0,
            pitch: 55.0,
        },
        Shot("01-both-islands"),
        look(City(Vec2::ZERO), 420.0),
        Shot("02-the-city-island"),
        look(home, 78.0),
        Shot("03-home"),
        look(home, 330.0),
        Shot("04-eight-towns"),
        look(home, 1150.0),
        Shot("05-as-far-as-the-camera-goes"),
        Look {
            at: home,
            dist: 120.0,
            yaw: 0.9,
            pitch: 14.0,
        },
        Shot("06-toward-the-horizon"),
        Look {
            at: Mid,
            dist: 700.0,
            yaw: 1.3,
            pitch: 12.0,
        },
        Shot("07-the-channel"),
        Night(true),
        look(home, 300.0),
        Shot("09-night"),
        Night(false),
        // A town in hand: over ground that takes it, over water and
        // over another town.
        look(Town(2, Vec2::ZERO), 150.0),
        Shot("10-before-the-move"),
        Carry(2, FitNear(Vec2::new(250.0, 160.0))),
        look(Last, 170.0),
        Shot("11-in-hand-fits"),
        Carry(2, WaterNear(Vec2::new(60.0, 20.0))),
        look(Last, 170.0),
        Shot("12-in-hand-water"),
        Carry(2, Town(1, Vec2::new(14.0, 6.0))),
        look(Last, 170.0),
        Shot("13-in-hand-on-a-town"),
        PutBack,
        // Set down: while it goes, and after.
        look(Town(2, Vec2::ZERO), 150.0),
        Settle,
        Run(0.05),
        Move(2, FitNear(Vec2::new(250.0, 160.0))),
        // The trees about the two sites are replanted meanwhile, so
        // the still waits for them.
        Run(0.22),
        Settle,
        Snap("14-going"),
        look(Last, 150.0),
        Run(0.95),
        Snap("15-arriving"),
        Run(3.0),
        Hold,
        Shot("16-after-the-move"),
        look(home, 420.0),
        Shot("17-roads-after-the-move"),
        look(Town(4, Vec2::ZERO), 110.0),
        Shot("18-where-it-stood"),
        // More towns than the land began with sites for.
        Found(vec![3; 34]),
        look(home, 1150.0),
        Shot("19-forty-two-towns"),
        look(Town(41, Vec2::ZERO), 240.0),
        Shot("20-the-outermost-town"),
        Look {
            at: Mid,
            dist: 2600.0,
            yaw: 0.0,
            pitch: 55.0,
        },
        Shot("21-both-islands-grown"),
        // Low over broken ground, where tiles of several sizes meet.
        Look {
            at: Hamlet(Vec2::new(-300.0, 200.0)),
            dist: 230.0,
            yaw: 0.6,
            pitch: 15.0,
        },
        Shot("22-low-over-the-hills"),
        // While the view slides fast, with the ground still coming:
        // the tiles in by then depend on the threads that build
        // them, so this still differs from run to run.
        look(home, 330.0),
        Shot("23-before-a-slide"),
        Slide("a slide for a still", Vec2::new(900.0, 500.0), 45),
        Snap("24-in-a-fast-slide"),
    ]
}

/// The timings: what is drawn from near and from far, and the
/// frames while the view slides.
fn bench() -> Vec<Step> {
    use Spot::*;
    use Step::*;
    let home = Town(0, Vec2::ZERO);
    vec![
        Found(vec![9, 5, 12, 6, 8, 4, 7, 10]),
        Run(0.0),
        look(home, 78.0),
        Count("8 towns, near"),
        Slide("8 towns, near, still", Vec2::ZERO, 240),
        Slide(
            "8 towns, near, sliding 60 a second",
            Vec2::X * 240.0,
            240,
        ),
        Slide("the same, 400 a second", Vec2::X * 1600.0, 240),
        look(home, 330.0),
        Count("8 towns, all in view"),
        Slide("8 towns, all in view, still", Vec2::ZERO, 240),
        Slide(
            "the same, sliding 400 a second",
            Vec2::Y * 1600.0,
            240,
        ),
        look(home, 1150.0),
        Count("8 towns, from as far as it goes"),
        Slide("the same, still", Vec2::ZERO, 240),
        Slide(
            "the same, sliding 2000 a second",
            Vec2::X * 8000.0,
            240,
        ),
        Found(vec![12; 34]),
        look(home, 78.0),
        Count("42 towns, near"),
        Slide("42 towns, near, still", Vec2::ZERO, 240),
        look(home, 1150.0),
        Count("42 towns, from as far as it goes"),
        Slide("the same, still", Vec2::ZERO, 240),
        look(Mid, 78.0),
        Count("42 towns, looking at the sea between the islands"),
        Slide("the same, still", Vec2::ZERO, 240),
    ]
}

fn main() {
    let timing = std::env::args().any(|a| a == "--bench");
    let out = offscreen::word("--out").map(PathBuf::from);
    if !timing && out.is_none() {
        println!("far_lands -- --out <dir> | --bench");
        return;
    }
    if let Some(dir) = &out {
        std::fs::create_dir_all(dir).expect("output folder");
    }

    let mut app = App::new();
    app.add_plugins((
        offscreen::plugins(SIZE).disable::<AudioPlugin>(),
        RealmPlugin,
    ));
    Target::insert(&mut app, SIZE, 1.0);
    // A held hour: with the clock running the sky would follow the
    // wall clock.
    let noon = app.world().resource::<SkySettings>().held(false);
    app.insert_resource(Hour(Some(noon)))
        .insert_resource(SoundSettings {
            muted: true,
            ..default()
        })
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f32(FRAME),
        ))
        .insert_resource(Run {
            steps: if timing { bench() } else { stills() },
            at: 0,
            frames: 0,
            settled: 0,
            since: 0.0,
            timing,
            out,
            last: Vec2::ZERO,
            town: 0,
            began: Instant::now(),
            times: Vec::new(),
            shots: 0,
        })
        .add_systems(PostStartup, draw_offscreen)
        .add_systems(Update, run);
    app.run();
}

fn draw_offscreen(
    mut commands: Commands,
    target: Res<Target>,
    camera: Single<Entity, With<WorldCamera>>,
) {
    commands
        .entity(*camera)
        .insert((target.drawn(), IsDefaultUiCamera));
}

/// The place of a spot, for the town the run is busy with.
fn place(
    spot: Spot,
    run: &Run,
    land: &Land,
    society: &Society,
) -> Vec2 {
    let around = |near: Vec2, fits: &dyn Fn(Vec2) -> bool| {
        (0..4000)
            .map(|i| {
                near + Vec2::from_angle(i as f32 * 2.4)
                    * (i as f32 * 0.6)
            })
            .find(|&p| fits(p))
            .unwrap_or(near)
    };
    let centre = land.hamlet_centre();
    match spot {
        Spot::City(at) => at,
        Spot::Hamlet(off) => centre + off,
        Spot::Mid => centre * 0.5,
        Spot::Town(town, off) => society
            .town(TownId(town))
            .map_or(off, |town| land.sites[town.site].at + off),
        Spot::FitNear(near) => around(centre + near, &|p| {
            society
                .fits(land, Ground::Town(TownId(run.town)), p)
                .is_ok()
        }),
        Spot::WaterNear(near) => {
            around(centre + near, &|p| land.is_water(p))
        }
        Spot::Last => run.last,
    }
}

/// Takes the camera to `to` through the hand's own orbit, which the
/// realm frames its view from: it measures the pan from the middle
/// of the places and the distance from the stand it takes to see them
/// all, which the view it made last frame gives back.
fn steer(
    to: GeneratorView,
    seen: &GeneratorView,
    centre: &Centre,
    camera: &CameraSettings,
    orbit: &mut Orbit,
    glide: &mut Glide,
) {
    let base = seen.dist / glide.0.zoom;
    let held = Orbit {
        pan: to.focus - centre.0,
        zoom: to.dist / base,
        yaw: to.yaw,
        pitch: (to.pitch - camera.pitch).to_radians(),
    };
    *orbit = held;
    glide.0 = held;
}

fn run(
    mut commands: Commands,
    mut run: ResMut<Run>,
    target: Res<Target>,
    loading: Res<Loading>,
    tally: Option<Res<Tally>>,
    mut land: ResMut<Land>,
    mut society: ResMut<Society>,
    view: Res<GeneratorView>,
    (mut orbit, mut glide, centre, camera_settings): (
        ResMut<Orbit>,
        ResMut<Glide>,
        Res<Centre>,
        Res<CameraSettings>,
    ),
    (mut stage, sky, mut hour, mut pace): (
        ResMut<Stage>,
        Res<SkySettings>,
        ResMut<Hour>,
        ResMut<TimeUpdateStrategy>,
    ),
    mut clock: ResMut<Clock>,
    mut reach: ResMut<Reach>,
    mut carrying: ResMut<Carrying>,
    camera: Single<&Transform, With<WorldCamera>>,
    entities: Query<Entity>,
    mut exit: MessageWriter<AppExit>,
) {
    run.settled = if loading.is_done() {
        run.settled + 1
    } else {
        0
    };
    run.frames += 1;
    // Time stands still while the world is built, so that a still
    // does not depend on how long the building took. A slide and a
    // timing are of the building itself.
    let waits = !run.timing
        && !loading.is_done()
        && !matches!(run.steps.get(run.at), Some(Step::Slide(..)));
    let frame = if waits { 0.0 } else { FRAME };
    *pace = TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f32(frame),
    );
    let Some(step) = run.steps.get(run.at).cloned() else {
        // Leave time for the last file to be written.
        if run.frames > 60 {
            exit.write(AppExit::Success);
        }
        return;
    };
    let first = run.frames == 1;
    let mut done = true;
    match step {
        Step::Found(sizes) => {
            for size in sizes {
                let town = society.found_town(&mut land);
                for _ in 0..size {
                    society.add_agent(&land, town);
                }
            }
        }
        Step::Look {
            at,
            dist,
            yaw,
            pitch,
        } => {
            let focus = place(at, &run, &land, &society);
            let to = GeneratorView {
                focus,
                dist,
                yaw,
                pitch,
            };
            steer(
                to,
                &view,
                &centre,
                &camera_settings,
                &mut orbit,
                &mut glide,
            );
        }
        Step::Night(night) => {
            stage.night = night;
            hour.0 = Some(sky.held(night));
        }
        Step::Hold => clock.frozen = true,
        Step::Run(secs) => {
            clock.frozen = false;
            if first {
                run.since = clock.t;
            }
            done = clock.t - run.since >= secs;
        }
        Step::Carry(town, at) => {
            run.town = town;
            let to = place(at, &run, &land, &society);
            run.last = to;
            commands.queue(move |world: &mut World| {
                moving::pick_up(world, Ground::Town(TownId(town)));
            });
            // The pointer, over that ground.
            let eye = camera.translation;
            let toward = land.on_ground(to) - eye;
            reach.at = Some(SIZE.as_vec2() * 0.5);
            reach.ray = Dir3::new(toward)
                .ok()
                .map(|toward| Ray3d::new(eye, toward));
        }
        Step::PutBack => {
            carrying.0 = None;
            reach.at = None;
            reach.ray = None;
        }
        Step::Move(town, at) => {
            run.town = town;
            let to = place(at, &run, &land, &society);
            run.last = to;
            let moved =
                society.move_town(&mut land, TownId(town), to);
            info!("town {town} to {to}: {moved:?}");
        }
        Step::Shot(name) => {
            // The first still waits for pipelines too.
            let wait = if run.shots == 0 { 120 } else { 12 };
            done = false;
            if run.settled == wait
                && let Some(dir) = &run.out
            {
                let path = dir.join(format!("{name}.png"));
                info!("saving {}", path.display());
                target.shoot(&mut commands, path);
            }
            if run.settled >= wait + 4 {
                run.shots += 1;
                done = true;
            }
        }
        Step::Settle => done = run.settled >= 12,
        Step::Snap(name) => {
            if let Some(dir) = &run.out {
                target.shoot(
                    &mut commands,
                    dir.join(format!("{name}.png")),
                );
            }
        }
        Step::Count(name) => {
            done = run.settled >= 30;
            if done {
                let tally =
                    tally.as_deref().copied().unwrap_or_default();
                let megabytes = |bytes: usize| bytes as f32 / 1e6;
                println!(
                    "{name}: built in {:.2} s; {} patches of \
                     ground, far trees in {:.1} MB, {} trees \
                     standing, {} towns, {} houses in {:.1} MB, {} \
                     roads in {:.1} MB, {} entities",
                    run.began.elapsed().as_secs_f32(),
                    tally.tiles,
                    megabytes(loading.bytes()),
                    tally.trees,
                    tally.towns,
                    tally.houses,
                    megabytes(tally.house_bytes),
                    tally.roads,
                    megabytes(tally.road_bytes),
                    entities.iter().count(),
                );
            }
        }
        Step::Slide(name, by, frames) => {
            if first {
                run.began = Instant::now();
                run.times.clear();
            } else {
                let now = run.began.elapsed().as_secs_f32();
                run.times.push(now);
            }
            let step = by / frames as f32;
            orbit.pan += step;
            glide.0.pan += step;
            done = run.frames > frames;
            if done {
                let mut each = run
                    .times
                    .windows(2)
                    .map(|w| (w[1] - w[0]) * 1000.0)
                    .collect::<Vec<_>>();
                each.sort_by(f32::total_cmp);
                let mean =
                    each.iter().sum::<f32>() / each.len() as f32;
                println!(
                    "{name}: mean {mean:.2} ms, median {:.2}, 99th \
                     {:.2}, worst {:.2}",
                    each[each.len() / 2],
                    each[each.len() * 99 / 100],
                    each[each.len() - 1],
                );
                orbit.pan -= by;
                glide.0.pan -= by;
            }
        }
    }
    if done {
        run.at += 1;
        run.frames = 0;
        run.settled = 0;
        if matches!(run.steps.get(run.at), Some(Step::Count(_))) {
            run.began = Instant::now();
        }
    }
}
