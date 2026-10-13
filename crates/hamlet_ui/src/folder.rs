//! The folder a hamlet works on, on the hamlet's page: the system's
//! dialog to choose one, the way to stop, and the switch between
//! manual and auto.

use std::path::{Path, PathBuf};

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, NonSendMarker, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, segmented,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_core::agents::{Settlement, Works};
use hamlet_core::core::Tokio;
use hamlet_rules::work::{Mode, Work};
use hamlet_world::pick::{Thing, select};
use hamlet_world::works::Pulled;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::Ui;
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{action, bad_action, bare, brass, buttons, note};
use crate::overview::{heading, tell};
use crate::roll::hamlets_moved;

/// The most earlier folders offered again.
const EARLIER: usize = 3;

/// The hamlet a folder was chosen for, and the folder. None when the
/// dialog was closed with nothing chosen.
type Chosen = (Entity, Option<PathBuf>);

/// The dialog a folder is chosen in.
#[derive(Resource)]
pub(crate) struct Picker {
    /// The hamlet whose folder the chief asked to choose.
    asked: Option<Entity>,
    /// A dialog is open.
    open: bool,
    tx: UnboundedSender<Chosen>,
    rx: UnboundedReceiver<Chosen>,
}

impl Default for Picker {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Picker {
            asked: None,
            open: false,
            tx,
            rx,
        }
    }
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Picker>()
        .add_observer(pulled)
        .add_systems(Update, (open, take));
}

/// A press on a hamlet's lever switches its mode. With no folder it
/// shows the hamlet's page, where one is chosen.
fn pulled(pulled: On<Pulled>, mut commands: Commands) {
    let hamlet = pulled.hamlet;
    commands.queue(move |world: &mut World| {
        let Some(work) = world.get::<Works>(hamlet).map(|w| &w.0)
        else {
            return;
        };
        if work.folder.is_none() {
            select(world, Some(Thing::Common(hamlet)));
            return;
        }
        let to = match work.mode {
            Mode::Manual => Mode::Auto,
            Mode::Auto => Mode::Manual,
        };
        tell(world, hamlet, format!("/mode {}", to.name()));
    });
}

/// Opens the system's folder dialog for the hamlet that asked. The
/// marker keeps this on the main thread, where macOS wants a dialog
/// begun.
fn open(
    _main: NonSendMarker,
    mut picker: ResMut<Picker>,
    tokio: Option<Res<Tokio>>,
) {
    if picker.open {
        picker.asked = None;
        return;
    }
    let (Some(hamlet), Some(tokio)) = (picker.asked.take(), tokio)
    else {
        return;
    };
    picker.open = true;
    let chosen = rfd::AsyncFileDialog::new()
        .set_title("Choose the folder this hamlet works on")
        .pick_folder();
    let tx = picker.tx.clone();
    tokio.0.spawn(async move {
        let folder = chosen.await.map(|f| f.path().to_path_buf());
        let _ = tx.send((hamlet, folder));
    });
}

/// Points the hamlet at the folder its dialog gave.
fn take(mut picker: ResMut<Picker>, mut commands: Commands) {
    while let Ok((hamlet, folder)) = picker.rx.try_recv() {
        picker.open = false;
        if let Some(folder) = folder {
            commands.queue(move |world: &mut World| {
                work_on(world, hamlet, &folder)
            });
        }
    }
}

fn work_on(world: &mut World, hamlet: Entity, folder: &Path) {
    tell(world, hamlet, format!("/folder {}", folder.display()));
}

/// The last part of `folder`, as a button names it.
fn name_of(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
}

/// The folder `hamlet` works on, with what changes it.
pub(crate) fn section(hamlet: Entity) -> Ui {
    let work = derived(move |world: &World| {
        let mut work = world
            .get::<Settlement>(hamlet)
            .map(|settlement| settlement.record().work())
            .unwrap_or_default();
        // One that is gone cannot be chosen again.
        work.before.retain(|folder| folder.is_dir());
        work.before.truncate(EARLIER);
        work
    })
    .when(hamlets_moved());
    keyed(work, move |work: &Work| {
        let small = |text: String| note(text).boxed();
        let choose = action(
            Glyph::Folder,
            match work.folder {
                Some(_) => "CHOOSE ANOTHER",
                None => "CHOOSE A FOLDER",
            },
            move |world| {
                world.resource_mut::<Picker>().asked = Some(hamlet);
            },
        );
        let mut parts = vec![heading(Glyph::Folder, "WORKS ON")];
        match &work.folder {
            Some(folder) => {
                parts.push(
                    label(folder.display().to_string())
                        .size(13.0)
                        .boxed(),
                );
                parts.push(buttons(vec![
                    choose,
                    bad_action(Glyph::Cross, "STOP", move |world| {
                        tell(
                            world,
                            hamlet,
                            "/folder none".to_string(),
                        )
                    }),
                ]));
                let mode = work.mode;
                parts.push(
                    segmented(
                        ["MANUAL", "AUTO"],
                        match mode {
                            Mode::Manual => 0,
                            Mode::Auto => 1,
                        },
                        move |world, chosen| {
                            let to = match chosen {
                                0 => Mode::Manual,
                                _ => Mode::Auto,
                            };
                            if to != mode {
                                tell(
                                    world,
                                    hamlet,
                                    format!("/mode {}", to.name()),
                                );
                            }
                        },
                    )
                    .boxed(),
                );
                parts.push(small(mode.says().to_string()));
            }
            None => {
                parts.push(small(
                    "No folder. Its agents work in their own \
                     workspaces."
                        .to_string(),
                ));
                parts.push(buttons(vec![choose]));
            }
        }
        if !work.before.is_empty() {
            let earlier = work
                .before
                .iter()
                .map(|folder| {
                    let folder = folder.clone();
                    bevy_fynix::views::button(
                        label(name_of(&folder)).wrap(false),
                    )
                    .padding(crate::looks::pad())
                    .rules(brass)
                    .tagged(Mood::Plain)
                    .on_activate(move |world| {
                        work_on(world, hamlet, &folder)
                    })
                    .boxed()
                })
                .collect::<Vec<_>>();
            parts.push(small("Worked on before:".to_string()));
            parts.push(buttons(earlier));
        }
        column(parts).gap(6.0).align(AlignItems::FlexStart).boxed()
    })
    .within(bare())
    .boxed()
}
