//! The generated world alone (docs/endless-world.md):
//! towns, houses, robots and roads drawn from a made-up society, with
//! keys to grow it and watch that nothing already built moves.
//!
//! Run: `cargo run --example town_gen`
//! Stills: `TOWN_SHOT=<dir> cargo run --example town_gen`
//! Timing: `TOWN_BENCH=1 cargo run --release --example town_gen`

// Bevy systems take many parameters and long query types by nature.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

#[path = "../common/note.rs"]
mod note;

mod bench;
mod demo;
mod keys;
mod made_up;
mod stills;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use bevy::winit::WinitSettings;
use hamlet_realm::WorldSystems;
use hamlet_realm::debug::DebugSettings;
use hamlet_realm::generator::land::Land;
use hamlet_realm::generator::society::Society;
use hamlet_realm::generator::{
    GenPlugin, GenView, Loading, Phase, Tally,
};
use hamlet_realm::sound::SoundSettings;

use self::bench::{
    BenchRun, MainTime, main_began, main_ended, run_bench,
};
use self::demo::{Demo, demo};
use self::keys::{Hand, keys};
use self::made_up::{effects, society};
use self::note::note;
use self::stills::{ShotRun, run_shots};

#[derive(Component)]
struct Hud;

fn main() {
    let shots = std::env::var("TOWN_SHOT").ok().map(PathBuf::from);
    let bench = std::env::var("TOWN_BENCH").is_ok();
    let seed = std::env::var("TOWN_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(7);
    let scale = std::env::var("TOWN_SCALE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(1.0);

    println!("Town generator keys:");
    println!("  A  add an agent      X  remove an agent");
    println!("  Z  bring a removed or merged agent back");
    println!("  R  add a road        U  raise a road's level");
    println!("  O  let a road grow over");
    println!("  F  found a town      M  merge two agents");
    println!("  G  send a robot along a road");
    println!("  W  make an agent wait for you (again: fault, rest)");
    println!("  B  set the town to work (again: to rest)");
    println!(
        "  K  tell the town's robots what the work is: the next"
    );
    println!("     task of the loop      J  a tool fails");
    println!("  Tab  next town       0  the whole world");
    println!("  Arrows pan   - =  zoom   Q E  turn   T  day, night");
    println!("  [ ]  turn the hour back and on");
    println!("  N  sound on, off     , .  quieter, louder");
    println!("  Y  music on, off");
    println!("  F3  debug overlay (or run with TOWN_DEBUG=1)");
    println!("  Esc  quit");

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "hamlet: town generator".into(),
                resolution: WindowResolution::new(1360, 860)
                    .with_scale_factor_override(scale),
                present_mode: if bench {
                    PresentMode::AutoNoVsync
                } else {
                    PresentMode::AutoVsync
                },
                ..default()
            }),
            ..default()
        }),
        GenPlugin { seed, ..default() },
    ))
    .insert_resource(effects(
        &std::env::var("TOWN_FX").unwrap_or_default(),
    ))
    .insert_resource(DebugSettings {
        shown: std::env::var("TOWN_DEBUG").is_ok(),
        ..default()
    })
    .init_resource::<Hand>()
    .insert_resource(WinitSettings::continuous())
    .add_systems(Startup, (spawn_hud, first_world))
    .add_systems(Update, hud);

    if let Some(dir) = shots {
        app.insert_resource(ShotRun::into(dir)).add_systems(
            Update,
            run_shots.run_if(world_loaded).before(WorldSystems),
        );
    } else if bench {
        app.insert_resource(BenchRun::of(
            std::env::var("TOWN_BENCH").is_ok_and(|b| b == "fx"),
        ))
        .init_resource::<MainTime>()
        .add_systems(First, main_began)
        .add_systems(Last, main_ended)
        .add_systems(
            Update,
            run_bench.run_if(world_loaded).before(WorldSystems),
        );
    } else {
        // Heard here, where the app keeps it off until asked.
        let mut sound = SoundSettings {
            muted: false,
            music_on: true,
            ..default()
        };
        if let Some(volume) = std::env::var("TOWN_VOLUME")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
        {
            sound.volume = volume;
        }
        app.insert_resource(sound)
            .add_systems(Update, keys.before(WorldSystems));
        let secs = std::env::var("TOWN_DEMO")
            .ok()
            .and_then(|secs| secs.parse::<f32>().ok());
        if let Some(until) = secs {
            app.insert_resource(Demo::until(until))
                .add_systems(Update, demo.before(WorldSystems));
        }
    }
    app.run();
}

fn first_world(
    land: Res<Land>,
    mut world: ResMut<Society>,
    mut view: ResMut<GenView>,
) {
    *world = society(&land, &[9, 5], 3);
    view.focus = land.sites[world.towns[0].site].at;
    view.dist = 78.0;
}

fn spawn_hud(mut commands: Commands) {
    commands.spawn((Hud, note("", false)));
    commands.spawn((
        note("The land is being built", true),
        DespawnOnExit(Phase::Loading),
    ));
}

fn hud(
    tally: Res<Tally>,
    hand: Res<Hand>,
    shots: Option<Res<ShotRun>>,
    bench: Option<Res<BenchRun>>,
    mut hud: Single<(&mut Text, &mut Visibility), With<Hud>>,
) {
    *hud.1 = if shots.is_some() || bench.is_some() {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    let line = format!(
        "town {} of {}   {} houses   {} robots   {} roads ({:.0} \
         units)   {} trees   [A] agent [X] remove [Z] restore [R] \
         road [U] raise [O] overgrow [F] found [M] merge [V] move \
         [G] send [W] wait [B] work [K] task [J] fail [Tab] next \
         [0] world [T] night",
        hand.town + 1,
        tally.towns,
        tally.houses,
        tally.robots,
        tally.roads,
        tally.road_length,
        tally.trees,
    );
    if hud.0.0 != line {
        hud.0.0 = line;
    }
}

/// Stills and timings wait for the ground and the trees.
fn world_loaded(loading: Option<Res<Loading>>) -> bool {
    loading.is_some_and(|l| l.done())
}
