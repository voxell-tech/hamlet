//! The first look test: one skill stall, a curved path with the inlay
//! over a bridge, and the Keeper. It exists to judge the look. The
//! scene itself is in `style/`.
//!
//! Run: `cargo run --example style_test`
//! Stills: `STYLE_SHOT=<dir> cargo run --example style_test`
//! Timing: `STYLE_BENCH=1 cargo run --release --example style_test`
//! Timing of a crowd of robots: the same with `STYLE_BENCH=crowd`
//!
//! N steps through the sets in `sets.rs`: small scenes of robots
//! playing their motions.

#![expect(
    clippy::print_stdout,
    reason = "it prints its keys and the set it shows"
)]

#[path = "../common/note.rs"]
mod note;

mod bench;
mod sets;
mod show;
mod stills;
mod style;

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use bevy::winit::{UpdateMode, WinitSettings};
use hamlet_realm::debug::DebugSettings;
use hamlet_realm::{Clock, RealmSystems, WorldSystems};
use hamlet_vfx::inlay::InlayState;

use self::note::note;
use crate::bench::{BenchRun, run_bench, run_crowd_bench};
use crate::show::{Sets, Show, aim, cast, direct};
use crate::stills::{ShotRun, run_shots};
use crate::style::StylePlugin;
use crate::style::demo::{Demo, View, to_stage};

/// The request on the paper tag in the test.
const TAG_TEXT: &str = "Run: brew install jq";

/// True while the G key has the tag shown.
#[derive(Resource, Default)]
struct ShowTag(bool);

#[derive(Component)]
struct Hud;

fn main() {
    let shots = std::env::var("STYLE_SHOT").ok().map(PathBuf::from);
    let bench = std::env::var("STYLE_BENCH").ok();
    let crowd_bench = bench.as_deref() == Some("crowd");
    let bench = bench.is_some();
    // One window pixel per screen pixel unless asked otherwise, so
    // that the stills and the timings are at 1360x860 on any
    // display.
    let scale = std::env::var("STYLE_SCALE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(1.0);

    println!("Style test keys:");
    println!("  T  time of day (day, night)");
    println!(
        "  I  inlay state (unlit, current, fill, signal, fault)"
    );
    println!(
        "  K  Keeper state (rest, working, waiting, fault, failed)"
    );
    println!(
        "  C  camera (default, wide, Keeper close-up, stall \
         close-up)"
    );
    println!("  L  line up all five Keeper states");
    println!("  N  next set of robots in motion (replays from 0)");
    println!("  F  depth of field on or off");
    println!("  G  show the paper tag    P  pause    Esc  quit");
    println!("  F3  debug overlay (or run with STYLE_DEBUG=1)");

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "hamlet: style test".into(),
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
        StylePlugin,
    ))
    .insert_resource(DebugSettings {
        shown: std::env::var("STYLE_DEBUG").is_ok(),
        ..default()
    })
    .init_resource::<ShowTag>()
    .insert_resource(Sets(sets::all()))
    .insert_resource(Show {
        set: None,
        fresh: false,
        lights: true,
    })
    .insert_resource(WinitSettings::continuous())
    .add_systems(Startup, spawn_hud)
    .add_systems(
        Update,
        (
            (keys, show_tag, to_stage, cast, direct)
                .chain()
                .in_set(RealmSystems::Steer),
            aim.after(WorldSystems),
            hud,
        ),
    );

    if let Some(dir) = shots {
        app.insert_resource(ShotRun::into(dir))
            .add_systems(Update, run_shots.before(keys));
    } else if crowd_bench {
        app.insert_resource(BenchRun::default())
            .add_systems(Update, run_crowd_bench.before(keys));
    } else if bench {
        app.insert_resource(BenchRun::default())
            .add_systems(Update, run_bench.before(keys));
    } else {
        app.add_systems(Update, pacing);
    }
    app.run();
}

fn spawn_hud(mut commands: Commands) {
    commands.spawn((Hud, note("", false)));
}

/// The tag shows while an approval is waiting, or when G asks for it.
fn show_tag(show: Res<ShowTag>, mut demo: ResMut<Demo>) {
    let wanted = (show.0 || demo.inlay == InlayState::Signal)
        .then(|| TAG_TEXT.to_string());
    if demo.tag != wanted {
        demo.tag = wanted;
    }
}

fn keys(
    input: Res<ButtonInput<KeyCode>>,
    mut demo: ResMut<Demo>,
    mut show: ResMut<ShowTag>,
    mut on_show: ResMut<Show>,
    sets: Res<Sets>,
    mut clock: ResMut<Clock>,
    mut exit: MessageWriter<AppExit>,
) {
    if input.just_pressed(KeyCode::KeyN) {
        on_show.set = match on_show.set {
            None => Some(0),
            Some(i) if i + 1 < sets.0.len() => Some(i + 1),
            Some(_) => None,
        };
        on_show.fresh = true;
        clock.t = 0.0;
        demo.lineup = false;
        match on_show.set {
            Some(i) => {
                demo.view = View::Lineup;
                demo.night = sets.0[i].night;
                println!("set: {}", sets.0[i].name);
            }
            None => demo.view = View::Default,
        }
    }
    if input.just_pressed(KeyCode::KeyT) {
        demo.night = !demo.night;
    }
    if input.just_pressed(KeyCode::KeyI) {
        demo.inlay = demo.inlay.next();
        clock.t = 0.0;
    }
    if input.just_pressed(KeyCode::KeyK) {
        demo.kstate = demo.kstate.next();
    }
    if input.just_pressed(KeyCode::KeyC) {
        demo.view = demo.view.next();
    }
    if input.just_pressed(KeyCode::KeyL) {
        demo.lineup = !demo.lineup;
        demo.view = if demo.lineup {
            View::Lineup
        } else {
            View::Default
        };
    }
    if input.just_pressed(KeyCode::KeyF) {
        demo.dof = !demo.dof;
    }
    if input.just_pressed(KeyCode::KeyG) {
        show.0 = !show.0;
    }
    if input.just_pressed(KeyCode::KeyP) {
        clock.frozen = !clock.frozen;
    }
    if input.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

fn hud(
    demo: Res<Demo>,
    shots: Option<Res<ShotRun>>,
    mut hud: Single<(&mut Text, &mut Visibility), With<Hud>>,
) {
    *hud.1 = if shots.is_some() {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    let line = format!(
        "[T] {}   [I] inlay: {:?}   [K] Keeper: {:?}   [C] {:?}   \
         [F] focus blur {}   [L] line-up   [G] tag",
        if demo.night { "night" } else { "day" },
        demo.inlay,
        demo.kstate,
        demo.view,
        if demo.dof { "on" } else { "off" },
    );
    if hud.0.0 != line {
        hud.0.0 = line;
    }
}

/// Draw only when something is moving. With the inlay unlit and the
/// Keeper at rest the scene is still, so the window sleeps until
/// there is input.
fn pacing(
    demo: Res<Demo>,
    show: Res<Show>,
    mut settings: ResMut<WinitSettings>,
) {
    let mode = if demo.animating() || show.set.is_some() {
        UpdateMode::Continuous
    } else {
        UpdateMode::reactive_low_power(Duration::from_secs(60))
    };
    if settings.focused_mode != mode {
        settings.focused_mode = mode;
        settings.unfocused_mode = mode;
    }
}
