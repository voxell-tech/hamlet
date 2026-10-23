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
use hamlet_client::realm::{Mode, Works};
use hamlet_desktop::folder::{ChooseFolder, FolderChosen};
use hamlet_realm::pick::{Thing, select};
use hamlet_realm::works::Switched;

use crate::View;
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{action, bad_action, bare, brass, buttons, note};
use crate::overview::{heading, tell};
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
        let Some(work) = world.get::<Works>(hamlet) else {
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
        tell(
            world,
            hamlet,
            format!(
                "/mode {}",
                match to {
                    Mode::Manual => "manual",
                    Mode::Auto => "auto",
                }
            ),
        );
    });
}

/// Points the hamlet at the folder its dialog gave.
fn chosen(chosen: On<FolderChosen>, mut commands: Commands) {
    let FolderChosen { asker, folder } = chosen.event().clone();
    commands.queue(move |world: &mut World| {
        work_on(world, asker, &folder);
    });
}

fn work_on(world: &mut World, hamlet: Entity, folder: &Path) {
    tell(world, hamlet, format!("/folder {}", folder.display()));
}

/// The folder `hamlet` works on, with what changes it.
pub(crate) fn section(hamlet: Entity) -> impl View {
    let work = derived(move |world: &World| {
        world.get::<Works>(hamlet).cloned()
    })
    .when(hamlets_moved());
    keyed(work, move |work| {
        let Some(work) = work else {
            return column(()).boxed();
        };
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
                parts.push(label(folder.clone()).size(13.0).boxed());
                parts.push(
                    buttons(vec![
                        choose,
                        bad_action(
                            Glyph::Cross,
                            "NONE",
                            move |world| {
                                tell(
                                    world,
                                    hamlet,
                                    "/folder none".to_string(),
                                );
                            },
                        ),
                    ])
                    .boxed(),
                );
            }
            None => {
                parts.push(small("No folder selected.".to_string()));
                parts.push(buttons(vec![choose]).boxed());
            }
        }
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
                            format!(
                                "/mode {}",
                                match to {
                                    Mode::Manual => "manual",
                                    Mode::Auto => "auto",
                                }
                            ),
                        );
                    }
                },
            )
            .boxed(),
        );
        parts.push(small(work.words.clone()));
        parts.push(small(
            "Folders are paths on the server's machine.".into(),
        ));
        if !work.before.is_empty() {
            let earlier = work
                .before
                .iter()
                .take(EARLIER)
                .map(|folder| {
                    let folder = folder.clone();
                    bevy_fynix::views::button(label(folder.clone()))
                        .padding(crate::looks::pad())
                        .rules(brass)
                        .tagged(Mood::Plain)
                        .on_activate(move |world| {
                            work_on(
                                world,
                                hamlet,
                                Path::new(&folder),
                            );
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

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_client::realm::Settlement;
    use hamlet_protocol::client as protocol;

    use super::*;
    use crate::testing::{panel_app, press, words};

    #[test]
    fn the_server_folder_history_and_mode_send_commands_to_its_mayor()
    {
        let mut app = panel_app();
        app.add_plugins((plugin, crate::overview::plugin));
        let mut state = hamlet_testing::realm::state();
        state.hamlets[0].folder =
            Some("/on-the-server/current".into());
        state.hamlets[0].before = (1..=4)
            .map(|n| format!("/on-the-server/earlier-{n}"))
            .collect();
        state.hamlets[0].mode_words =
            "The server holds each command for its chief.".into();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state.clone()),
        );
        let hamlet = app
            .world_mut()
            .query::<(Entity, &Settlement)>()
            .iter(app.world())
            .next()
            .unwrap()
            .0;
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            crate::overview::hamlet(hamlet),
        );
        app.update();
        let shown = words(&mut app);
        for expected in [
            "/on-the-server/current",
            "/on-the-server/earlier-1",
            "/on-the-server/earlier-2",
            "/on-the-server/earlier-3",
            "MANUAL",
            "The server holds each command for its chief.",
        ] {
            assert!(
                shown.iter().any(|text| text == expected),
                "missing {expected}"
            );
        }
        assert!(!shown.iter().any(|text| text.contains("earlier-4")));
        press(&mut app, "AUTO");
        press(&mut app, "/on-the-server/earlier-2");
        app.world_mut().trigger(FolderChosen {
            asker: hamlet,
            folder: "/chosen/on-the-server/with spaces".into(),
        });
        app.world_mut().flush();
        press(&mut app, "NONE");
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .filter_map(|message| match message {
                protocol::ClientMessage::Say(say) => {
                    assert_eq!(
                        say.to,
                        protocol::Address {
                            hamlet: "home".into(),
                            agent: "mayor".into()
                        }
                    );
                    Some(say.text.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            sent,
            [
                "/mode auto",
                "/folder /on-the-server/earlier-2",
                "/folder /chosen/on-the-server/with spaces",
                "/folder none"
            ]
        );
        state.hamlets[0].folder = None;
        state.hamlets[0].mode_words =
            "The server says no folder is selected.".into();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        assert!(
            words(&mut app).iter().any(|text| text
                == "The server says no folder is selected.")
        );
    }
}
