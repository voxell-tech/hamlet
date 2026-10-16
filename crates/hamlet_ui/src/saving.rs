//! Closing the window only after editor saves are confirmed.

use std::collections::HashMap;

use bevy::app::{App, AppExit, Update};
use bevy::ecs::prelude::*;
use bevy_fynix::views::{FrameProps, column};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_client::messages::{Reconnected, Response};
use hamlet_desktop::editor;
use hamlet_protocol::v0;

use crate::icons::Glyph;
use crate::looks::{action, bad_action, note};
use crate::{AnyView, View};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Saves>()
        .add_observer(answered)
        .add_observer(reconnected)
        .add_systems(Update, settle);
}

#[derive(Resource, Default)]
pub(crate) struct Saves {
    pending: HashMap<String, Option<String>>,
    closing: bool,
}

pub(crate) fn track(world: &mut World, id: String) {
    world
        .get_resource_or_init::<Saves>()
        .pending
        .insert(id, None);
}

pub(crate) fn close(world: &mut World) {
    editor::poll(world);
    if world.get_resource_or_init::<Saves>().pending.is_empty() {
        world.write_message(AppExit::Success);
    } else {
        world.resource_mut::<Saves>().closing = true;
    }
}

fn answered(response: On<Response>, mut saves: ResMut<Saves>) {
    if !saves.pending.contains_key(&response.to) {
        return;
    }
    match &response.message {
        v0::ServerMessage::Reply(reply)
            if reply.kind == v0::ReplyKind::Notice =>
        {
            saves.pending.remove(&response.to);
        }
        v0::ServerMessage::Reply(reply) => {
            saves.pending.insert(
                response.to.clone(),
                Some(reply.text.clone()),
            );
        }
        v0::ServerMessage::Fault(fault) => {
            saves.pending.insert(
                response.to.clone(),
                Some(fault.text.clone()),
            );
        }
        _ => {}
    }
}

fn reconnected(_: On<Reconnected>, mut saves: ResMut<Saves>) {
    for error in saves.pending.values_mut() {
        *error = Some(
            "The connection was lost before this save was \
             confirmed. Inspect the server document before applying \
             the kept copy again."
                .into(),
        );
    }
}

fn settle(world: &mut World) {
    if world.resource::<Saves>().closing {
        close(world);
    }
}

pub(crate) fn screen() -> impl View {
    keyed(
        resource::<Saves, _>(|s| (s.closing, s.pending.clone())),
        |(closing, pending)| {
            if !closing {
                return column(()).boxed();
            }
            let errors = pending
                .values()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            let text = if errors.is_empty() {
                "Waiting for the server to confirm editor saves. \
                 Closing now keeps local copies, but the server may \
                 not have saved them."
                    .into()
            } else {
                errors
            };
            AnyView::new(move |cx: &mut crate::Cx| {
                let copies = editor::copies(cx.world)
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                let panel = cx.build(
                    column((
                        note(text),
                        note(format!("Recovery copies:\n{copies}")),
                        action(Glyph::Back, "KEEP OPEN", |world| {
                            world.resource_mut::<Saves>().closing =
                                false
                        }),
                        bad_action(
                            Glyph::Power,
                            "CLOSE WITH COPIES",
                            |world| {
                                world.write_message(AppExit::Success);
                            },
                        ),
                    ))
                    .gap(8.0)
                    .padding(bevy::ui::UiRect::all(bevy::ui::px(
                        24.0,
                    )))
                    .fill(cx.theme().palette.card),
                );
                cx.world
                    .entity_mut(panel)
                    .insert(bevy::ui::GlobalZIndex(1000));
                if let Some(mut node) =
                    cx.world.get_mut::<bevy::ui::Node>(panel)
                {
                    node.position_type =
                        bevy::ui::PositionType::Absolute;
                    node.left = bevy::ui::percent(20.0);
                    node.top = bevy::ui::px(60.0);
                    node.width = bevy::ui::percent(60.0);
                }
                panel
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use bevy::app::AppExit;
    use bevy::ecs::prelude::*;
    use bevy::ecs::system::In;
    use hamlet_client::outbox::Outbox;
    use hamlet_desktop::editor::{Edit, Opener};
    use hamlet_protocol::v0;

    #[derive(Resource, Default)]
    struct Opened(Vec<std::path::PathBuf>);

    #[test]
    fn closing_after_a_last_editor_save_waits_for_the_servers_receipt()
     {
        let mut app = crate::me::tests::panel_app();
        app.add_plugins((
            hamlet_desktop::DesktopPlugin { window: false },
            super::plugin,
            crate::settings::plugin,
        ));
        app.init_resource::<Opened>();
        let opener = app.world_mut().register_system(
            |In(path): In<std::path::PathBuf>,
             mut opened: ResMut<Opened>| {
                opened.0.push(path);
                true
            },
        );
        app.insert_resource(Opener(opener));
        app.world_mut().trigger(Edit {
            key: "charter".into(),
            text: "old".into(),
        });
        app.update();
        let file = app.world().resource::<Opened>().0[0].clone();
        std::fs::write(&file, "The last save").unwrap();
        crate::window::quit(app.world_mut());
        assert_eq!(app.should_exit(), None);
        let id = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|m| match m {
                v0::ClientMessage::Edit(v0::Edit::Charter {
                    id,
                    text: Some(text),
                }) if text == "The last save" => Some(id.clone()),
                _ => None,
            })
            .expect("the final poll queues the edit before closing");
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Reply(v0::Reply {
                to: id,
                kind: v0::ReplyKind::Notice,
                text: "Saved.".into(),
                upload: None,
            }),
        );
        app.update();
        assert_eq!(app.should_exit(), Some(AppExit::Success));
        drop(app);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "The last save"
        );
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }
    #[test]
    fn an_in_flight_save_keeps_the_window_open_and_offers_visible_recovery_copies()
     {
        use crate::me::tests::{press, words};
        let mut app = crate::me::tests::panel_app();
        app.add_plugins((
            hamlet_desktop::DesktopPlugin { window: false },
            super::plugin,
            crate::settings::plugin,
        ));
        app.init_resource::<Opened>();
        let opener = app.world_mut().register_system(
            |In(path): In<std::path::PathBuf>,
             mut opened: ResMut<Opened>| {
                opened.0.push(path);
                true
            },
        );
        app.insert_resource(Opener(opener));
        app.world_mut().trigger(Edit {
            key: "charter".into(),
            text: "old".into(),
        });
        app.update();
        let file = app.world().resource::<Opened>().0[0].clone();
        std::fs::write(&file, "Offline save").unwrap();
        hamlet_desktop::editor::poll(app.world_mut());
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        crate::window::quit(app.world_mut());
        assert_eq!(app.should_exit(), None);
        app.world_mut()
            .trigger(hamlet_client::messages::Reconnected);
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            super::screen(),
        );
        app.update();
        let shown = words(&mut app).join("\n");
        assert!(shown.contains("Inspect the server document"));
        assert!(shown.contains(file.to_str().unwrap()));
        press(&mut app, "CLOSE WITH COPIES");
        app.update();
        assert_eq!(app.should_exit(), Some(AppExit::Success));
        drop(app);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "Offline save"
        );
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }
}
