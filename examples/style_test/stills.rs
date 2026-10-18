//! The stills: each a state of the scene at a moment.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use hamlet_realm::Clock;
use hamlet_realm::keeper::KState;
use hamlet_vfx::inlay::InlayState;

use crate::sets;
use crate::show::{Sets, Show};
use crate::style::demo::{Demo, View};

struct Shot {
    name: String,
    demo: Demo,
    t: f32,
    set: Option<usize>,
}

fn shot_list(sets: &[sets::Set]) -> Vec<Shot> {
    let base = Demo::default();
    let mut shots = Vec::new();
    // Every moment of every set, as `<set>-<number>`.
    for (i, set) in sets.iter().enumerate() {
        for (k, &t) in set.moments.iter().enumerate() {
            shots.push(Shot {
                name: format!("{}-{k:02}", set.name),
                demo: Demo {
                    night: set.night,
                    view: View::Lineup,
                    ..base.clone()
                },
                t,
                set: Some(i),
            });
        }
    }
    let mut add = |name: &str, t: f32, demo: Demo| {
        shots.push(Shot {
            name: name.into(),
            demo,
            t,
            set: None,
        })
    };
    for night in [false, true] {
        let when = if night { "night" } else { "day" };
        let b = Demo {
            night,
            ..base.clone()
        };
        add(&format!("{when}-idle"), 0.5, b.clone());
        add(
            &format!("{when}-current"),
            4.45,
            Demo {
                inlay: InlayState::Current,
                kstate: KState::Working,
                ..b.clone()
            },
        );
        add(
            &format!("{when}-fill"),
            11.5,
            Demo {
                inlay: InlayState::Fill,
                kstate: KState::Working,
                ..b.clone()
            },
        );
        add(
            &format!("{when}-signal-with-tag"),
            0.5,
            Demo {
                inlay: InlayState::Signal,
                kstate: KState::Waiting,
                ..b.clone()
            },
        );
        add(
            &format!("{when}-fault"),
            0.9,
            Demo {
                inlay: InlayState::Fault,
                kstate: KState::Fault,
                ..b.clone()
            },
        );
        add(
            &format!("{when}-stall"),
            0.5,
            Demo {
                view: View::Stall,
                ..b.clone()
            },
        );
    }
    add(
        "day-idle-no-blur",
        0.5,
        Demo {
            dof: false,
            ..base.clone()
        },
    );
    add(
        "day-wide-current",
        4.45,
        Demo {
            inlay: InlayState::Current,
            kstate: KState::Working,
            view: View::Wide,
            ..base.clone()
        },
    );
    add(
        "night-wide-current",
        4.45,
        Demo {
            night: true,
            inlay: InlayState::Current,
            kstate: KState::Working,
            view: View::Wide,
            ..base.clone()
        },
    );
    // The two ends of a stride, in the line-up (second from the left
    // is walking).
    add(
        "keeper-walk-1",
        0.2746,
        Demo {
            view: View::Lineup,
            lineup: true,
            ..base.clone()
        },
    );
    add(
        "keeper-walk-2",
        0.8238,
        Demo {
            view: View::Lineup,
            lineup: true,
            ..base.clone()
        },
    );
    add(
        "keeper",
        0.5,
        Demo {
            view: View::Keeper,
            ..base.clone()
        },
    );
    add(
        "keeper-states",
        0.5,
        Demo {
            view: View::Lineup,
            lineup: true,
            ..base.clone()
        },
    );
    add(
        "keeper-states-night",
        0.5,
        Demo {
            night: true,
            view: View::Lineup,
            lineup: true,
            ..base.clone()
        },
    );
    shots
}

/// With STYLE_SHOT set: steps through the stills, saves each, then
/// exits.
#[derive(Resource)]
pub struct ShotRun {
    dir: PathBuf,
    index: usize,
    frame: u32,
    /// Stills saved so far.
    saved: u32,
}

impl ShotRun {
    /// A run that saves its stills into `dir`.
    pub fn into(dir: PathBuf) -> Self {
        ShotRun {
            dir,
            index: 0,
            frame: 0,
            saved: 0,
        }
    }
}

pub fn run_shots(
    mut commands: Commands,
    mut run: ResMut<ShotRun>,
    mut demo: ResMut<Demo>,
    mut show: ResMut<Show>,
    sets: Res<Sets>,
    mut clock: ResMut<Clock>,
    mut exit: MessageWriter<AppExit>,
) {
    let shots = shot_list(&sets.0);
    let only = std::env::var("STYLE_ONLY").ok();
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
    // settle.
    let settle = if run.saved == 0 {
        200
    } else if shot.set.is_some() {
        14
    } else {
        45
    };
    if run.frame == 0 {
        *demo = shot.demo.clone();
        clock.frozen = true;
        clock.t = shot.t;
        show.set = shot.set;
        show.fresh = true;
    }
    run.frame += 1;
    if run.frame == settle {
        run.saved += 1;
        let path = run.dir.join(format!("{}.png", shot.name));
        info!("saving {}", path.display());
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if run.frame >= settle + 8 {
        run.index += 1;
        run.frame = 0;
    }
}
