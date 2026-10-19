//! The folder a hamlet works on, on the hamlet's page: the system's
//! dialog to choose one, the way to stop, and the switch between
//! manual and auto.

use std::path::Path;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, segmented,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_core::agents::{Settlement, Works};
use hamlet_desktop::folder::{ChooseFolder, FolderChosen};
use hamlet_realm_api::orders::Switched;
use hamlet_realm_api::pick::{Thing, select};
use hamlet_rules::work::{Mode, Work};
use hamlet_sdk::cards::heading;

use crate::View;
use crate::chat::decree_for;
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{action, bad_action, bare, brass, buttons, note};
use crate::roll::hamlets_moved;

/// The most earlier folders offered again.
const EARLIER: usize = 3;

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(switched).add_observer(chosen);
}

/// A press on a hamlet's clockwork switches its mode. With no folder
/// it shows the hamlet's page, where one is chosen.
fn switched(switched: On<Switched>, mut commands: Commands) {
    let hamlet = switched.hamlet;
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
        decree_for(world, hamlet, format!("/mode {}", to.name()));
    });
}

/// Points the hamlet at the folder its dialog gave.
fn chosen(chosen: On<FolderChosen>, mut commands: Commands) {
    let FolderChosen { asker, folder } = chosen.event().clone();
    commands.queue(move |world: &mut World| {
        work_on(world, asker, &folder)
    });
}

fn work_on(world: &mut World, hamlet: Entity, folder: &Path) {
    decree_for(
        world,
        hamlet,
        format!("/folder {}", folder.display()),
    );
}

/// The last part of `folder`, as a button names it.
fn name_of(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
}

/// The folder `hamlet` works on, with what changes it.
pub(crate) fn section(hamlet: Entity) -> impl View {
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
                world.trigger(ChooseFolder {
                    asker: hamlet,
                    title: "Choose the folder this hamlet works on"
                        .into(),
                });
            },
        );
        let mut parts =
            vec![heading(Glyph::Folder, "WORKS ON").boxed()];
        match &work.folder {
            Some(folder) => {
                parts.push(
                    label(folder.display().to_string())
                        .size(13.0)
                        .boxed(),
                );
                parts.push(
                    buttons(vec![
                        choose,
                        bad_action(
                            Glyph::Cross,
                            "STOP",
                            move |world| {
                                decree_for(
                                    world,
                                    hamlet,
                                    "/folder none".to_string(),
                                )
                            },
                        ),
                    ])
                    .boxed(),
                );
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
                                decree_for(
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
                parts.push(buttons(vec![choose]).boxed());
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
            parts.push(buttons(earlier).boxed());
        }
        column(parts).gap(6.0).align(AlignItems::FlexStart).boxed()
    })
    .within(bare())
}
