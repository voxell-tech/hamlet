//! Adding a plugin from a folder the chief picks: the system's
//! dialog, the folder read off the frame, the question put where the
//! folder is a plugin's source or takes another's place, the build,
//! and the copy into the install folder, after which the plugin is
//! asked about as one that appeared there.

mod build;
pub(crate) mod check;
pub(crate) mod place;

use std::path::PathBuf;

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use hamlet_base::{HomeDir, Tokio};
use hamlet_desktop::file::{FileDialog, Pick};
use hamlet_sdk::capability::{
    AddAnswer, AddPlugin, Adding, Appeared, NativeFolders, Question,
    ReadNative, Uninstalled,
};
use hamlet_sdk::plugin::{FullAccess, PluginInfo};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};
use tokio::sync::oneshot;

use self::build::Failed;
use self::check::{Known, Make, Plan};
use crate::install::{WASM, cap};
use crate::limits::Limits;
use crate::pick::Picking;

/// Opens the dialog when the chief asks to add a plugin, and carries
/// what was chosen through to the install folder.
pub fn plugin(app: &mut App) {
    app.init_resource::<Adding>()
        .init_resource::<Adder>()
        .add_observer(add)
        .add_observer(answer)
        .add_systems(
            Update,
            (open.run_if(asked), take.run_if(under_way)).chain(),
        );
}

/// The title of the dialog.
const TITLE: &str = "Choose the folder of a plugin to add";

/// What work off the frame gives back.
enum Done {
    Read(Result<Plan, String>),
    Line(String),
    Built(Result<Plan, Failed>),
    Placed(Plan, Result<(), String>),
}

/// The dialog of the chief's folder, and the work on what was chosen
/// in it.
#[derive(Resource)]
pub struct Adder {
    /// The dialog is to be opened.
    asked: bool,
    tx: UnboundedSender<Option<PathBuf>>,
    rx: UnboundedReceiver<Option<PathBuf>>,
    done_tx: UnboundedSender<Done>,
    done_rx: UnboundedReceiver<Done>,
    /// The folder read, waiting for the chief to go on.
    waiting: Option<Plan>,
    /// Ends the build under way.
    stop: Option<oneshot::Sender<()>>,
}

impl Default for Adder {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        let (done_tx, done_rx) = unbounded_channel();
        Self {
            asked: false,
            tx,
            rx,
            done_tx,
            done_rx,
            waiting: None,
            stop: None,
        }
    }
}

impl Adder {
    /// Closes the dialog with `folder` chosen, or with nothing, as
    /// the system's dialog does.
    pub fn choose(&self, folder: Option<PathBuf>) {
        let _ = self.tx.send(folder);
    }
}

fn asked(adder: Res<Adder>) -> bool {
    adder.asked
}

fn under_way(adding: Res<Adding>) -> bool {
    !adding.free()
}

/// Has the dialog opened, unless adding is already under way.
fn add(
    _: On<AddPlugin>,
    dialog: Option<Res<FileDialog>>,
    picking: Res<Picking>,
    mut adder: ResMut<Adder>,
    mut adding: ResMut<Adding>,
) {
    if !adding.free() {
        return;
    }
    let refused = |why: &str| Adding::Refused {
        why: why.into(),
        log: String::new(),
    };
    *adding = match dialog {
        None => refused("there is no folder dialog here"),
        // One dialog is open at a time, a plugin's among them.
        Some(_) if picking.under_way() => {
            refused("a file dialog is already open")
        }
        Some(_) => {
            adder.asked = true;
            Adding::Choosing
        }
    };
}

/// Opens the dialog. The marker keeps this on the main thread, where
/// macOS wants a dialog begun.
fn open(
    _main: NonSendMarker,
    dialog: Option<Res<FileDialog>>,
    tokio: Option<Res<Tokio>>,
    mut adder: ResMut<Adder>,
) {
    adder.asked = false;
    if let Some(dialog) = dialog {
        (dialog.0)(
            TITLE.into(),
            Pick::Folder,
            adder.tx.clone(),
            tokio.as_deref().map(|tokio| &tokio.0),
        );
    }
}

/// Takes in what the dialog and the work off the frame gave.
fn take(
    home: Option<Res<HomeDir>>,
    limits: Res<Limits>,
    tokio: Option<Res<Tokio>>,
    native: Option<Res<NativeFolders>>,
    plugins: Query<(&PluginInfo, Has<FullAccess>, Has<Uninstalled>)>,
    mut adder: ResMut<Adder>,
    mut adding: ResMut<Adding>,
    mut commands: Commands,
) {
    let Some(home) = home else {
        return;
    };
    let native = native.map(|native| native.0);
    let background = Background {
        tokio: tokio.as_deref(),
        done: adder.done_tx.clone(),
    };
    while let Ok(chosen) = adder.rx.try_recv() {
        let Some(chosen) = chosen else {
            *adding = Adding::Idle;
            continue;
        };
        *adding = Adding::Reading;
        let known = plugins
            .iter()
            .map(|(info, native, uninstalled)| Known {
                id: info.id.clone(),
                name: info.name.clone(),
                version: info.version.clone(),
                builtin: info.builtin,
                native,
                installed: !uninstalled,
            })
            .collect::<Vec<_>>();
        let (root, limits) = (home.0.root.clone(), limits.clone());
        background.blocking(move || {
            Done::Read(check::read(
                &chosen, &root, &limits, &known, native,
            ))
        });
    }
    while let Ok(done) = adder.done_rx.try_recv() {
        match done {
            Done::Read(Ok(plan)) => {
                let build = plan.make != Make::Copy;
                if build || plan.replaces.is_some() {
                    *adding = Adding::Asking(Question {
                        name: plan.name.clone(),
                        version: plan.version.clone(),
                        build,
                        replaces: plan
                            .replaces
                            .as_ref()
                            .map(|(words, _)| words.clone()),
                        native: plan.native,
                    });
                    adder.waiting = Some(plan);
                } else {
                    *adding = placing(
                        &background,
                        plan,
                        &home,
                        &limits,
                        native,
                    );
                }
            }
            Done::Read(Err(why)) | Done::Placed(_, Err(why)) => {
                *adding = Adding::Refused {
                    why,
                    log: String::new(),
                };
            }
            Done::Line(line) => {
                if let Adding::Building { line: last, .. } =
                    &mut *adding
                {
                    *last = line;
                }
            }
            Done::Built(Ok(plan)) => {
                adder.stop = None;
                *adding = placing(
                    &background,
                    plan,
                    &home,
                    &limits,
                    native,
                );
            }
            Done::Built(Err(Failed { why, log })) => {
                adder.stop = None;
                *adding = Adding::Refused { why, log };
            }
            Done::Placed(plan, Ok(())) => {
                info!(
                    "plugin {}: {} {} added from {}",
                    plan.id,
                    plan.name,
                    plan.version,
                    plan.folder.display()
                );
                commands.trigger(Appeared {
                    plugin: plan.id.clone(),
                    asked: true,
                });
                *adding = Adding::Added(added(&plan));
            }
        }
    }
}

/// The chief's answer to the question put about the folder, or to
/// stop its build.
fn answer(
    answer: On<AddAnswer>,
    home: Option<Res<HomeDir>>,
    limits: Res<Limits>,
    tokio: Option<Res<Tokio>>,
    native: Option<Res<NativeFolders>>,
    mut adder: ResMut<Adder>,
    mut adding: ResMut<Adding>,
) {
    if matches!(*adding, Adding::Building { .. }) {
        if !answer.go
            && let Some(stop) = adder.stop.take()
        {
            let _ = stop.send(());
        }
        return;
    }
    let (Adding::Asking(_), Some(plan), Some(home)) =
        (&*adding, adder.waiting.take(), home)
    else {
        return;
    };
    if !answer.go {
        *adding = Adding::Idle;
        return;
    }
    let background = Background {
        tokio: tokio.as_deref(),
        done: adder.done_tx.clone(),
    };
    if plan.make == Make::Copy {
        let native = native.map(|native| native.0);
        *adding = placing(&background, plan, &home, &limits, native);
        return;
    }
    let Some(tokio) = tokio else {
        *adding = Adding::Refused {
            why: "there is no runtime to build on".into(),
            log: String::new(),
        };
        return;
    };
    let (stop, stopped) = oneshot::channel();
    adder.stop = Some(stop);
    *adding = Adding::Building {
        name: plan.name.clone(),
        line: String::new(),
    };
    let done = adder.done_tx.clone();
    let wasm = cap(WASM, &limits);
    let log = limits.build_log_bytes;
    tokio.0.spawn(async move {
        let say = |line| {
            let _ = done.send(Done::Line(line));
        };
        let built = build::build(plan, wasm, log, say, stopped).await;
        let _ = done.send(Done::Built(built));
    });
}

/// Where work on the chosen folder runs: on the runtime where there
/// is one, and at once otherwise.
struct Background<'a> {
    tokio: Option<&'a Tokio>,
    done: UnboundedSender<Done>,
}

impl Background<'_> {
    fn blocking(&self, work: impl FnOnce() -> Done + Send + 'static) {
        let done = self.done.clone();
        let run = move || {
            let _ = done.send(work());
        };
        match self.tokio {
            Some(tokio) => {
                tokio.0.spawn_blocking(run);
            }
            None => run(),
        }
    }
}

/// Starts the copy of `plan` into the install folder.
fn placing(
    background: &Background,
    plan: Plan,
    home: &HomeDir,
    limits: &Limits,
    native: Option<ReadNative>,
) -> Adding {
    let (plugins, limits) = (home.0.plugins(), limits.clone());
    background.blocking(move || {
        let placed = place::place(&plan, &plugins, &limits, native);
        Done::Placed(plan, placed)
    });
    Adding::Reading
}

/// What the chief is told of `plan` once its files are in place.
fn added(plan: &Plan) -> String {
    let named = format!("{} {}", plan.name, plan.version);
    match &plan.replaces {
        Some((was, true)) if plan.native => format!(
            "{named} is in the place of {was}. It loads when Hamlet \
             starts again, which asks you first."
        ),
        Some((was, true)) => format!(
            "{named} is in the place of {was}. It loads as an \
             update, and anything new it asks for is asked first."
        ),
        _ => format!(
            "{named} is in the install folder. Needs You asks \
             whether to install it."
        ),
    }
}
