//! The settings screen: a front page of its parts, each on a page
//! of its own.

mod plugins;

use std::path::Path;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, dropdown, label,
    number_field, row, scroll, slider,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_core::core::{HomeDir, SaveSettings, Settings};
use hamlet_core::oversight::Revoke;
use hamlet_core::oversight::grants::Grants;
use hamlet_desktop::awake::KeepAwake;
use hamlet_home::charter;
use hamlet_model::config::Config;
use hamlet_realm::sky::Quality;
use hamlet_realm::sound::SoundSettings;
use hamlet_sdk::page;
use hamlet_sdk::plugin::Entry;
use hamlet_sdk::settings::{Line, SettingsPage, SettingsPages};

pub use self::plugins::{Open, Tab};
use crate::dock::{Panel, PanelAppExt, panel};
use crate::icons::Glyph;
use crate::looks::{
    Fills, ICON, action, bad_action, bare, buttons, doors, hairline,
    note, page_head, tick, well,
};
use crate::me::Me;
use crate::platform::DisplayText;
use crate::roll::{either, moved};
use crate::world_menu::{self, Deed};
use crate::{AnyView, Cx, View, markdown, with_cx};

pub fn plugin(app: &mut App) {
    app.add_panel(Panel::new(
        "settings",
        "SETTINGS",
        Glyph::Sliders,
        || panel(screen()),
    ))
    .add_plugins(plugins::plugin)
    .init_resource::<Pane>()
    .init_resource::<Charter>()
    .add_systems(
        Update,
        (
            read_charter.run_if(resource_changed::<Pane>),
            leave_page.run_if(resource_changed::<SettingsPages>),
        ),
    );
}

/// Declares the page of the plugins, a door of the Settings panel.
pub(crate) fn contribute(entry: &mut Entry) {
    entry.settings_page_with(
        "PLUGINS",
        Glyph::Power,
        "What is added to Hamlet, each on or off, and what each may \
         do.",
        plugins::page,
    );
}

/// Turns back to the front page when the page a plugin added is gone.
fn leave_page(pages: Res<SettingsPages>, mut pane: ResMut<Pane>) {
    if let Pane::Contributed(item) = *pane
        && !pages.holds(item)
    {
        *pane = Pane::Front;
    }
}

fn field(name: &'static str, value: impl View) -> impl View {
    column((
        label(name)
            .size(12.0)
            .bold(true)
            .tone(Tone::Faint)
            .tagged(DisplayText),
        value,
    ))
    .gap(6.0)
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
            .width(px(140.0)),
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
    /// The words every agent's prompt opens with.
    Charter,
    /// The rules that hold for every hamlet.
    Realm,
    Senses,
    /// A page a plugin added, by the entity of its
    /// [`SettingsPage`], which has its words and its icon.
    Contributed(Entity),
}

impl Pane {
    const PAGES: [Pane; 6] = [
        Pane::Endpoints,
        Pane::Models,
        Pane::Agents,
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
            Pane::Charter => "CHARTER",
            Pane::Realm => "REALM",
            Pane::Senses => "SOUND AND SIGHT",
            Pane::Contributed(_) => "",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Pane::Front | Pane::Contributed(_) => Glyph::Sliders,
            Pane::Endpoints => Glyph::Globe,
            Pane::Models => Glyph::Chip,
            Pane::Agents => Glyph::Robot,
            Pane::Charter => Glyph::Notes,
            Pane::Realm => Glyph::Signpost,
            Pane::Senses => Glyph::Sun,
        }
    }

    fn holds(self) -> &'static str {
        match self {
            Pane::Front | Pane::Contributed(_) => "",
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
            Pane::Charter => "What every agent is told.",
            Pane::Realm => "Rules that apply to every hamlet.",
            Pane::Senses => "Sound, music and how much is drawn.",
        }
    }
}

/// A setting's words, and under them its switch or slider.
fn told(control: impl View, words: impl View) -> impl View {
    column((words, control))
        .gap(6.0)
        .align(AlignItems::FlexStart)
}

fn switch(
    on: impl Into<bevy_fynix::Prop<bool>>,
    set: impl Fn(&mut World, bool) + Send + Sync + 'static,
) -> impl View {
    tick(on).on_change(set)
}

/// The settings of agents. Each applies when changed.
fn agents() -> impl View {
    column((
        field(
            "Reflect every N turns",
            told(
                number_field(
                    resource::<Settings, _>(|s| {
                        s.config.reflect_every
                    }),
                    |world, turns| {
                        save(world, |config| {
                            config.reflect_every = turns
                        })
                    },
                )
                .range(0usize, 1000usize)
                .width(px(90.0))
                .rules(well),
                note("0 means never."),
            ),
        ),
        field(
            "Tools network access",
            told(
                switch(
                    resource::<Settings, _>(|s| s.config.allow_net),
                    |world, on| {
                        save(world, |config| config.allow_net = on)
                    },
                ),
                note(resource::<Settings, _>(|s| {
                    if s.config.allow_net {
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
                note(
                    "The computer stays awake while an agent works.",
                ),
            ),
        ),
        field("Lasting permissions", permissions()),
    ))
    .gap(12.0)
}

/// The permissions agents were given for good from a petition, each
/// to be taken back.
fn permissions() -> AnyView {
    let held = derived(|world: &World| {
        world
            .get_resource::<Grants>()
            .map(|grants| {
                grants
                    .held()
                    .iter()
                    .map(|grant| {
                        (
                            grant.name.clone(),
                            grant.permission,
                            grant.by.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
    .when(crate::roll::moved::<Grants>());
    keyed(held, |held| {
        if held.is_empty() {
            return note(
                "None yet. Choosing Always Allow on a petition adds \
                 one.",
            )
            .boxed();
        }
        let rows = held
            .iter()
            .map(|(agent, permission, by)| {
                let (agent, permission) =
                    (agent.clone(), *permission);
                let says = format!(
                    "{agent} may use: {} (given by {by})",
                    permission.name()
                );
                row((
                    row((label(says).size(12.0),)).fills_width(),
                    bad_action(
                        Glyph::Cross,
                        "REVOKE",
                        move |world| {
                            let chief =
                                world.resource::<Me>().user.clone();
                            world.trigger(Revoke {
                                agent: agent.clone(),
                                permission,
                                chief,
                            });
                            world.flush();
                        },
                    ),
                ))
                .gap(12.0)
                .align(AlignItems::Center)
                .boxed()
            })
            .collect::<Vec<_>>();
        column(rows).gap(6.0).boxed()
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

impl Charter {
    /// The charter of the home at `root`.
    pub fn of(root: &Path) -> Self {
        let own = charter::own(root);
        Charter {
            own: own.is_some(),
            text: own.unwrap_or_else(|| charter::DEFAULT.to_string()),
        }
    }
}

/// Reads the home's charter again while its page is open.
fn read_charter(
    home: Option<Res<HomeDir>>,
    pane: Res<Pane>,
    mut shown: ResMut<Charter>,
) {
    let Some(home) = home.filter(|_| *pane == Pane::Charter) else {
        return;
    };
    let now = Charter::of(&home.0.root);
    if *shown != now {
        *shown = now;
    }
}

/// Opens the home's own charter in the program the system keeps for
/// such a file, made from the built-in one if the home has none.
fn edit_charter(world: &mut World) {
    let Some(home) = world.get_resource::<HomeDir>() else {
        return;
    };
    if let Ok(file) = charter::adopt(&home.0.root) {
        crate::links::open_file(&file);
    }
    let _ = world.run_system_cached(read_charter);
}

/// Removes the home's own charter.
pub(crate) fn default_charter(world: &mut World) {
    let Some(home) = world.get_resource::<HomeDir>() else {
        return;
    };
    if charter::reset(&home.0.root).is_ok() {
        let _ = world.run_system_cached(read_charter);
    }
}

/// The words every agent's prompt opens with, and the ways to change
/// them and to have the built-in ones again.
fn charter_page(cx: &Cx) -> impl View + use<> {
    let actions = keyed(resource::<Charter, _>(|c| c.own), |own| {
        let edit = action(Glyph::Rename, "EDIT", edit_charter)
            .tooltip(|| {
                label("Opens the file in your editor").size(12.0)
            })
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
    column((
        note("Every agent reads this at the start of each turn."),
        actions,
        hairline(cx.theme().palette.brass_6),
        shown,
    ))
    .gap(12.0)
}

/// Saves the settings with one value changed.
fn save(world: &mut World, change: impl FnOnce(&mut Config)) {
    let mut config = world.resource::<Settings>().config.clone();
    change(&mut config);
    world.trigger(SaveSettings(config));
}

/// A switch and a slider: a kind of sound that is on or off, and its
/// level.
fn level(
    on: fn(&SoundSettings) -> bool,
    turn: fn(&mut SoundSettings, bool),
    at: fn(&SoundSettings) -> f32,
    set: fn(&mut SoundSettings, f32),
) -> impl View {
    row((
        switch(resource::<SoundSettings, _>(on), move |world, to| {
            turn(&mut world.resource_mut::<SoundSettings>(), to)
        }),
        slider(resource::<SoundSettings, _>(at)).on_change(
            move |world, to| {
                set(&mut world.resource_mut::<SoundSettings>(), to)
            },
        ),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
}

fn senses() -> impl View {
    column((
        field(
            "Visual quality",
            told(quality(), note("Particles, bloom and shadows.")),
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
    ))
    .gap(12.0)
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
    column((
        note("Choose again where your model comes from."),
        action(Glyph::Restore, "SET UP AGAIN", crate::welcome::again),
    ))
    .gap(6.0)
    .padding(UiRect::top(px(8.0)))
    .align(AlignItems::FlexStart)
}

/// The door to a page a plugin added.
fn contributed_door(line: &Line) -> AnyView {
    let item = line.item;
    crate::looks::list_door(
        line.icon,
        line.title.clone(),
        line.holds.clone(),
        move |world| {
            *world.resource_mut::<Pane>() = Pane::Contributed(item)
        },
    )
}

/// A page a plugin added, under a head with its words.
fn contributed(item: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let (icon, title, panel) = cx
            .world
            .get::<SettingsPage>(item)
            .map(|page| {
                (page.icon, page.title.clone(), page.panel.clone())
            })
            .unwrap_or((
                Glyph::Sliders,
                String::new(),
                String::new(),
            ));
        let body = page::content(cx.world, item, &panel);
        cx.build(
            column((
                page_head(
                    "SETTINGS",
                    |world| {
                        *world.resource_mut::<Pane>() = Pane::Front
                    },
                    icon,
                    title,
                ),
                body,
            ))
            .gap(16.0),
        )
    })
}

/// The Settings panel's contents.
pub fn screen() -> AnyView {
    // The doors are listed again when a page comes or goes.
    let shown = derived(|world: &World| {
        (
            *world.resource::<Pane>(),
            world.resource::<SettingsPages>().clone(),
        )
    })
    .when(either(moved::<Pane>(), moved::<SettingsPages>()));
    let page = keyed(shown, |(pane, added)| {
        if let Pane::Contributed(item) = pane {
            return contributed(*item);
        }
        if *pane == Pane::Front {
            let mut pages =
                Pane::PAGES.map(door).into_iter().collect::<Vec<_>>();
            pages.extend(added.0.iter().map(contributed_door));
            let foot = again().boxed();
            return with_cx(move |cx| doors(cx, pages, foot));
        }
        let body = match pane {
            Pane::Endpoints => crate::models::endpoints().boxed(),
            Pane::Models => crate::models::shelf().boxed(),
            Pane::Agents => agents().boxed(),
            Pane::Charter => with_cx(charter_page),
            Pane::Realm => crate::rules::realm().boxed(),
            Pane::Front | Pane::Senses | Pane::Contributed(_) => {
                senses().boxed()
            }
        };
        column((
            page_head(
                "SETTINGS",
                |world| *world.resource_mut::<Pane>() = Pane::Front,
                pane.glyph(),
                pane.title(),
            ),
            body,
        ))
        .gap(16.0)
        .boxed()
    })
    .within(column(()).gap(0.0).max_width(px(640.0)));
    AnyView::new(move |cx: &mut Cx| {
        cx.build(scroll((page,)).fills_height())
    })
}
