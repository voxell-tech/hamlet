//! The settings screen: a front page of its parts, each on a page
//! of its own.

mod workspace;

use bevy::app::{App, Update};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, dropdown, label,
    number_field, row, scroll, slider,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::messages::{ChiefMessage, Reconnected, Response};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Grants, HOME, MAYOR, Settings};
use hamlet_desktop::awake::KeepAwake;
use hamlet_desktop::editor::{Edit, Edited};
use hamlet_protocol::client as protocol;
use hamlet_realm::sky::Quality;
use hamlet_realm::sound::SoundSettings;

use crate::dock::{PanelAppExt, panel};
use crate::fade::ScrollFade;
use crate::icons::Glyph;
use crate::looks::{
    Fills, ICON, action, bad_action, bare, buttons, close,
    description, doors, hairline, list, note, page_head, parts,
    section, well,
};
use crate::switch::switch;
use crate::world_menu::{self, Deed};
use crate::{AnyView, Cx, View, markdown, with_cx};

pub fn plugin(app: &mut App) {
    app.add_panel(
        "settings",
        DockWindowKind::new("SETTINGS", || panel(screen()))
            .icon(Glyph::Sliders.image(ICON)),
    )
    .add_plugins(workspace::plugin)
    .init_resource::<Pane>()
    .init_resource::<Charter>()
    .init_resource::<CharterRequests>()
    .add_observer(charter_received)
    .add_observer(charter_saved)
    .add_observer(
        |_: On<Reconnected>,
         pane: Res<Pane>,
         outbox: ResMut<Outbox>,
         requests: ResMut<CharterRequests>| {
            read_charter(pane, outbox, requests);
        },
    )
    .add_systems(
        Update,
        read_charter.run_if(resource_changed::<Pane>),
    );
}

/// A part of a page: its name over `value`.
fn field(name: &'static str, value: impl View) -> impl View {
    close((section(name), value))
}

/// The choice of the world's visual quality.
fn quality() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        cx.build(
            dropdown(
                Quality::ALL.map(Quality::name),
                resource::<Quality, _>(|q| {
                    Quality::ALL
                        .iter()
                        .position(|a| a == q)
                        .unwrap_or_default()
                }),
                Glyph::Chevron.image(ICON),
                |world, at| {
                    *world.resource_mut::<Quality>() =
                        Quality::ALL[at];
                },
            )
            .width(percent(100.0))
            .max_width(percent(100.0)),
        )
    })
}

/// A part of the settings, each on a page of its own.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    /// The list of the others.
    #[default]
    Front,
    Endpoints,
    Models,
    Agents,
    /// Where agents' commands run: this computer or a remote server.
    Workspace,
    /// The words every agent's prompt opens with.
    Charter,
    /// The limits that hold for every hamlet.
    Realm,
    Senses,
}

impl Pane {
    const PAGES: [Pane; 7] = [
        Pane::Endpoints,
        Pane::Models,
        Pane::Agents,
        Pane::Workspace,
        Pane::Charter,
        Pane::Realm,
        Pane::Senses,
    ];

    fn title(self) -> &'static str {
        match self {
            Pane::Front => "SETTINGS",
            Pane::Endpoints => "ENDPOINTS",
            Pane::Models => "MODELS",
            Pane::Agents => "AGENTS",
            Pane::Workspace => "WORKSPACE",
            Pane::Charter => "CHARTER",
            Pane::Realm => "REALM",
            Pane::Senses => "SOUND AND SIGHT",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Pane::Front => Glyph::Sliders,
            Pane::Endpoints => Glyph::Globe,
            Pane::Models => Glyph::Chip,
            Pane::Agents => Glyph::Robot,
            Pane::Workspace => Glyph::Shield,
            Pane::Charter => Glyph::Notes,
            Pane::Realm => Glyph::Signpost,
            Pane::Senses => Glyph::Sun,
        }
    }

    fn holds(self) -> &'static str {
        match self {
            Pane::Front => "",
            Pane::Endpoints => {
                "Where models come from, and their keys."
            }
            Pane::Models => {
                "The default model, which models are allowed and \
                 their prices."
            }
            Pane::Agents => {
                "Reflection, network access and keeping awake."
            }
            Pane::Workspace => {
                "Where agents' commands run and their houses are \
                 kept."
            }
            Pane::Charter => "What every agent is told.",
            Pane::Realm => "Limits that apply to every hamlet.",
            Pane::Senses => "Sound, music and how much is drawn.",
        }
    }
}

/// A setting's switch or field, and under it its words.
fn told(control: impl View, words: impl View) -> impl View {
    with_cx(move |cx| {
        column((control, words))
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::FlexStart)
    })
}

/// The settings of agents. Each applies when changed.
fn agents() -> impl View {
    parts((
        field(
            "Reflect every N turns",
            told(
                number_field(
                    resource::<Settings, _>(|s| s.0.reflect_every),
                    |world, turns| {
                        save(world, |set| {
                            set.reflect_every = Some(turns);
                        });
                    },
                )
                .range(0i64, 1000i64)
                .width(px(90.0))
                .rules(well),
                note("0 means never."),
            ),
        ),
        field(
            "Tools network access",
            told(
                switch(
                    resource::<Settings, _>(|s| s.0.allow_net),
                    |world, on| {
                        save(world, |set| set.allow_net = Some(on));
                    },
                ),
                description(resource::<Settings, _>(|s| {
                    if s.0.allow_net {
                        "Agents' commands can reach the internet."
                            .to_string()
                    } else {
                        "Agents' commands cannot reach the internet."
                            .to_string()
                    }
                })),
            ),
        ),
        field(
            "Keep awake",
            told(
                switch(
                    resource::<KeepAwake, _>(|k| k.0),
                    |world, on| {
                        world.resource_mut::<KeepAwake>().0 = on;
                    },
                ),
                description(
                    "The computer stays awake while an agent works.",
                ),
            ),
        ),
        field("Lasting permissions", permissions()),
    ))
}

/// The permissions agents were given for good from a petition, each
/// to be taken back.
fn permissions() -> AnyView {
    let held = derived(|world: &World| {
        world
            .get_resource::<Grants>()
            .map(|grants| {
                grants
                    .0
                    .iter()
                    .filter(|grant| {
                        grant.source == protocol::GrantSource::Grant
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
    .when(crate::roll::moved::<Grants>());
    keyed(held, |held| {
        if held.is_empty() {
            return description(
                "None yet. Choosing Always Allow on a petition adds \
                 one.",
            )
            .boxed();
        }
        let rows = held
            .iter()
            .map(|grant| {
                let agent = grant.name.clone();
                let permission = match grant.permission {
                    protocol::Permission::Network => "network",
                    protocol::Permission::Unconfined => "unconfined",
                };
                let by = grant.by.as_deref().unwrap_or("limits");
                let hamlet = grant
                    .agent
                    .as_ref()
                    .map(|agent| agent.hamlet.clone())
                    .unwrap_or_else(|| HOME.into());
                let says = format!(
                    "{agent} may use: {permission} (given by {by})"
                );
                with_cx(move |cx| {
                    row((
                        row((description(says),)).fills_width(),
                        bad_action(
                            Glyph::Cross,
                            "REVOKE",
                            move |world| {
                                world.trigger(ChiefMessage {
                                    to: protocol::Address {
                                        hamlet: hamlet.clone(),
                                        agent: MAYOR.into(),
                                    },
                                    text: format!(
                                        "/revoke {agent} \
                                         {permission}"
                                    ),
                                });
                                world.flush();
                            },
                        ),
                    ))
                    .gap(cx.theme().space.section)
                    .align(AlignItems::Center)
                })
                .boxed()
            })
            .collect::<Vec<_>>();
        list(rows).boxed()
    })
    .within(bare())
    .boxed()
}

/// The charter as its page shows it.
#[derive(Resource, Default, PartialEq)]
pub struct Charter {
    pub text: String,
    /// The home keeps one of its own.
    pub own: bool,
}

#[derive(Resource, Default)]
struct CharterRequests {
    read: Option<protocol::RequestId>,
    edit: Option<protocol::RequestId>,
    error: String,
    loaded: bool,
}

fn read_charter(
    pane: Res<Pane>,
    mut outbox: ResMut<Outbox>,
    mut requests: ResMut<CharterRequests>,
) {
    if *pane == Pane::Charter {
        requests.read = Some(outbox.send(|id| {
            protocol::ClientMessage::Read(protocol::Read::Charter {
                id,
            })
        }));
    }
}

fn charter_received(
    response: On<Response>,
    mut requests: ResMut<CharterRequests>,
    mut charter: ResMut<Charter>,
    mut outbox: ResMut<Outbox>,
) {
    let reading = requests.read.as_ref() == Some(&response.to);
    let editing = requests.edit.as_ref() == Some(&response.to);
    if !reading && !editing {
        return;
    }
    match &response.message {
        protocol::ServerMessage::Read(
            protocol::ReadResult::Charter {
                charter: received, ..
            },
        ) if reading => {
            *charter = Charter {
                text: received.text.clone(),
                own: received.own,
            };
            requests.loaded = true;
        }
        protocol::ServerMessage::Reply(reply)
            if reply.kind == protocol::ReplyKind::Error =>
        {
            requests.error.clone_from(&reply.text);
        }
        protocol::ServerMessage::Fault(fault) => {
            requests.error.clone_from(&fault.text);
        }
        protocol::ServerMessage::Reply(_) if editing => {
            requests.read = Some(outbox.send(|id| {
                protocol::ClientMessage::Read(
                    protocol::Read::Charter { id },
                )
            }));
        }
        _ => return,
    }
    if reading {
        requests.read = None;
    }
    if editing {
        requests.edit = None;
    }
}

fn edit_charter(world: &mut World) {
    if !world.resource::<CharterRequests>().loaded {
        return;
    }
    let text = world.resource::<Charter>().text.clone();
    world.trigger(Edit {
        key: "charter".into(),
        text,
    });
    world.flush();
}

fn charter_saved(
    edited: On<Edited>,
    mut commands: bevy::ecs::system::Commands,
) {
    if edited.key != "charter" {
        return;
    }
    let text = edited.text.clone();
    commands.queue(move |world: &mut World| {
        let id = world.resource_mut::<Outbox>().send(|id| {
            protocol::ClientMessage::Edit(protocol::Edit::Charter {
                id,
                text: Some(text),
            })
        });
        crate::saving::track(world, id.clone());
        let mut requests = world.resource_mut::<CharterRequests>();
        requests.error.clear();
        requests.edit = Some(id);
    });
}

/// A request to restore the server's default charter.
pub(crate) fn default_charter(world: &mut World) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Charter {
            id,
            text: None,
        })
    });
    let mut requests = world.resource_mut::<CharterRequests>();
    requests.error.clear();
    requests.edit = Some(id);
}

/// The words every agent's prompt opens with, and the ways to change
/// them and to have the built-in ones again.
fn charter_page(cx: &Cx) -> impl View + use<> {
    let actions = keyed(resource::<Charter, _>(|c| c.own), |own| {
        let edit = action(Glyph::Rename, "EDIT", edit_charter)
            .tooltip(|| note("Opens the file in your editor"))
            .boxed();
        let mut actions = vec![edit];
        if *own {
            actions.push(action(
                Glyph::Restore,
                "BACK TO THE DEFAULT",
                |world| world_menu::ask(world, Deed::DefaultCharter),
            ));
        }
        buttons(actions).boxed()
    })
    .within(bare());
    let shown =
        keyed(resource::<Charter, _>(|c| c.text.clone()), |text| {
            markdown::fixed(text, Tone::Body).boxed()
        })
        .within(bare());
    parts((
        description(
            "Every agent reads this at the start of each turn.",
        ),
        note(resource::<Charter, _>(|charter| {
            if charter.own {
                "The home's own charter.".to_string()
            } else {
                "The default charter.".to_string()
            }
        })),
        actions,
        label(resource::<CharterRequests, _>(|requests| {
            requests.error.clone()
        }))
        .tone(Tone::Critical),
        hairline(cx.theme().palette.brass_6),
        shown,
    ))
}

/// An edit of one received setting.
fn save(
    world: &mut World,
    change: impl FnOnce(&mut protocol::EditSettingsSet),
) {
    let mut set = protocol::EditSettingsSet {
        model: None,
        endpoints: None,
        reflect_every: None,
        allow_net: None,
        workspace: None,
    };
    change(&mut set);
    world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Settings {
            id,
            set,
        })
    });
}

/// A switch and a slider: a kind of sound that is on or off, and its
/// level.
fn level(
    on: fn(&SoundSettings) -> bool,
    turn: fn(&mut SoundSettings, bool),
    at: fn(&SoundSettings) -> f32,
    set: fn(&mut SoundSettings, f32),
) -> impl View {
    with_cx(move |cx| {
        row((
            switch(
                resource::<SoundSettings, _>(on),
                move |world, to| {
                    turn(
                        &mut world.resource_mut::<SoundSettings>(),
                        to,
                    );
                },
            ),
            slider(resource::<SoundSettings, _>(at))
                .on_change(move |world, to| {
                    set(
                        &mut world.resource_mut::<SoundSettings>(),
                        to,
                    );
                })
                .grow(1.0),
        ))
        .gap(cx.theme().space.gap)
        .width(percent(100.0))
        .align(AlignItems::Center)
    })
}

/// A switch alone: a group of sounds that is on or off.
fn sound_switch(
    on: fn(&SoundSettings) -> bool,
    turn: fn(&mut SoundSettings, bool),
) -> impl View {
    switch(resource::<SoundSettings, _>(on), move |world, to| {
        turn(&mut world.resource_mut::<SoundSettings>(), to);
    })
}

fn senses() -> impl View {
    parts((
        field(
            "Visual quality",
            told(
                quality(),
                description("Particles, bloom and shadows."),
            ),
        ),
        field(
            "Sound",
            level(
                |s| !s.muted,
                |s, on| s.muted = !on,
                |s| s.sound_level,
                |s, level| s.sound_level = level,
            ),
        ),
        field(
            "Music",
            level(
                |s| s.music_on,
                |s, on| s.music_on = on,
                |s| s.music_level,
                |s, level| s.music_level = level,
            ),
        ),
        field(
            "Wildlife",
            told(
                sound_switch(
                    |s| s.wildlife_on,
                    |s, on| s.wildlife_on = on,
                ),
                description("Birds, crickets and other animals."),
            ),
        ),
        field(
            "Ambience",
            told(
                sound_switch(
                    |s| s.ambience_on,
                    |s, on| s.ambience_on = on,
                ),
                description(
                    "The wind and the other sounds of the land.",
                ),
            ),
        ),
    ))
}

fn door(pane: Pane) -> AnyView {
    crate::looks::list_door(
        pane.glyph(),
        pane.title(),
        pane.holds(),
        move |world| *world.resource_mut::<Pane>() = pane,
    )
}

/// The way back to the first-run screen.
fn again() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        column((
            description("Choose again where your model comes from."),
            action(
                Glyph::Restore,
                "SET UP AGAIN",
                crate::welcome::again,
            ),
        ))
        .gap(gap / 2.0)
        .padding(UiRect::top(px(gap)))
        .align(AlignItems::FlexStart)
    })
}

/// The Settings panel's contents.
pub fn screen() -> AnyView {
    let page = keyed(resource::<Pane, _>(|pane| *pane), |pane| {
        if *pane == Pane::Front {
            let (pages, foot) =
                (Pane::PAGES.map(door).into(), again().boxed());
            return with_cx(move |cx| doors(cx, pages, foot));
        }
        let body = match pane {
            Pane::Endpoints => crate::models::endpoints().boxed(),
            Pane::Models => crate::models::shelf().boxed(),
            Pane::Agents => agents().boxed(),
            Pane::Workspace => workspace::page().boxed(),
            Pane::Charter => with_cx(charter_page),
            Pane::Realm => crate::limits::realm().boxed(),
            Pane::Front | Pane::Senses => senses().boxed(),
        };
        let head = page_head(
            "SETTINGS",
            |world| *world.resource_mut::<Pane>() = Pane::Front,
            pane.glyph(),
            pane.title(),
        );
        with_cx(move |cx| {
            column((head, body)).gap(cx.theme().space.panel)
        })
    });
    AnyView::new(move |cx: &mut Cx| {
        let width = cx.theme().space.page_width;
        let page =
            page.within(column(()).gap(0.0).max_width(px(width)));
        cx.build(
            scroll((page,))
                .fills_height()
                .tagged(ScrollFade::default()),
        )
    })
}

#[cfg(test)]
mod tests {
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

        let reflection = control::<EditableText>(
            &mut app,
            "Reflect every N turns",
        );
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
        app.world_mut().trigger(bevy::input_focus::FocusLost {
            entity: reflection,
        });
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
                changes: vec![protocol::Update::Settings {
                    settings,
                }],
            }),
        );
        settle(&mut app);
        assert!(app.world().get::<Checked>(network).is_some());
        assert_eq!(
            app.world()
                .get::<EditableText>(reflection)
                .unwrap()
                .value(),
            "7"
        );
        assert!(
            words_and_spans(&mut app).iter().any(|text| text
                == "Agents' commands can reach the internet.")
        );
    }
    #[test]
    fn received_grants_show_the_giver_and_revoke_through_the_mayor() {
        let mut app = settings_page(Pane::Agents);
        assert!(words_and_spans(&mut app).iter().any(|text| text
            == "None yet. Choosing Always Allow on a petition \
                adds one."));
        let grant = protocol::Grant {
            agent: Some(protocol::AgentRef {
                hamlet: "home".into(),
                number: 0,
            }),
            name: "founder".into(),
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
        assert!(words_and_spans(&mut app).iter().any(|text| text
            == "founder may use: network (given by chief)"));
        crate::testing::press(&mut app, "REVOKE");
        assert_eq!(
            requests(&mut app),
            [serde_json::json!({
                "type": "say", "to": {"hamlet": "home", "agent": "mayor"}, "text": "/revoke founder network"
            })]
        );
    }

    #[test]
    fn limits_permissions_belong_to_bylaws_and_cannot_be_revoked_here()
     {
        let mut app = settings_page(Pane::Agents);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: 1,
                revision: 2,
                changes: vec![protocol::Update::Grants {
                    grants: vec![protocol::Grant {
                        agent: None,
                        name: "founder".into(),
                        permission: protocol::Permission::Network,
                        by: None,
                        source: protocol::GrantSource::Limits,
                    }],
                }],
            }),
        );
        settle(&mut app);
        let shown = words_and_spans(&mut app).join("\n");
        assert!(
            shown.contains(
                "None yet. Choosing Always Allow on a petition adds \
                 one."
            ),
            "{shown}"
        );
        assert!(!shown.contains("REVOKE"));
        assert!(!shown.contains("given by limits"));
    }

    #[test]
    fn the_charter_page_reads_the_server_text_and_whether_the_home_keeps_its_own()
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
        assert!(
            shown
                .iter()
                .any(|text| text == "The home's own charter."),
            "{shown:?}"
        );
        assert!(
            shown.iter().any(|text| text == "BACK TO THE DEFAULT")
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
        crate::testing::press(&mut app, "EDIT");
        let edits = &app.world().resource::<Edits>().0;
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].key, "charter");
        assert_eq!(edits[0].text, "Keep the orchard kind.");
        app.world_mut().trigger(Edited {
            key: "founder-character".into(),
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
            }),
        );
        settle(&mut app);
        assert!(
            words_and_spans(&mut app)
                .iter()
                .any(|text| text == "The charter was refused.")
        );

        app.init_resource::<world_menu::Open>();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            world_menu::view(),
        );
        crate::testing::press(&mut app, "BACK TO THE DEFAULT");
        settle(&mut app);
        assert!(requests(&mut app).is_empty());
        assert!(words_and_spans(&mut app).iter().any(|text| text == "GO BACK TO THE DEFAULT CHARTER?"));
        crate::testing::press(&mut app, "USE THE DEFAULT");
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
        assert!(
            shown.iter().any(|text| text == "The default charter.")
        );
        assert!(
            shown
                .iter()
                .any(|text| text == "The server's default words.")
        );
        assert!(
            !shown.iter().any(|text| text == "BACK TO THE DEFAULT")
        );
        assert!(
            !shown
                .iter()
                .any(|text| text == "The charter was refused.")
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
}
