//! The settings screen: a front page of its parts, each on a page
//! of its own.

mod appearance;
mod sight;
mod visits;
pub(crate) mod workspace;

use bevy::app::{App, Update};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Overflow, OverflowAxis, percent, px};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::shortcut::Scope;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, dropdown, frame,
    label, number_field, row, scroll, slider,
};
use bevy_fynix::{
    Prop, ScopedExt, ViewExt, derived, keyed, resource,
};
use hamlet_client::messages::{Reconnected, Response};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{
    Confinement, Grants, Providers, Settings,
};
use hamlet_desktop::awake::KeepAwake;
use hamlet_desktop::editor::{Edit, Edited};
use hamlet_protocol::{Effort, client as protocol};
use hamlet_realm::sky::Quality;
use hamlet_realm::sound::SoundSettings;
use hamlet_ui::styles::{
    Fills, ICON, action, bad_action, bare, beside, breadcrumb,
    buttons, close, description, filling, list, nav_list, note,
    parts, pill, section, small_rules, text_input, title, valued_row,
};
use hamlet_ui::widgets::edges::faded;
use hamlet_ui::widgets::ellipsis::cut;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::switch::switch;
use hamlet_ui::{AnyView, View, built, with_cx};

use crate::appearance::Appearance;
use crate::dock::{PanelAppExt, panel};
use crate::roll::{either, moved};
use crate::root::Lasting;
use crate::world_menu::{self, Deed};

pub fn plugin(app: &mut App) {
    app.add_panel(
        "settings",
        DockWindowKind::new("Settings", || panel(screen()))
            .icon(Glyph::Sliders.image(ICON)),
    )
    .add_plugins((sight::plugin, workspace::plugin, visits::plugin))
    .init_resource::<Pane>()
    .init_resource::<Charter>()
    .init_resource::<CharterRequests>()
    .add_observer(charter_received)
    .add_observer(charter_saved)
    .add_observer(default_character_saved)
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
    Sound,
    /// How the world is drawn: quality, the hour and the fog.
    Sight,
    /// Light, dark, or as the computer is set.
    Appearance,
}

impl Pane {
    const PAGES: [Pane; 9] = [
        Pane::Endpoints,
        Pane::Models,
        Pane::Agents,
        Pane::Workspace,
        Pane::Charter,
        Pane::Realm,
        Pane::Sound,
        Pane::Sight,
        Pane::Appearance,
    ];

    fn title(self) -> &'static str {
        match self {
            Pane::Front => "Settings",
            Pane::Endpoints => "Endpoints",
            Pane::Models => "Models",
            Pane::Agents => "Agent behaviour",
            Pane::Workspace => "Workspace",
            Pane::Charter => "Charter",
            Pane::Realm => "Realm limits",
            Pane::Sound => "Sound",
            Pane::Sight => "Sight",
            Pane::Appearance => "Appearance",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Pane::Front => Glyph::Sliders,
            Pane::Endpoints => Glyph::Signal,
            Pane::Models => Glyph::Chip,
            Pane::Agents => Glyph::Robot,
            Pane::Workspace => Glyph::Shield,
            Pane::Charter => Glyph::Notes,
            Pane::Realm => Glyph::Globe,
            Pane::Sound => Glyph::Speaker,
            Pane::Sight => Glyph::Sun,
            Pane::Appearance => Glyph::Eye,
        }
    }

    /// What the page holds now, as the index shows it beside its
    /// name. None for a page with no one value to show.
    fn now(self) -> Option<Prop<String>> {
        Some(match self {
            Pane::Endpoints => derived(|world: &World| {
                let endpoints =
                    world.resource::<Settings>().0.endpoints.len();
                let providers = world
                    .get_resource::<Providers>()
                    .map_or(0, |providers| {
                        providers
                            .0
                            .iter()
                            .filter(|provider| provider.configured)
                            .count()
                    });
                match endpoints + providers {
                    0 => "None".to_string(),
                    n => format!("{n} connected"),
                }
            })
            .when(either(moved::<Settings>(), moved::<Providers>()))
            .into(),
            Pane::Models => shown::<Settings>(|s| {
                crate::models::model_name(&s.0.model).to_string()
            }),
            Pane::Agents => {
                shown::<Settings>(|s| match s.0.reflect_every {
                    0 => "Reflection off".to_string(),
                    n => format!("Reflect every {n}"),
                })
            }
            Pane::Workspace => shown::<Settings>(|s| {
                let remote = s.0.workspace.url.is_some();
                workspace::CHOICES[usize::from(remote)].to_string()
            }),
            // Read only while its page is open: shown once it has
            // been.
            Pane::Charter => derived(|world: &World| {
                if !world.resource::<CharterRequests>().loaded {
                    return String::new();
                }
                charter_state(world.resource::<Charter>()).to_string()
            })
            .when(either(
                moved::<Charter>(),
                moved::<CharterRequests>(),
            ))
            .into(),
            Pane::Sound => shown::<SoundSettings>(|s| {
                if s.muted { "Off" } else { "On" }.to_string()
            }),
            Pane::Sight => {
                shown::<Quality>(|quality| quality.name().to_string())
            }
            Pane::Appearance => shown::<Appearance>(|appearance| {
                appearance.pick().mode.name().to_string()
            }),
            Pane::Front | Pane::Realm => return None,
        })
    }
}

/// What `read` makes of `R`, or nothing while there is no `R`.
fn shown<R: Resource>(
    read: impl Fn(&R) -> String + Send + Sync + 'static,
) -> Prop<String> {
    derived(move |world: &World| {
        world.get_resource::<R>().map(&read).unwrap_or_default()
    })
    .when(moved::<R>())
    .into()
}

/// A setting's switch or field, and under it its words.
fn told(control: impl View, words: impl View) -> impl View {
    with_cx(move |cx| {
        column((control, words))
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::FlexStart)
    })
}

/// A setting's control alone, as wide as itself.
fn alone(control: impl View) -> impl View {
    column((control,)).align(AlignItems::FlexStart)
}

/// A warning under the network switch while the confinement does not
/// enforce it. Nothing otherwise.
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
    crate::models::efforts("Model's own");

/// Who can let a scholar into the city, as the pick shows them.
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
                            resource::<Settings, _>(|s| {
                                EFFORTS
                                    .iter()
                                    .position(|(effort, _)| {
                                        *effort == s.0.effort
                                    })
                                    .unwrap_or_default()
                            }),
                            Glyph::Chevron.image(ICON),
                            |world, at| {
                                save(world, |set| {
                                    set.effort = Some(EFFORTS[at].0);
                                });
                            },
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
                            resource::<Settings, _>(|s| {
                                NEWCOMERS
                                    .iter()
                                    .position(|(who, _)| {
                                        *who == s.0.newcomers
                                    })
                                    .unwrap_or_default()
                            }),
                            Glyph::Chevron.image(ICON),
                            |world, at| {
                                save(world, |set| {
                                    set.newcomers =
                                        Some(NEWCOMERS[at].0);
                                });
                            },
                        )
                        .width(px(160.0)),
                    )
                }),
                description(resource::<Settings, _>(|s| {
                    match s.0.newcomers {
                        protocol::Newcomers::Open => {
                            "A scholar an agent makes moves in at \
                             once."
                        }
                        protocol::Newcomers::Chief => {
                            "A scholar an agent makes waits at the \
                             harbor until a chief welcomes it."
                        }
                        protocol::Newcomers::Hand => {
                            "The Hand lets a scholar in or turns it \
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
                        "A scholar made blank starts empty.".into()
                    } else {
                        format!(
                            "A scholar made blank starts from: {}",
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
                    .map(|grant| {
                        let shown = hamlet_client::realm::named(
                            world,
                            &grant.name,
                        )
                        .map_or_else(
                            || grant.name.clone(),
                            |agent| {
                                hamlet_client::realm::shown_of(
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
                "None yet. Choosing Allow always on a petition adds \
                 one.",
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
                                crate::chat::command(
                                    world,
                                    format!(
                                        "/revoke {agent} \
                                         {permission}"
                                    ),
                                );
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

/// Whether the realm keeps a charter of its own, as its pill says.
fn charter_state(charter: &Charter) -> &'static str {
    if charter.own { "Edited" } else { "Default" }
}

/// `text` with the lead of each paragraph that opens with one, as
/// "Tools: ..." or a line "How you improve yourself:" does, made a
/// heading.
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

/// The words every agent's prompt opens with, set as a document, and
/// the ways to change them and to have the built-in ones again.
fn charter_page() -> impl View {
    let actions = keyed(resource::<Charter, _>(|c| c.own), |own| {
        let edit = action(Glyph::Rename, "Edit", edit_charter)
            .tooltip(|| note("Opens the file in your editor"))
            .boxed();
        let mut actions = vec![edit];
        if *own {
            actions.push(action(
                Glyph::Restore,
                "Back to the default",
                |world| world_menu::ask(world, Deed::DefaultCharter),
            ));
        }
        buttons(actions).boxed()
    })
    .within(bare());
    let shown = keyed(
        resource::<Charter, _>(|c| with_heads(&c.text)),
        |text| hamlet_ui::markdown::page(text, Tone::Body).boxed(),
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

fn sound() -> impl View {
    parts((
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
            alone(sound_switch(
                |s| s.wildlife_on,
                |s, on| s.wildlife_on = on,
            )),
        ),
        field(
            "Ambience",
            alone(sound_switch(
                |s| s.ambience_on,
                |s, on| s.ambience_on = on,
            )),
        ),
    ))
}

fn nav_item(pane: Pane) -> AnyView {
    valued_row(pane.glyph(), pane.title(), pane.now(), move |world| {
        *world.resource_mut::<Pane>() = pane;
    })
}

/// The way back to the first-run screen, as a row of the index.
fn again() -> AnyView {
    hamlet_ui::styles::nav_row(
        Glyph::Restore,
        "First-run setup",
        None,
        crate::welcome::again,
    )
}

/// The Settings panel's contents: the page's head at the top, and
/// the rest of it scrolling under the head.
pub fn screen() -> AnyView {
    keyed(resource::<Pane, _>(|pane| *pane), |&pane| {
        let (head, body) = parts_of(pane);
        with_cx(move |cx| {
            let space = &cx.theme().space;
            // The table of models is wider than a page of settings.
            let width = px(if pane == Pane::Models {
                space.table_width
            } else {
                space.page_width
            });
            // It scrolls down only: a page is laid out to the
            // panel's width. Each page keeps where it was scrolled
            // to.
            let scrolled =
                scroll((column((body,)).gap(0.0).max_width(width),))
                    .fills_height()
                    .overflow(Overflow {
                        x: OverflowAxis::Clip,
                        y: OverflowAxis::Scroll,
                    })
                    .tagged(Lasting(format!("settings.{pane:?}")));
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

/// The head of the page of `pane`, and what is under it.
fn parts_of(pane: Pane) -> (AnyView, AnyView) {
    if pane == Pane::Front {
        let mut pages: Vec<AnyView> =
            Pane::PAGES.map(nav_item).into();
        pages.push(again());
        let list = with_cx(move |cx| nav_list(cx, pages)).boxed();
        return (title(Pane::Front.title()).boxed(), list);
    }
    let body = match pane {
        Pane::Endpoints => crate::models::endpoints().boxed(),
        Pane::Models => crate::models::shelf().boxed(),
        Pane::Agents => agents().boxed(),
        Pane::Workspace => workspace::page().boxed(),
        Pane::Charter => charter_page().boxed(),
        Pane::Realm => crate::limits::realm().boxed(),
        Pane::Sight => sight::page().boxed(),
        Pane::Appearance => appearance::page().boxed(),
        Pane::Front | Pane::Sound => sound().boxed(),
    };
    // The path names the page; the charter's state stands beside it.
    let path = breadcrumb(
        vec!["Settings".into()],
        pane.title(),
        |world, _| {
            *world.resource_mut::<Pane>() = Pane::Front;
        },
    );
    let head = match pane {
        Pane::Charter => beside((
            path,
            pill(resource::<Charter, _>(|charter| {
                charter_state(charter).to_string()
            })),
        ))
        .boxed(),
        _ => path.boxed(),
    };
    (head, body)
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
            words_and_spans(&mut app).iter().any(|text| text
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
        assert!(
            shown.iter().any(|text| text == "Edited"),
            "{shown:?}"
        );
        assert!(
            shown.iter().any(|text| text == "Back to the default")
        );
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
        assert!(words_and_spans(&mut app).iter().any(|text| text == "Go back to the default charter?"));
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
        assert!(
            !shown.iter().any(|text| text == "Back to the default")
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
