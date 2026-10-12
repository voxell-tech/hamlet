//! Frame times of the scene, and of a growing crowd of robots.

use std::time::Instant;

use bevy::diagnostic::{
    DiagnosticsStore, FrameTimeDiagnosticsPlugin,
};
use bevy::prelude::*;
use hamlet_vfx::inlay::InlayState;
use hamlet_world::Clock;
use hamlet_world::keeper::KState;

use crate::show::{Sets, Show};
use crate::style::demo::{Demo, View};

/// With STYLE_BENCH set: measures frame time with vertical sync off.
#[derive(Resource)]
pub struct BenchRun {
    /// Frames let pass before a phase is measured.
    warm: u32,
    /// Frames a phase is measured over.
    measured: u32,
    phase: usize,
    frame: u32,
    started: Option<Instant>,
}

impl Default for BenchRun {
    fn default() -> Self {
        BenchRun {
            warm: 300,
            measured: 1200,
            phase: 0,
            frame: 0,
            started: None,
        }
    }
}

/// With STYLE_BENCH=crowd: measures frame time with more and more
/// robots on the grass.
pub fn run_crowd_bench(
    mut run: ResMut<BenchRun>,
    mut demo: ResMut<Demo>,
    mut show: ResMut<Show>,
    sets: Res<Sets>,
    mut clock: ResMut<Clock>,
    mut exit: MessageWriter<AppExit>,
) {
    // The first two are thrown away: pipelines compile during them.
    let phases = [
        ("town-192", false, true),
        ("town-192", true, true),
        ("town-1", false, true),
        ("town-48", false, true),
        ("town-192", false, true),
        ("town-1", true, true),
        ("town-48", true, true),
        ("town-192", true, true),
        ("town-192", true, false),
    ];
    let Some(&(name, night, lights)) = phases.get(run.phase) else {
        exit.write(AppExit::Success);
        return;
    };
    if run.frame == 0 {
        *demo = Demo {
            night,
            view: View::Lineup,
            ..default()
        };
        show.set = sets.0.iter().position(|s| s.name == name);
        show.lights = lights;
        show.fresh = true;
        clock.frozen = false;
        clock.t = 0.0;
    }
    run.frame += 1;
    if run.frame == run.warm {
        run.started = Some(Instant::now());
    }
    if run.frame == run.warm + run.measured {
        let ms = run.started.unwrap().elapsed().as_secs_f64()
            * 1000.0
            / run.measured as f64;
        let warm = if run.phase < 2 { "warm-up" } else { "bench" };
        println!(
            "{warm}: {name}, {}, lanterns {}: {ms:.2} ms per frame",
            if night { "night" } else { "day" },
            if lights {
                "light the ground"
            } else {
                "cast no light"
            },
        );
        run.phase += 1;
        run.frame = 0;
    }
}

pub fn run_bench(
    mut run: ResMut<BenchRun>,
    mut demo: ResMut<Demo>,
    mut clock: ResMut<Clock>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    let phases = [
        (
            "day, inlay and Keeper animating",
            false,
            InlayState::Current,
            KState::Working,
            true,
        ),
        (
            "night, inlay and Keeper animating",
            true,
            InlayState::Current,
            KState::Working,
            true,
        ),
        (
            "day, nothing animating, redrawn every frame",
            false,
            InlayState::Unlit,
            KState::Rest,
            true,
        ),
        (
            "night, nothing animating, redrawn every frame",
            true,
            InlayState::Unlit,
            KState::Rest,
            true,
        ),
        (
            "day, animating, no depth of field",
            false,
            InlayState::Current,
            KState::Working,
            false,
        ),
    ];
    let Some(&(name, night, inlay, kstate, dof)) =
        phases.get(run.phase)
    else {
        exit.write(AppExit::Success);
        return;
    };
    if run.frame == 0 {
        demo.night = night;
        demo.inlay = inlay;
        demo.kstate = kstate;
        demo.dof = dof;
        clock.frozen = false;
    }
    run.frame += 1;
    if run.frame == run.warm {
        run.started = Some(Instant::now());
    }
    if run.frame == run.warm + run.measured {
        let ms = run.started.unwrap().elapsed().as_secs_f64()
            * 1000.0
            / run.measured as f64;
        let bevy = diagnostics
            .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
            .and_then(|d| d.average())
            .unwrap_or(0.0);
        println!(
            "bench: {name}: {ms:.2} ms per frame ({:.0} fps); \
             Bevy's frame_time average {bevy:.2} ms",
            1000.0 / ms
        );
        run.phase += 1;
        run.frame = 0;
    }
}
