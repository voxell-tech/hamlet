//! The Settings panel and the pages it has of its own: endpoints,
//! models, MCP servers, agent behaviour, workspace, charter and
//! realm limits. The window's other parts add their own pages with
//! [`hamlet_core::settings::SettingsAppExt::add_settings_page`].

pub mod mcp;
#[cfg(test)]
mod testing;
mod visits;
pub mod workspace;

use bevy::app::{App, Update};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use bevy::ui::{AlignItems, Overflow, OverflowAxis, px};
use hamlet_core::client::messages::{Reconnected, Response};
use hamlet_core::client::outbox::Outbox;
use hamlet_core::client::realm::{
    Confinement, Grants, Providers, Settings,
};
use hamlet_core::confirm::Confirm;
use hamlet_core::desktop::awake::KeepAwake;
use hamlet_core::desktop::editor::{Edit, Edited};
use hamlet_core::fynix::dock::DockWindowKind;
use hamlet_core::fynix::shortcut::{
    Chord, CommandId, CommandSpec, GLOBAL, Scope, ShortcutAppExt,
};
use hamlet_core::fynix::tokens::Tone;
use hamlet_core::fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, dropdown, frame,
    label, number_field, row, scroll,
};
use hamlet_core::fynix::{
    Binding, Prop, ScopedExt, ViewExt, derived, keyed, resource,
};
use hamlet_core::menus::{MenuAppExt, MenuItem};
use hamlet_core::orders::Decree;
use hamlet_core::panels::{PanelAppExt, ShowPanel, panel};
use hamlet_core::protocol::{Effort, client as protocol};
use hamlet_core::roll::{either, moved};
use hamlet_core::saving::Saving;
use hamlet_core::settings::{
    ENDPOINTS, MODELS, PANEL, SettingsAppExt, SettingsPage,
    SettingsPages, SettingsPane, field, shown, told,
};
use hamlet_core::ui::styles::{
    Fills, ICON, action, bad_action, bare, beside, breadcrumb,
    buttons, close, description, filling, list, nav_list, note,
    parts, pill, small_rules, text_input, title, valued_row,
};
use hamlet_core::ui::widgets::edges::faded;
use hamlet_core::ui::widgets::ellipsis::cut;
use hamlet_core::ui::widgets::icons::Glyph;
use hamlet_core::ui::widgets::keys::Keys;
use hamlet_core::ui::widgets::lasting::Lasting;
use hamlet_core::ui::widgets::switch::switch;
use hamlet_core::ui::{AnyView, View, built, with_cx};
use hamlet_core::welcome::{OpenProjects, OpenWelcome};

/// The keys that bring Settings forward.
const KEYS: Chord = Keys::new(KeyCode::Comma).primary().chord();

pub fn plugin(app: &mut App) {
    app.add_panel(
        PANEL,
        DockWindowKind::new("Settings", || panel(screen()))
            .icon(Glyph::Sliders.image(ICON)),
    )
    .add_command(
        CommandSpec {
            id: CommandId("app.settings"),
            label: "Settings",
            scope: GLOBAL,
            run: |world, _| world.trigger(ShowPanel(PANEL.into())),
            enabled: |_| true,
            repeat: false,
        },
        &[KEYS],
    )
    .add_menu_item(
        MenuItem::new(Glyph::Sliders, "Settings", 10, |commands| {
            commands.trigger(ShowPanel(PANEL.into()));
        })
        .keys(KEYS),
    )
    .add_plugins((mcp::plugin, visits::plugin, workspace::plugin))
    // Each provider's limits are under it, filled by
    // `hamlet_core::plans::plugin`.
    .init_resource::<hamlet_core::plans::Plans>()
    .init_resource::<SettingsPane>()
    .init_resource::<Charter>()
    .init_resource::<CharterRequests>()
    .add_observer(charter_received)
    .add_observer(charter_saved)
    .add_observer(default_character_saved)
    .add_observer(
        |_: On<Reconnected>,
         pane: Res<SettingsPane>,
         outbox: ResMut<Outbox>,
         requests: ResMut<CharterRequests>| {
            read_charter(pane, outbox, requests);
        },
    )
    .add_systems(
        Update,
        read_charter.run_if(resource_changed::<SettingsPane>),
    )
    .add_settings_page(
        SettingsPage::new(
            ENDPOINTS,
            "Endpoints",
            Glyph::Signal,
            10,
            || hamlet_core::models::endpoints().boxed(),
        )
        .now(endpoints_now),
    )
    .add_settings_page(
        SettingsPage::new(
            MODELS,
            "Models",
            Glyph::Chip,
            20,
            hamlet_core::models::shelf,
        )
        .now(models_now)
        .wide(),
    )
    .add_settings_page(
        SettingsPage::new(
            MCP,
            "MCP Servers",
            Glyph::Bolt,
            30,
            || mcp::page().boxed(),
        )
        .now(mcp_now),
    )
    .add_settings_page(
        SettingsPage::new(
            AGENTS,
            "Agent Behaviour",
            Glyph::Robot,
            40,
            || agents().boxed(),
        )
        .now(agents_now),
    )
    .add_settings_page(
        SettingsPage::new(
            CHARTER,
            "Charter",
            Glyph::Notes,
            60,
            || charter_page().boxed(),
        )
        .now(charter_now)
        .beside_title(|| {
            pill(resource::<Charter, _>(|charter| {
                charter_state(charter).to_string()
            }))
            .boxed()
        }),
    )
    .add_settings_page(SettingsPage::new(
        REALM,
        "Realm Limits",
        Glyph::Globe,
        70,
        hamlet_core::limits::realm,
    ));
}

/// The id of the page of the MCP servers whose tools agents are
/// offered.
pub const MCP: &str = "mcp";
/// The id of the Agent Behaviour page.
pub const AGENTS: &str = "agents";
/// The id of the page of the words every agent's prompt opens with.
pub const CHARTER: &str = "charter";
/// The id of the page of the limits that hold for every hamlet.
pub const REALM: &str = "realm";

fn endpoints_now() -> Prop<String> {
    derived(|world: &World| {
        let endpoints =
            world.resource::<Settings>().0.endpoints.len();
        let providers = world.get_resource::<Providers>().map_or(
            0,
            |providers| {
                providers
                    .0
                    .iter()
                    .filter(|provider| provider.configured)
                    .count()
            },
        );
        match endpoints + providers {
            0 => "None".to_string(),
            n => format!("{n} connected"),
        }
    })
    .when(either(moved::<Settings>(), moved::<Providers>()))
    .into()
}

fn models_now() -> Prop<String> {
    shown::<Settings>(|s| {
        hamlet_core::models::model_name(&s.0.model).to_string()
    })
}

fn mcp_now() -> Prop<String> {
    shown::<Settings>(|s| match s.0.mcp.servers.len() {
        0 => "None".to_string(),
        1 => "1 server".to_string(),
        n => format!("{n} servers"),
    })
}

fn agents_now() -> Prop<String> {
    shown::<Settings>(|s| match s.0.reflect_every {
        0 => "Reflection off".to_string(),
        n => format!("Reflect every {n}"),
    })
}

/// Read only while its page is open.
fn charter_now() -> Prop<String> {
    derived(|world: &World| {
        if !world.resource::<CharterRequests>().loaded {
            return String::new();
        }
        charter_state(world.resource::<Charter>()).to_string()
    })
    .when(either(moved::<Charter>(), moved::<CharterRequests>()))
    .into()
}

/// A warning under the network switch while it is not enforced.
fn not_enforced() -> AnyView {
    keyed(
        resource::<Confinement, _>(|c| c.0.enforces_network()),
        |enforced| {
            if *enforced {
                frame().boxed()
            } else {
                label("Not enforced in insecure mode.")
                    .bold(true)
                    .tone(Tone::Critical)
                    .rules(small_rules)
                    .boxed()
            }
        },
    )
    .within(row(()).gap(0.0))
    .boxed()
}

const EFFORTS: [(Option<Effort>, &str); 8] =
    hamlet_core::models::efforts("Model's own");

/// Who can let a profile into the city, as the pick shows them.
const NEWCOMERS: [(protocol::Newcomers, &str); 3] = [
    (protocol::Newcomers::Open, "Anyone"),
    (protocol::Newcomers::Chief, "A chief"),
    (protocol::Newcomers::Hand, "The Hand"),
];

/// The most letters of the default character shown under its edit.
const CHARACTER_SHOWN: usize = 90;

/// The longest default character the server takes, in letters.
const CHARACTER_MOST: usize = 4000;

/// The key of the default character in the editor.
const CHARACTER_KEY: &str = "default-character";

/// Opens the default character in the editor.
fn edit_default_character(world: &mut World) {
    let text = world.resource::<Settings>().0.character.clone();
    world.trigger(Edit {
        key: CHARACTER_KEY.into(),
        text,
    });
    world.flush();
}

/// Sends the default character the editor saved.
fn default_character_saved(
    edited: On<Edited>,
    mut commands: bevy::ecs::system::Commands,
) {
    if edited.key != CHARACTER_KEY {
        return;
    }
    let text = edited.text.chars().take(CHARACTER_MOST).collect();
    commands.queue(move |world: &mut World| {
        save(world, |set| set.character = Some(text));
    });
}

/// The settings of agents. Each applies when changed.
fn agents() -> impl View {
    parts((
        field(
            "Reflect every N turns",
            told(
                number_field(Binding::new(
                    resource::<Settings, _>(|s| s.0.reflect_every),
                    |world, turns| {
                        save(world, |set| {
                            set.reflect_every = Some(turns);
                        });
                    },
                ))
                .range(0i64, 1000i64)
                .width(px(90.0))
                .rules(text_input),
                note("0 means never."),
            ),
        ),
        field(
            "Thinking effort",
            told(
                built(|cx| {
                    cx.build(
                        dropdown(
                            EFFORTS.map(|(_, name)| name),
                            Binding::new(
                                resource::<Settings, _>(|s| {
                                    EFFORTS
                                        .iter()
                                        .position(|(effort, _)| {
                                            *effort == s.0.effort
                                        })
                                        .unwrap_or_default()
                                }),
                                |world, at: usize| {
                                    save(world, |set| {
                                        set.effort =
                                            Some(EFFORTS[at].0);
                                    });
                                },
                            ),
                            Glyph::Chevron.image(ICON),
                        )
                        .width(px(160.0)),
                    )
                }),
                note(
                    "How hard models think by default; an agent's \
                     own choice wins.",
                ),
            ),
        ),
        field(
            "Newcomers",
            told(
                built(|cx| {
                    cx.build(
                        dropdown(
                            NEWCOMERS.map(|(_, name)| name),
                            Binding::new(
                                resource::<Settings, _>(|s| {
                                    NEWCOMERS
                                        .iter()
                                        .position(|(who, _)| {
                                            *who == s.0.newcomers
                                        })
                                        .unwrap_or_default()
                                }),
                                |world, at: usize| {
                                    save(world, |set| {
                                        set.newcomers =
                                            Some(NEWCOMERS[at].0);
                                    });
                                },
                            ),
                            Glyph::Chevron.image(ICON),
                        )
                        .width(px(160.0)),
                    )
                }),
                description(resource::<Settings, _>(|s| {
                    match s.0.newcomers {
                        protocol::Newcomers::Open => {
                            "A profile an agent makes moves in at \
                             once."
                        }
                        protocol::Newcomers::Chief => {
                            "A profile an agent makes waits at the \
                             harbor until a chief welcomes it."
                        }
                        protocol::Newcomers::Hand => {
                            "The Hand lets a profile in or turns it \
                             away. You can undo what it decides."
                        }
                    }
                    .to_string()
                })),
            ),
        ),
        field(
            "Default character",
            told(
                action(Glyph::Rename, "Edit", edit_default_character)
                    .tooltip(|| {
                        note("Opens the text in your editor")
                    }),
                note(resource::<Settings, _>(|s| {
                    if s.0.character.is_empty() {
                        "A profile made blank starts empty.".into()
                    } else {
                        format!(
                            "A profile made blank starts from: {}",
                            cut(&s.0.character, CHARACTER_SHOWN)
                        )
                    }
                })),
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
                not_enforced(),
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

/// The lasting permissions agents were given, each revocable.
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
                    .map(|grant| {
                        let shown = hamlet_core::client::realm::named(
                            world,
                            &grant.name,
                        )
                        .map_or_else(
                            || grant.name.clone(),
                            |agent| {
                                hamlet_core::client::realm::shown_of(
                                    world, agent,
                                )
                            },
                        );
                        (grant.clone(), shown)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
    .when(moved::<Grants>());
    keyed(held, |held| {
        if held.is_empty() {
            return description(
                "None yet. Choosing Allow always on a wait adds one.",
            )
            .boxed();
        }
        let rows = held
            .iter()
            .map(|(grant, shown)| {
                let agent = grant.name.clone();
                let permission = match grant.permission {
                    protocol::Permission::Network => "network",
                    protocol::Permission::Unconfined => "unconfined",
                };
                let by = grant.by.as_deref().unwrap_or("limits");
                let says = format!(
                    "{shown} may use: {permission} (given by {by})"
                );
                with_cx(move |cx| {
                    row((
                        row((description(says),)).fills_width(),
                        bad_action(
                            Glyph::Cross,
                            "Revoke",
                            move |world| {
                                world.trigger(Decree {
                                    hamlet: None,
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
    /// The realm keeps one of its own.
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
    pane: Res<SettingsPane>,
    mut outbox: ResMut<Outbox>,
    mut requests: ResMut<CharterRequests>,
) {
    if pane.0 == Some(CHARTER) {
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
        world.trigger(Saving(id.clone()));
        let mut requests = world.resource_mut::<CharterRequests>();
        requests.error.clear();
        requests.edit = Some(id);
    });
}

/// A request to restore the server's default charter.
fn default_charter(world: &mut World) {
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

/// Whether the realm keeps a charter of its own, as its pill says.
fn charter_state(charter: &Charter) -> &'static str {
    if charter.own { "Edited" } else { "Default" }
}

/// `text` with each paragraph's "Lead:" made a heading.
fn with_heads(text: &str) -> String {
    let mut lines = Vec::new();
    let mut opens = true;
    for line in text.lines() {
        let lead = line.split_once(':').filter(|(lead, rest)| {
            opens
                && (1..=4).contains(&lead.split_whitespace().count())
                && lead.chars().all(|c| c.is_alphabetic() || c == ' ')
                && (rest.is_empty() || rest.starts_with(' '))
        });
        match lead {
            Some((lead, rest)) => {
                lines.push(format!("### {lead}"));
                if !rest.trim().is_empty() {
                    lines.push(rest.trim_start().to_string());
                }
            }
            None => lines.push(line.to_string()),
        }
        opens = line.trim().is_empty();
    }
    lines.join("\n")
}

/// The charter page.
fn charter_page() -> impl View {
    let actions = keyed(resource::<Charter, _>(|c| c.own), |own| {
        let edit = action(Glyph::Rename, "Edit", edit_charter)
            .tooltip(|| note("Opens the file in your editor"))
            .boxed();
        let mut actions = vec![edit];
        if *own {
            actions.push(action(
                Glyph::Restore,
                "Back to the Default",
                |world| {
                    world.trigger(Confirm {
                        question: "Go back to the default charter?",
                        outcome: "Your own charter is deleted from \
                                  disk. This cannot be undone.",
                        yes: "Use the Default",
                        act: default_charter,
                    });
                },
            ));
        }
        buttons(actions).boxed()
    })
    .within(bare());
    let shown = keyed(
        resource::<Charter, _>(|c| with_heads(&c.text)),
        |text| {
            hamlet_core::ui::markdown::page(text, Tone::Body).boxed()
        },
    )
    .within(bare());
    parts((
        close((
            actions,
            label(resource::<CharterRequests, _>(|requests| {
                requests.error.clone()
            }))
            .tone(Tone::Critical),
        )),
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
        character: None,
        effort: None,
        newcomers: None,
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

fn nav_item(page: &SettingsPage) -> AnyView {
    let id = page.id;
    valued_row(
        page.glyph,
        page.title,
        page.now.map(|now| now()),
        move |world| {
            world.resource_mut::<SettingsPane>().0 = Some(id);
        },
    )
}

/// The way back to the first-run screen, as a row of the index.
fn again() -> AnyView {
    hamlet_core::ui::styles::nav_row(
        Glyph::Restore,
        "First-Run Setup",
        None,
        |world| world.trigger(OpenWelcome::Again),
    )
}

/// The index row that opens the projects card.
fn projects() -> AnyView {
    hamlet_core::ui::styles::nav_row(
        Glyph::Hamlet,
        "Bring In Projects",
        None,
        |world| world.trigger(OpenProjects),
    )
}

/// The Settings panel's contents.
pub fn screen() -> AnyView {
    keyed(resource::<SettingsPane, _>(|pane| *pane), |&pane| {
        with_cx(move |cx| {
            let pages = cx.world.resource::<SettingsPages>();
            let (head, body, wide) = parts_of(pane, pages);
            let space = &cx.theme().space;
            let width = px(if wide {
                space.table_width
            } else {
                space.page_width
            });
            // Each page keeps where it was scrolled to.
            let scrolled =
                scroll((column((body,)).gap(0.0).max_width(width),))
                    .fills_height()
                    .overflow(Overflow {
                        x: OverflowAxis::Clip,
                        y: OverflowAxis::Scroll,
                    })
                    .tagged(Lasting(format!(
                        "settings.{}",
                        pane.0.unwrap_or("front")
                    )));
            column((
                column((head,)).max_width(width),
                faded(scrolled),
            ))
            .gap(space.section)
            .fills_height()
        })
        .boxed()
    })
    .within(filling().tagged(Scope(visits::KEYS)))
    .boxed()
}

/// The head of the page of `pane`, what is under it, and whether it
/// is as wide as a table.
fn parts_of(
    pane: SettingsPane,
    pages: &SettingsPages,
) -> (AnyView, AnyView, bool) {
    let Some(page) = pane.0.and_then(|id| pages.get(id)) else {
        let mut rows: Vec<AnyView> =
            pages.iter().map(nav_item).collect();
        rows.push(projects());
        rows.push(again());
        let list = with_cx(move |cx| nav_list(cx, rows)).boxed();
        return (title("Settings").boxed(), list, false);
    };
    // The path names the page; what the page has stands beside it.
    let path = breadcrumb(
        vec!["Settings".into()],
        page.title,
        |world, _| {
            world.resource_mut::<SettingsPane>().0 = None;
        },
    );
    let head = match page.beside_title {
        Some(beside_title) => beside((path, beside_title())).boxed(),
        None => path.boxed(),
    };
    (head, (page.body)(), page.wide)
}

#[cfg(test)]
mod tests {
    use bevy::text::EditableText;
    use bevy::ui::Checked;
    use bevy::ui_widgets::{Checkbox, ValueChange};
    use hamlet_core::protocol::client as protocol;
    use hamlet_core::testing::{control, requests, settle};

    use super::*;
    use crate::testing::settings_page;

    #[test]
    fn agent_controls_send_only_the_changed_setting_and_follow_received_changes()
     {
        let mut app = settings_page(Some(AGENTS));
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

        let mut settings =
            hamlet_core::testing::realm::state().settings;
        settings.allow_net = true;
        settings.reflect_every = 7;
        hamlet_core::client::receive(
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
    }
}
