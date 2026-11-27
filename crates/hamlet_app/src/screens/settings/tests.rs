use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::{Checkbox, ValueChange};
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;

use super::*;
use crate::testing::{
    control, requests, settings_page, settle, words_and_spans,
};

#[test]
fn agent_controls_send_only_the_changed_setting_and_follow_received_changes()
 {
    let mut app = settings_page(Pane::Agents);
    let network =
        control::<Checkbox>(&mut app, "Tools network access");
    app.world_mut().trigger(ValueChange {
        source: network,
        value: true,
        is_final: true,
    });
    app.world_mut().flush();
    assert_eq!(
        requests(&mut app),
        [serde_json::json!({
            "type": "edit", "doc": "settings", "set": {"allowNet": true}
        })]
    );

    let awake = control::<Checkbox>(&mut app, "Keep awake");
    app.world_mut().trigger(ValueChange {
        source: awake,
        value: true,
        is_final: true,
    });
    app.world_mut().flush();
    assert!(app.world().resource::<KeepAwake>().0);
    assert!(requests(&mut app).is_empty());

    let reflection =
        control::<EditableText>(&mut app, "Reflect every N turns");
    app.world_mut().trigger(bevy::input_focus::FocusGained {
        entity: reflection,
        cause: bevy::input_focus::FocusCause::Pressed,
    });
    app.world_mut().flush();
    app.world_mut()
        .get_mut::<EditableText>(reflection)
        .unwrap()
        .editor_mut()
        .set_text("5");
    app.world_mut()
        .trigger(bevy::input_focus::FocusLost { entity: reflection });
    app.world_mut().flush();
    assert_eq!(
        requests(&mut app),
        [serde_json::json!({
            "type": "edit", "doc": "settings", "set": {"reflectEvery": 5}
        })]
    );

    let mut settings = hamlet_testing::realm::state().settings;
    settings.allow_net = true;
    settings.reflect_every = 7;
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::Change(protocol::Changes {
            epoch: 1,
            revision: 2,
            changes: vec![protocol::Update::Settings { settings }],
        }),
    );
    settle(&mut app);
    assert!(app.world().get::<Checked>(network).is_some());
    assert_eq!(
        app.world().get::<EditableText>(reflection).unwrap().value(),
        "7"
    );
}
#[test]
fn received_grants_show_the_giver_and_revoke_through_the_hand() {
    let mut app = settings_page(Pane::Agents);
    let grant = protocol::Grant {
        agent: Some(protocol::AgentId(2)),
        name: "hire#2".into(),
        permission: protocol::Permission::Network,
        by: Some("chief".into()),
        source: protocol::GrantSource::Grant,
    };
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::Change(protocol::Changes {
            epoch: 1,
            revision: 2,
            changes: vec![protocol::Update::Grants {
                grants: vec![grant],
            }],
        }),
    );
    settle(&mut app);
    assert!(
        words_and_spans(&mut app)
            .iter()
            .any(|text| text
                == "ada may use: network (given by chief)")
    );
    crate::testing::press(&mut app, "Revoke");
    assert_eq!(
        requests(&mut app),
        [serde_json::json!({
            "type": "say", "to": 1, "text": "/revoke hire#2 network"
        })]
    );
}

#[test]
fn limits_permissions_belong_to_bylaws_and_cannot_be_revoked_here() {
    let mut app = settings_page(Pane::Agents);
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::Change(protocol::Changes {
            epoch: 1,
            revision: 2,
            changes: vec![protocol::Update::Grants {
                grants: vec![protocol::Grant {
                    agent: None,
                    name: "hire#2".into(),
                    permission: protocol::Permission::Network,
                    by: None,
                    source: protocol::GrantSource::Limits,
                }],
            }],
        }),
    );
    settle(&mut app);
    let shown = words_and_spans(&mut app).join("\n");
    assert!(!shown.contains("Revoke"));
    assert!(!shown.contains("given by limits"));
}

#[test]
fn the_charter_page_reads_the_server_text_and_whether_the_realm_keeps_its_own()
 {
    let mut app = settings_page(Pane::Charter);
    let sent = app
        .world()
        .resource::<Outbox>()
        .iter()
        .find_map(|message| match message {
            protocol::ClientMessage::Read(
                protocol::Read::Charter { id },
            ) => Some(id.clone()),
            _ => None,
        })
        .expect("the charter page requests its text");
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::Read(
            protocol::ReadResult::Charter {
                to: sent,
                charter: protocol::Charter {
                    text: "Keep the orchard kind.".into(),
                    own: true,
                },
            },
        ),
    );
    settle(&mut app);
    let shown = words_and_spans(&mut app);
    assert!(
        shown.iter().any(|text| text == "Keep the orchard kind."),
        "{shown:?}"
    );
    assert!(shown.iter().any(|text| text == "Edited"), "{shown:?}");
    assert!(shown.iter().any(|text| text == "Back to the default"));
}

#[test]
fn a_paragraphs_lead_becomes_a_heading_and_other_colons_stay() {
    let text = "You are an agent of ours: kind.\n\nTools: \
                `bash` runs.\n- A list: item.\n\nHow you \
                improve yourself:\n- Notes.\n\nhttps://a.example \
                helps.";
    assert_eq!(
        with_heads(text),
        "You are an agent of ours: kind.\n\n### Tools\n`bash` \
         runs.\n- A list: item.\n\n### How you improve \
         yourself\n- Notes.\n\nhttps://a.example helps."
    );
}

#[derive(Resource, Default)]
struct Edits(Vec<Edit>);

#[test]
fn charter_edit_saves_and_confirmed_reset_go_to_the_server_and_refusals_stay_by_the_buttons()
 {
    use hamlet_desktop::editor::{Edit, Edited};
    let mut app = settings_page(Pane::Charter);
    let id = app
        .world()
        .resource::<Outbox>()
        .iter()
        .filter_map(|message| match message {
            protocol::ClientMessage::Read(
                protocol::Read::Charter { id },
            ) => Some(id.clone()),
            _ => None,
        })
        .last()
        .unwrap();
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::Read(
            protocol::ReadResult::Charter {
                to: id,
                charter: protocol::Charter {
                    text: "Keep the orchard kind.".into(),
                    own: true,
                },
            },
        ),
    );
    app.init_resource::<Edits>().add_observer(
        |edit: On<Edit>, mut edits: ResMut<Edits>| {
            edits.0.push(edit.event().clone());
        },
    );
    settle(&mut app);
    requests(&mut app);
    crate::testing::press(&mut app, "Edit");
    let edits = &app.world().resource::<Edits>().0;
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].key, "charter");
    assert_eq!(edits[0].text, "Keep the orchard kind.");
    app.world_mut().trigger(Edited {
        key: "default-character-other".into(),
        text: "Ignore this save".into(),
    });
    assert!(requests(&mut app).is_empty());
    app.world_mut().trigger(Edited {
        key: "charter".into(),
        text: "New charter words.".into(),
    });
    app.world_mut().flush();
    let edit = app
        .world()
        .resource::<Outbox>()
        .iter()
        .next()
        .cloned()
        .unwrap();
    let protocol::ClientMessage::Edit(protocol::Edit::Charter {
        id,
        ..
    }) = edit
    else {
        panic!("{edit:?}");
    };
    assert_eq!(
        requests(&mut app),
        [
            serde_json::json!({"type": "edit", "doc": "charter", "text": "New charter words."})
        ]
    );
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::Reply(protocol::Reply {
            to: id,
            kind: protocol::ReplyKind::Error,
            text: "The charter was refused.".into(),
            upload: None,
            image: None,
            images: Vec::new(),
            file: None,
            files: Vec::new(),
        }),
    );
    settle(&mut app);
    assert!(
        words_and_spans(&mut app)
            .iter()
            .any(|text| text == "The charter was refused.")
    );

    app.init_resource::<world_menu::Open>();
    bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
        app.world_mut(),
        world_menu::view(),
    );
    crate::testing::press(&mut app, "Back to the default");
    settle(&mut app);
    assert!(requests(&mut app).is_empty());
    assert!(
        words_and_spans(&mut app)
            .iter()
            .any(|text| text == "Go back to the default charter?")
    );
    crate::testing::press(&mut app, "Use the default");
    let id = app
        .world()
        .resource::<Outbox>()
        .iter()
        .find_map(|message| match message {
            protocol::ClientMessage::Edit(
                protocol::Edit::Charter { id, .. },
            ) => Some(id.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        requests(&mut app),
        [
            serde_json::json!({"type": "edit", "doc": "charter", "text": null})
        ]
    );
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::Reply(protocol::Reply {
            to: id,
            kind: protocol::ReplyKind::Notice,
            text: "The default charter is back.".into(),
            upload: None,
            image: None,
            images: Vec::new(),
            file: None,
            files: Vec::new(),
        }),
    );
    let read = app
        .world_mut()
        .resource_mut::<Outbox>()
        .drain()
        .find_map(|message| match message {
            protocol::ClientMessage::Read(
                protocol::Read::Charter { id },
            ) => Some(id),
            _ => None,
        })
        .expect("the accepted reset refreshes the charter");
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::Read(
            protocol::ReadResult::Charter {
                to: read,
                charter: protocol::Charter {
                    text: "The server's default words.".into(),
                    own: false,
                },
            },
        ),
    );
    settle(&mut app);
    let shown = words_and_spans(&mut app);
    assert!(shown.iter().any(|text| text == "Default"));
    assert!(
        shown
            .iter()
            .any(|text| text == "The server's default words.")
    );
    assert!(!shown.iter().any(|text| text == "Back to the default"));
    assert!(
        !shown.iter().any(|text| text == "The charter was refused.")
    );
}
#[test]
fn reconnecting_reads_the_open_charter_again_and_shows_a_read_refusal()
 {
    let mut app = settings_page(Pane::Charter);
    requests(&mut app);
    app.world_mut().trigger(Reconnected);
    app.world_mut().flush();
    let sent = app
        .world()
        .resource::<Outbox>()
        .iter()
        .find_map(|message| match message {
            protocol::ClientMessage::Read(
                protocol::Read::Charter { id },
            ) => Some(id.clone()),
            _ => None,
        })
        .expect("a fresh charter after reconnecting");
    hamlet_client::receive(
        app.world_mut(),
        protocol::ServerMessage::Fault(protocol::Fault {
            to: Some(sent),
            text: "Cannot read the charter.".into(),
        }),
    );
    settle(&mut app);
    assert!(
        words_and_spans(&mut app)
            .iter()
            .any(|text| text == "Cannot read the charter.")
    );
}
