//! The Workspace page: where agents' commands run and their houses
//! are kept.

use bevy::app::{App, PreUpdate};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{frame, label, text_field};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Settings;
use hamlet_protocol::client as protocol;

use super::{Pane, field, told};
use crate::icons::Glyph;
use crate::looks::{
    action, bare, beside, choices, description, note, parts, well,
};
use crate::secret::secret_field;
use crate::{AnyView, PanelSystems, View};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Draft>()
        .add_observer(answered)
        // Before fynix mounts the page the pane opens, so its fields
        // start from the draft.
        .add_systems(
            PreUpdate,
            begin
                .run_if(resource_changed::<Pane>)
                .in_set(PanelSystems::Follow),
        );
}

const CHOICES: [&str; 2] = ["THIS COMPUTER", "REMOTE SERVER"];

/// The workspace as the page has it until it is saved.
#[derive(Resource, Default)]
struct Draft {
    remote: bool,
    url: String,
    /// Never received: the server keeps it and shows only the
    /// address.
    token: String,
    saving: Option<protocol::RequestId>,
    saved: bool,
    /// The move of the houses asked for and not yet answered.
    moving: Option<protocol::RequestId>,
    refusal: String,
}

/// Starts the draft from the chosen workspace each time the page
/// opens.
fn begin(
    pane: Res<Pane>,
    settings: Res<Settings>,
    mut draft: ResMut<Draft>,
) {
    if *pane != Pane::Workspace {
        return;
    }
    let url = settings.0.workspace.url.clone();
    *draft = Draft {
        remote: url.is_some(),
        url: url.unwrap_or_default(),
        ..Draft::default()
    };
}

fn save(world: &mut World) {
    let draft = world.resource::<Draft>();
    let workspace =
        draft.remote.then(|| protocol::EditSettingsSetWorkspace {
            url: draft.url.trim().into(),
            token: draft.token.clone(),
        });
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Settings {
            id,
            set: protocol::EditSettingsSet {
                workspace: Some(workspace),
                model: None,
                endpoints: None,
                effort: None,
                reflect_every: None,
                allow_net: None,
            },
        })
    });
    let mut draft = world.resource_mut::<Draft>();
    draft.saving = Some(id);
    draft.saved = false;
    draft.refusal.clear();
}

/// Asks the server to copy the houses to the saved workspace. The
/// settings it sends show how far it has come.
fn move_houses(world: &mut World) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::MoveHouses(protocol::MoveHouses {
            id,
        })
    });
    let mut draft = world.resource_mut::<Draft>();
    draft.moving = Some(id);
    draft.refusal.clear();
}

fn answered(response: On<Response>, mut draft: ResMut<Draft>) {
    if draft.moving.as_ref() == Some(&response.to) {
        draft.moving = None;
        match &response.message {
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Error =>
            {
                draft.refusal.clone_from(&reply.text);
            }
            protocol::ServerMessage::Fault(fault) => {
                draft.refusal.clone_from(&fault.text);
            }
            _ => {}
        }
        return;
    }
    if draft.saving.as_ref() != Some(&response.to) {
        return;
    }
    match &response.message {
        protocol::ServerMessage::Reply(reply)
            if reply.kind == protocol::ReplyKind::Error =>
        {
            draft.refusal.clone_from(&reply.text);
        }
        protocol::ServerMessage::Fault(fault) => {
            draft.refusal.clone_from(&fault.text);
        }
        protocol::ServerMessage::Reply(_) => {
            draft.token.clear();
            draft.saved = true;
        }
        _ => return,
    }
    draft.saving = None;
}

/// Whether the saved choice waits for a restart.
fn is_waiting_for_restart(settings: &Settings) -> bool {
    let workspace = &settings.0.workspace;
    workspace.url != workspace.running
}

fn pending(settings: &Settings) -> String {
    if is_waiting_for_restart(settings) {
        "Takes effect when Hamlet restarts.".into()
    } else {
        String::new()
    }
}

/// The latest move of the houses, as the server tells it.
fn move_of(
    settings: &Settings,
) -> Option<&protocol::SettingsViewWorkspaceMove> {
    settings.0.workspace.moving.as_ref()
}

/// What moving the houses does, how far it has come or how it ended.
/// A failure is the refusal's to tell.
fn progress(settings: &Settings) -> String {
    use protocol::SettingsViewWorkspaceMoveState as State;
    if !is_waiting_for_restart(settings) {
        return String::new();
    }
    match move_of(settings) {
        Some(latest) if latest.state == State::Moving => {
            format!(
                "Moving houses: {} of {}.",
                latest.done, latest.of
            )
        }
        Some(latest) if latest.state == State::Moved => {
            latest.text.clone()
        }
        _ => {
            let to = if settings.0.workspace.url.is_some() {
                "the remote server"
            } else {
                "this computer"
            };
            format!(
                "Copies every agent's house to {to}, replacing the \
                 houses there."
            )
        }
    }
}

/// The button that moves the houses to the saved workspace, while it
/// waits for a restart and no move runs.
fn moving() -> AnyView {
    keyed(
        resource::<Settings, _>(|settings| {
            is_waiting_for_restart(settings)
                && move_of(settings).is_none_or(|latest| {
                    latest.state
                        != protocol::SettingsViewWorkspaceMoveState::Moving
                })
        }),
        |offered| {
            if *offered {
                action(Glyph::Move, "MOVE HOUSES", move_houses)
            } else {
                frame().boxed()
            }
        },
    )
    .within(bare())
    .boxed()
}

/// The address and token of a remote workspace, while one is chosen.
fn remote() -> AnyView {
    keyed(resource::<Draft, _>(|draft| draft.remote), |remote| {
        if !*remote {
            return frame().boxed();
        }
        parts((
            field(
                "Server address",
                text_field(
                    resource::<Draft, _>(|draft| draft.url.clone()),
                    |world, url| {
                        world.resource_mut::<Draft>().url = url;
                    },
                )
                .rules(well),
            ),
            field(
                "Access token",
                secret_field(
                    resource::<Draft, _>(|draft| draft.token.clone()),
                    |world, token| {
                        world.resource_mut::<Draft>().token = token;
                    },
                ),
            ),
        ))
        .boxed()
    })
    .within(bare())
    .boxed()
}

pub(super) fn page() -> impl View {
    let where_run =
        keyed(resource::<Draft, _>(|draft| draft.remote), |remote| {
            choices(CHOICES, usize::from(*remote), |world, at| {
                world.resource_mut::<Draft>().remote = at == 1;
            })
            .boxed()
        })
        .within(bare());
    parts((
        field(
            "Where agents' commands run",
            told(
                where_run,
                description(resource::<Draft, _>(|draft| {
                    if draft.remote {
                        "Agents' commands run on a remote server, \
                         and their houses are kept there."
                    } else {
                        "Agents' commands run on this computer, and \
                         their houses are kept here."
                    }
                    .to_string()
                })),
            ),
        ),
        remote(),
        beside((
            action(Glyph::Check, "SAVE", save),
            note(resource::<Draft, _>(|draft| {
                if draft.saved { "Saved." } else { "" }.to_string()
            })),
        )),
        note(resource::<Settings, _>(pending)),
        moved(),
        label(resource::<Draft, _>(|draft| draft.refusal.clone()))
            .tone(Tone::Critical),
    ))
}

/// Moving the houses, once a saved workspace waits for a restart.
fn moved() -> impl View {
    beside((moving(), note(resource::<Settings, _>(progress))))
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::text::{
        EditableText, FontSize, FontSource, TextEdit, TextFont,
    };
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;

    use super::super::Pane;
    use crate::testing::{
        control, press, requests, settings_page, settle,
        words_and_spans,
    };

    fn received(
        app: &mut App,
        revision: i64,
        url: Option<&str>,
        running: Option<&str>,
    ) {
        received_moving(app, revision, url, running, None);
    }

    fn received_moving(
        app: &mut App,
        revision: i64,
        url: Option<&str>,
        running: Option<&str>,
        moving: Option<protocol::SettingsViewWorkspaceMove>,
    ) {
        let mut settings = hamlet_testing::realm::state().settings;
        settings.workspace = protocol::SettingsViewWorkspace {
            url: url.map(Into::into),
            running: running.map(Into::into),
            moving,
        };
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: 1,
                revision,
                changes: vec![protocol::Update::Settings {
                    settings,
                }],
            }),
        );
        settle(app);
    }

    fn type_into(app: &mut App, name: &str, text: &str) {
        let field = control::<EditableText>(app, name);
        app.world_mut().trigger(bevy::input_focus::FocusGained {
            entity: field,
            cause: bevy::input_focus::FocusCause::Pressed,
        });
        app.world_mut()
            .get_mut::<EditableText>(field)
            .unwrap()
            .editor_mut()
            .set_text(text);
        app.world_mut()
            .trigger(bevy::input_focus::FocusLost { entity: field });
        settle(app);
    }

    /// The edits left to apply after selecting all, copying, cutting
    /// and pasting in the field.
    fn kept_edits(app: &mut App, name: &str) -> Vec<TextEdit> {
        let field = control::<EditableText>(app, name);
        let mut text =
            app.world_mut().get_mut::<EditableText>(field).unwrap();
        for edit in [
            TextEdit::SelectAll,
            TextEdit::Copy,
            TextEdit::Cut,
            TextEdit::Paste,
        ] {
            text.queue_edit(edit);
        }
        settle(app);
        let mut text =
            app.world_mut().get_mut::<EditableText>(field).unwrap();
        std::mem::take(&mut text.pending_edits)
    }

    #[test]
    fn a_remote_workspace_is_saved_with_its_token_out_of_sight_and_applies_after_a_restart()
     {
        let token = "token-0123456789abcdef";
        let mut app = settings_page(Pane::Front);
        received(&mut app, 2, None, None);
        *app.world_mut().resource_mut::<Pane>() = Pane::Workspace;
        settle(&mut app);
        let said = |app: &mut App, text: &str| {
            words_and_spans(app).iter().any(|shown| shown == text)
        };
        assert!(!said(
            &mut app,
            "Takes effect when Hamlet restarts."
        ));
        assert!(!said(&mut app, "Server address"));
        press(&mut app, "REMOTE SERVER");
        settle(&mut app);
        type_into(
            &mut app,
            "Server address",
            " https://workspace.example ",
        );
        type_into(&mut app, "Access token", token);
        assert!(said(&mut app, &"*".repeat(token.len())));
        assert!(
            !words_and_spans(&mut app)
                .iter()
                .any(|shown| shown.contains(token))
        );
        // The stars are in the hidden text's own monospace face,
        // where the caret lands on them.
        let mono = bevy::asset::uuid_handle!(
            "6f1c1d3e-0b7a-4c55-9a43-2d0f7a1c9e21"
        );
        app.insert_resource(crate::platform::Mono(mono.clone()));
        let field = control::<EditableText>(&mut app, "Access token");
        app.world_mut()
            .get_mut::<TextFont>(field)
            .unwrap()
            .font_size = FontSize::Px(17.0);
        settle(&mut app);
        let hidden =
            app.world().get::<TextFont>(field).unwrap().clone();
        assert_eq!(hidden.font, FontSource::Handle(mono));
        let stars = "*".repeat(token.len());
        let world = app.world_mut();
        assert!(
            world
                .query::<(&bevy::ui::widget::Text, &TextFont)>()
                .iter(world)
                .any(
                    |(text, face)| text.0 == stars && *face == hidden
                )
        );
        // The token is pasted in but never copied or cut out.
        assert_eq!(
            kept_edits(&mut app, "Access token"),
            [TextEdit::SelectAll, TextEdit::Paste]
        );
        assert_eq!(
            kept_edits(&mut app, "Server address"),
            [
                TextEdit::SelectAll,
                TextEdit::Copy,
                TextEdit::Cut,
                TextEdit::Paste
            ]
        );
        requests(&mut app);
        press(&mut app, "SAVE");
        let Some(protocol::ClientMessage::Edit(
            protocol::Edit::Settings { id, .. },
        )) = app.world().resource::<Outbox>().iter().next().cloned()
        else {
            panic!("no settings edit was sent");
        };
        assert_eq!(
            requests(&mut app),
            [serde_json::json!({
                "type": "edit", "doc": "settings", "set": {"workspace": {
                    "url": "https://workspace.example", "token": token,
                }}
            })]
        );
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to: id,
                kind: protocol::ReplyKind::Notice,
                text: "Saved.".into(),
                upload: None,
            }),
        );
        received(
            &mut app,
            3,
            Some("https://workspace.example"),
            None,
        );
        assert!(said(&mut app, "Saved."));
        assert!(!said(&mut app, &"*".repeat(token.len())));
        assert!(said(&mut app, "Takes effect when Hamlet restarts."));

        *app.world_mut().resource_mut::<Pane>() = Pane::Front;
        settle(&mut app);
        *app.world_mut().resource_mut::<Pane>() = Pane::Workspace;
        settle(&mut app);
        let address =
            control::<EditableText>(&mut app, "Server address");
        assert_eq!(
            app.world().get::<EditableText>(address).unwrap().value(),
            "https://workspace.example"
        );
        press(&mut app, "THIS COMPUTER");
        settle(&mut app);
        press(&mut app, "SAVE");
        assert_eq!(
            requests(&mut app),
            [serde_json::json!({
                "type": "edit", "doc": "settings", "set": {"workspace": null}
            })]
        );
    }

    #[test]
    fn houses_move_to_the_saved_workspace_with_their_progress_and_refusal_shown()
     {
        use protocol::SettingsViewWorkspaceMoveState as State;
        let url = Some("https://workspace.example");
        let mut app = settings_page(Pane::Front);
        received(&mut app, 2, None, None);
        *app.world_mut().resource_mut::<Pane>() = Pane::Workspace;
        settle(&mut app);
        let said = |app: &mut App, text: &str| {
            words_and_spans(app).iter().any(|shown| shown == text)
        };
        // Nothing waits for a restart, so nothing moves.
        assert!(!said(&mut app, "MOVE HOUSES"));
        received(&mut app, 3, url, None);
        assert!(said(
            &mut app,
            "Copies every agent's house to the remote server, \
             replacing the houses there."
        ));
        requests(&mut app);
        press(&mut app, "MOVE HOUSES");
        let Some(protocol::ClientMessage::MoveHouses(
            protocol::MoveHouses { id },
        )) = app.world().resource::<Outbox>().iter().next().cloned()
        else {
            panic!("no move was sent");
        };
        assert_eq!(
            requests(&mut app),
            [serde_json::json!({"type": "move-houses"})]
        );
        let moving = |done, state, text: &str| {
            Some(protocol::SettingsViewWorkspaceMove {
                state,
                done,
                of: 2,
                text: text.into(),
            })
        };
        received_moving(
            &mut app,
            4,
            url,
            None,
            moving(1, State::Moving, ""),
        );
        assert!(said(&mut app, "Moving houses: 1 of 2."));
        let stopped = "The house of agent 1 in home was not moved.";
        received_moving(
            &mut app,
            5,
            url,
            None,
            moving(1, State::Failed, stopped),
        );
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to: id,
                kind: protocol::ReplyKind::Error,
                text: stopped.into(),
                upload: None,
            }),
        );
        settle(&mut app);
        assert_eq!(
            words_and_spans(&mut app)
                .iter()
                .filter(|shown| *shown == stopped)
                .count(),
            1
        );
        let moved = "Moved 2 houses to the remote server.";
        received_moving(
            &mut app,
            6,
            url,
            None,
            moving(2, State::Moved, moved),
        );
        assert!(said(&mut app, moved));
    }
}
