//! Frame times of the realm from several views, each with parts of it
//! turned off in turn, to find what a frame spends its time on. Drawn
//! offscreen, with no window.
//!
//! `cargo run --release --example sea_bench -- [--size 2720x1720]`

#![expect(
    clippy::print_stdout,
    reason = "it prints its measurements"
)]

#[path = "../common/offscreen.rs"]
#[expect(dead_code, reason = "it takes no stills")]
mod offscreen;

use std::time::{Duration, Instant};

use bevy::audio::AudioPlugin;
use bevy::prelude::*;
use bevy::render::view::Msaa;
use bevy::time::TimeUpdateStrategy;
use hamlet_realm::generator::land::Land;
use hamlet_realm::generator::society::{Society, TownId};
use hamlet_realm::generator::{GeneratorView, Loading};
use hamlet_realm::sky::SkySettings;
use hamlet_realm::sound::SoundSettings;
use hamlet_realm::{Hour, RealmPlugin, WorldCamera};
use hamlet_vfx::water::Waves;

use self::offscreen::Target;

/// The frame, in pixels, unless `--size` says otherwise.
const SIZE: UVec2 = UVec2::new(1360, 860);

/// Seconds a frame lasts.
const FRAME: f32 = 1.0 / 60.0;

/// Frames let pass once a view is built, and frames measured.
const WARM: u32 = 60;
const MEASURED: u32 = 180;

/// A view of the first town: its name, the distance, the turn in
/// radians and the tilt in degrees.
const VIEWS: [(&str, f32, f32, f32); 7] = [
    ("close", 30.0, 0.0, 40.0),
    ("default", 72.0, 0.0, 40.0),
    ("mid", 200.0, 0.0, 40.0),
    ("far", 500.0, 0.0, 40.0),
    ("farthest", 1150.0, 0.0, 40.0),
    ("low, to the horizon", 120.0, 0.9, 14.0),
    ("overhead", 400.0, 0.0, 80.0),
];

/// What is turned off: the water, the shadows, multisampling.
#[derive(Clone, Copy)]
struct Off {
    name: &'static str,
    water: bool,
    shadows: bool,
    msaa: bool,
}

const SCENES: [Off; 5] = [
    Off {
        name: "all",
        water: false,
        shadows: false,
        msaa: false,
    },
    Off {
        name: "no water",
        water: true,
        shadows: false,
        msaa: false,
    },
    Off {
        name: "no shadows",
        water: false,
        shadows: true,
        msaa: false,
    },
    Off {
        name: "no msaa",
        water: false,
        shadows: false,
        msaa: true,
    },
    Off {
        name: "no water, no shadows",
        water: true,
        shadows: true,
        msaa: false,
    },
];

/// A view of the first town as a distance, a turn in radians and a
/// tilt in degrees.
type Pose = (f32, f32, f32);

/// While the view moves: its name, where it starts and ends, how far
/// it slides over the ground, and the frames it takes. Measured with
/// everything on, without waiting for the ground to be built, so the
/// frames that build it show.
const MOVES: [(&str, Pose, Pose, Vec2, u32); 4] = [
    (
        "zooming out",
        (72.0, 0.0, 40.0),
        (1150.0, 0.0, 40.0),
        Vec2::ZERO,
        240,
    ),
    (
        "zooming in",
        (1150.0, 0.0, 40.0),
        (72.0, 0.0, 40.0),
        Vec2::ZERO,
        240,
    ),
    (
        "panning",
        (200.0, 0.0, 40.0),
        (200.0, 0.0, 40.0),
        Vec2::new(1600.0, 0.0),
        240,
    ),
    (
        "circling",
        (120.0, 0.0, 30.0),
        (120.0, std::f32::consts::TAU, 30.0),
        Vec2::ZERO,
        240,
    ),
];

/// One measurement: the view, what is off, the pose it starts and
/// ends at, how far it slides and the frames it lasts.
struct Shot {
    name: &'static str,
    scene: Off,
    from: Pose,
    to: Pose,
    slide: Vec2,
    frames: u32,
}

impl Shot {
    fn moves(&self) -> bool {
        self.from != self.to || self.slide != Vec2::ZERO
    }
}

/// Every view in every scene, held still, then the moves.
fn shots() -> Vec<Shot> {
    let held = VIEWS.iter().flat_map(|&(name, dist, yaw, pitch)| {
        SCENES.iter().map(move |&scene| Shot {
            name,
            scene,
            from: (dist, yaw, pitch),
            to: (dist, yaw, pitch),
            slide: Vec2::ZERO,
            frames: MEASURED,
        })
    });
    let moving =
        MOVES.iter().map(|&(name, from, to, slide, frames)| Shot {
            name,
            scene: SCENES[0],
            from,
            to,
            slide,
            frames,
        });
    held.chain(moving).collect()
}

#[derive(Resource)]
struct Run {
    shots: Vec<Shot>,
    /// The shot being measured, by number.
    at: usize,
    founded: bool,
    frames: u32,
    times: Vec<f32>,
    began: Instant,
}

fn main() {
    let size = offscreen::word("--size")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some(UVec2::new(w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(SIZE);
    println!("{}x{} pixels", size.x, size.y);
    println!(
        "{:<22} {:<22} {:>9} {:>9} {:>9}",
        "view", "scene", "mean ms", "99th ms", "worst ms"
    );

    let mut app = App::new();
    app.add_plugins((
        offscreen::plugins(size).disable::<AudioPlugin>(),
        RealmPlugin,
    ));
    Target::insert(&mut app, size, 1.0);
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
            shots: shots(),
            at: 0,
            founded: false,
            frames: 0,
            times: Vec::new(),
            began: Instant::now(),
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

fn run(
    mut run: ResMut<Run>,
    loading: Res<Loading>,
    mut land: ResMut<Land>,
    mut society: ResMut<Society>,
    mut view: ResMut<GeneratorView>,
    mut water: Query<&mut Visibility, With<Waves>>,
    mut suns: Query<&mut DirectionalLight>,
    mut msaa: Query<&mut Msaa, With<WorldCamera>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !run.founded {
        for size in [9, 5, 12, 6, 8, 4, 7, 10] {
            let town = society.found_town(&mut land);
            for _ in 0..size {
                society.add_agent(&land, town);
            }
        }
        run.founded = true;
    }
    let Some(shot) = run.shots.get(run.at) else {
        exit.write(AppExit::Success);
        return;
    };
    let (name, scene, moves) = (shot.name, shot.scene, shot.moves());
    let (from, to, slide, frames) =
        (shot.from, shot.to, shot.slide, shot.frames);
    // The first shot waits longer: pipelines compile during it.
    let warm = if run.at == 0 { WARM * 4 } else { WARM };
    let home = society
        .town(TownId(0))
        .map_or(Vec2::ZERO, |town| land.sites[town.site].at);
    // How far through its frames the shot is.
    let t = (run.frames.saturating_sub(warm) as f32 / frames as f32)
        .clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let want = GeneratorView {
        focus: home + slide * t,
        dist: lerp(from.0, to.0),
        yaw: lerp(from.1, to.1),
        pitch: lerp(from.2, to.2),
    };
    if *view != want {
        *view = want;
    }
    for mut seen in &mut water {
        let next = if scene.water {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *seen != next {
            *seen = next;
        }
    }
    for mut sun in &mut suns {
        if sun.shadow_maps_enabled == scene.shadows {
            sun.shadow_maps_enabled = !scene.shadows;
        }
    }
    for mut samples in &mut msaa {
        let next = if scene.msaa { Msaa::Off } else { Msaa::Sample4 };
        if *samples != next {
            *samples = next;
        }
    }
    // A held view is measured once all is built; a move from when it
    // sets off, building and all.
    if !loading.is_done() && !(moves && run.frames >= warm) {
        run.frames = 0;
        return;
    }
    run.frames += 1;
    if run.frames == warm {
        run.times.clear();
        run.began = Instant::now();
    } else if run.frames > warm {
        let now = run.began.elapsed().as_secs_f32();
        run.times.push(now);
    }
    if run.frames > warm + frames {
        let mut each = run
            .times
            .windows(2)
            .map(|w| (w[1] - w[0]) * 1000.0)
            .collect::<Vec<_>>();
        each.sort_by(f32::total_cmp);
        let mean = each.iter().sum::<f32>() / each.len() as f32;
        println!(
            "{:<22} {:<22} {:>9.2} {:>9.2} {:>9.2}",
            name,
            scene.name,
            mean,
            each[each.len() * 99 / 100],
            each[each.len() - 1],
        );
        run.at += 1;
        run.frames = 0;
    }
}
