//! Frame times of the realm from several views, each with parts of it
//! turned off in turn or drawn at a lower quality, to find what a
//! frame spends its time on. Drawn
//! offscreen, with no window. With `--out`, it saves a still of each
//! view instead, as it is and as its meshes' edges. With `--view`, it
//! measures that one view alone.
//!
//! `cargo run --release --example sea_bench -- [--size 2720x1720]
//! [--out <dir>] [--view <name>]`

#![expect(
    clippy::print_stdout,
    reason = "it prints its measurements"
)]

#[path = "../common/offscreen.rs"]
mod offscreen;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::audio::AudioPlugin;
use bevy::pbr::wireframe::WireframeConfig;
use bevy::prelude::*;
use bevy::render::view::Msaa;
use bevy::time::TimeUpdateStrategy;
use hamlet_realm::generator::land::Land;
use hamlet_realm::generator::society::{Society, TownId};
use hamlet_realm::generator::{GeneratorView, Loading};
use hamlet_realm::sky::{Quality, SkySettings};
use hamlet_realm::sound::SoundSettings;
use hamlet_realm::{Hour, RealmPlugin, WorldCamera};
use hamlet_vfx::grass::GrassMaterial;
use hamlet_vfx::water::Waves;

use self::offscreen::Target;

/// The frame, in pixels, unless `--size` says otherwise.
const SIZE: UVec2 = UVec2::new(1360, 860);

/// Seconds a frame lasts.
const FRAME: f32 = 1.0 / 60.0;

/// Frames let pass once a view is built, and frames measured.
const WARM: u32 = 60;
const MEASURED: u32 = 180;

/// A view: its name, the distance, the turn in radians and the tilt
/// in degrees, and whether it looks at the sea off the coast rather
/// than at the first town.
const VIEWS: [(&str, f32, f32, f32, bool); 9] = [
    ("close", 30.0, 0.0, 40.0, false),
    ("default", 72.0, 0.0, 40.0, false),
    ("mid", 200.0, 0.0, 40.0, false),
    ("far", 500.0, 0.0, 40.0, false),
    ("farthest", 1150.0, 0.0, 40.0, false),
    ("low, to the horizon", 120.0, 0.9, 14.0, false),
    ("overhead", 400.0, 0.0, 80.0, false),
    ("sea, low", 60.0, 0.0, 18.0, true),
    ("sea, high", 300.0, 0.0, 45.0, true),
];

/// What is turned off: the water, the shadows, multisampling, the
/// grass; and the quality it is drawn at.
#[derive(Clone, Copy)]
struct Off {
    name: &'static str,
    water: bool,
    shadows: bool,
    msaa: bool,
    grass: bool,
    quality: Quality,
}

/// Everything on, at the highest quality.
const ALL: Off = Off {
    name: "all",
    water: false,
    shadows: false,
    msaa: false,
    grass: false,
    quality: Quality::High,
};

const SCENES: [Off; 8] = [
    ALL,
    Off {
        name: "no grass",
        grass: true,
        ..ALL
    },
    Off {
        name: "medium",
        quality: Quality::Medium,
        ..ALL
    },
    Off {
        name: "low",
        quality: Quality::Low,
        ..ALL
    },
    Off {
        name: "no water",
        water: true,
        ..ALL
    },
    Off {
        name: "no shadows",
        shadows: true,
        ..ALL
    },
    Off {
        name: "no msaa",
        msaa: true,
        ..ALL
    },
    Off {
        name: "no water, no shadows",
        water: true,
        shadows: true,
        ..ALL
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
    /// Whether it looks at the sea off the coast.
    at_sea: bool,
}

impl Shot {
    fn moves(&self) -> bool {
        self.from != self.to || self.slide != Vec2::ZERO
    }
}

/// Every view in every scene, held still, then the moves.
fn shots() -> Vec<Shot> {
    let held =
        VIEWS.iter().flat_map(|&(name, dist, yaw, pitch, at_sea)| {
            SCENES.iter().map(move |&scene| Shot {
                name,
                scene,
                from: (dist, yaw, pitch),
                to: (dist, yaw, pitch),
                slide: Vec2::ZERO,
                frames: MEASURED,
                at_sea,
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
            at_sea: false,
        });
    held.chain(moving).collect()
}

/// The stills: every view with everything on, then as its edges.
fn stills() -> Vec<Shot> {
    VIEWS
        .iter()
        .flat_map(|&(name, dist, yaw, pitch, at_sea)| {
            [false, true].map(|edges| Shot {
                at_sea,
                name,
                scene: Off {
                    name: if edges { "edges" } else { "all" },
                    ..SCENES[0]
                },
                from: (dist, yaw, pitch),
                to: (dist, yaw, pitch),
                slide: Vec2::ZERO,
                frames: 4,
            })
        })
        .collect()
}

#[derive(Resource)]
struct Run {
    shots: Vec<Shot>,
    /// Where the stills go, when it takes stills.
    out: Option<PathBuf>,
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
    let out = offscreen::word("--out").map(PathBuf::from);
    let only = offscreen::word("--view");
    if let Some(dir) = &out {
        std::fs::create_dir_all(dir).expect("output folder");
    }
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
            shots: if out.is_some() { stills() } else { shots() }
                .into_iter()
                .filter(|shot| {
                    only.as_ref().is_none_or(|v| shot.name == v)
                })
                .collect(),
            out,
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
    mut commands: Commands,
    mut run: ResMut<Run>,
    target: Res<Target>,
    edges: Option<ResMut<WireframeConfig>>,
    loading: Res<Loading>,
    mut land: ResMut<Land>,
    mut society: ResMut<Society>,
    mut view: ResMut<GeneratorView>,
    mut water: Query<&mut Visibility, With<Waves>>,
    mut grass: Query<
        &mut Visibility,
        (With<MeshMaterial3d<GrassMaterial>>, Without<Waves>),
    >,
    mut quality: ResMut<Quality>,
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
        // Leave time for the last still to be written.
        run.frames += 1;
        if run.frames > 30 {
            exit.write(AppExit::Success);
        }
        return;
    };
    let (name, scene, moves) = (shot.name, shot.scene, shot.moves());
    let (from, to, slide, frames, at_sea) =
        (shot.from, shot.to, shot.slide, shot.frames, shot.at_sea);
    // The first shot waits longer: pipelines compile during it.
    let warm = if run.at == 0 { WARM * 4 } else { WARM };
    let town = society
        .town(TownId(0))
        .map_or(Vec2::ZERO, |town| land.sites[town.site].at);
    // Out to sea from the town: the first water that way, and as far
    // again beyond.
    let home = if at_sea {
        (0..2000)
            .map(|i| town + Vec2::X * i as f32)
            .find(|&p| {
                land.is_water(p)
                    && land.height(p) < land.set.land.water_y - 1.0
            })
            .map_or(town, |coast| coast + Vec2::X * 40.0)
    } else {
        town
    };
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
    // Squares of grass come and go as the view moves, so each is
    // hidden as it comes.
    for mut seen in &mut grass {
        let next = if scene.grass {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *seen != next {
            *seen = next;
        }
    }
    quality.set_if_neq(scene.quality);
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
    if let Some(mut edges) = edges {
        let wanted = scene.name == "edges";
        if edges.global != wanted {
            edges.global = wanted;
        }
    }
    // A held view is measured once all is built; a move from when it
    // sets off, building and all.
    if !loading.is_done() && !(moves && run.frames >= warm) {
        run.frames = 0;
        return;
    }
    run.frames += 1;
    if let Some(dir) = run.out.clone() {
        if run.frames == warm {
            let path = dir.join(format!(
                "{:02}-{}-{}.png",
                run.at,
                name.replace([' ', ','], "-"),
                scene.name
            ));
            println!("{}", path.display());
            target.shoot(&mut commands, path);
        }
        if run.frames > warm + frames {
            run.at += 1;
            run.frames = 0;
        }
        return;
    }
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
