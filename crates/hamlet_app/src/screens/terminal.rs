//! The Terminal panel: the screen of the active terminal, and beside
//! it the terminals by hamlet, each agent's runs first, with the
//! press that opens another.

mod input;
pub mod keys;
mod screen;

use bevy::app::{App, PreUpdate, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::Rect;
use bevy::ui::{AlignItems, JustifyContent, Overflow, UiRect, px};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, menu_item,
    overlay, popup, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::outbox::RequestId;
use hamlet_client::realm::{self, HamletId};
use hamlet_client::terminals::{
    self, Terminal, TerminalKind, TerminalOpened, Watched,
};
use hamlet_ui::styles::{
    Fills, ICON, caption, close_button, description, hairline,
    link_row, scroll_area,
};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{AnyView, View, with_cx};

pub use self::screen::Canvas;
pub(crate) use self::screen::{Chosen, Room};
use crate::PanelSystems;
use crate::dock::{PanelAppExt, flush_panel, show_tab};

/// The size a terminal opens at before its canvas has been measured.
const FIRST_SIZE: (u16, u16) = (80, 24);

pub fn plugin(app: &mut App) {
    app.add_panel(
        "terminal",
        DockWindowKind::new("TERMINAL", || flush_panel(panel()))
            .icon(Glyph::Terminal.image(ICON)),
    )
    .init_resource::<Active>()
    .init_resource::<Listed>()
    .init_resource::<Adding>()
    .init_resource::<Opening>()
    .init_resource::<Chosen>()
    .init_resource::<Room>()
    .add_observer(opened)
    .add_systems(PreUpdate, list.in_set(PanelSystems::Bind))
    .add_systems(
        Update,
        (keep_watching, screen::draw, screen::fit).chain(),
    )
    .add_plugins(input::plugin);
}

/// The terminal the panel shows.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub struct Active(pub Option<Entity>);

/// Where the menu that opens a terminal hangs, while it is open.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
struct Adding(Option<Rect>);

/// The open the panel waits on, to show what it opens.
#[derive(Resource, Default)]
struct Opening(Option<RequestId>);

/// The terminals as the list shows them.
#[derive(Resource, Default, Clone, PartialEq)]
struct Listed(Vec<Group>);

/// The terminals of one hamlet, or the unbounded ones.
#[derive(Clone, PartialEq)]
struct Group {
    title: String,
    items: Vec<Item>,
}

#[derive(Clone, PartialEq)]
struct Item {
    terminal: Entity,
    name: String,
    /// The agent's name, or "chief".
    owner: String,
    sandboxed: bool,
    exit: Option<i64>,
    runs: bool,
    active: bool,
}

impl Listed {
    fn find(&self, terminal: Entity) -> Option<&Item> {
        self.0
            .iter()
            .flat_map(|group| &group.items)
            .find(|item| item.terminal == terminal)
    }

    fn first(&self) -> Option<Entity> {
        self.0
            .iter()
            .flat_map(|group| &group.items)
            .map(|item| item.terminal)
            .next()
    }
}

/// Lists the terminals by hamlet, the unbounded ones last. In a
/// hamlet the agents' come first, by agent and each agent's runs
/// before its shells, then the chief's.
fn list(world: &mut World) {
    let active = world.resource::<Active>().0;
    let Some(views) =
        world.try_query::<(Entity, &Terminal)>().map(|mut query| {
            query
                .iter(world)
                .map(|(entity, view)| (entity, view.0.clone()))
                .collect::<Vec<_>>()
        })
    else {
        return;
    };
    let mut keyed = views
        .into_iter()
        .map(|(entity, view)| {
            let hamlet = view.hamlet.clone();
            let title = hamlet.as_deref().map_or_else(
                || "Unbounded".to_owned(),
                |id| {
                    realm::hamlet(world, id)
                        .map(|h| realm::name_of(world, h))
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| id.to_owned())
                },
            );
            let owner = view.agent.as_ref().map(|agent| {
                realm::agent(world, agent)
                    .map(|a| realm::name_of(world, a))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| {
                        format!("agent {}", agent.number)
                    })
            });
            let runs = view.kind == TerminalKind::Runs;
            let name = if view.title.is_empty() {
                view.name.clone()
            } else {
                view.title.clone()
            };
            let order = (
                hamlet.is_none(),
                title.clone(),
                owner.is_none(),
                owner.clone(),
                !runs,
                view.name.clone(),
                view.id.clone(),
            );
            let item = Item {
                terminal: entity,
                name,
                owner: owner.unwrap_or_else(|| "chief".into()),
                sandboxed: view.sandboxed,
                exit: view.exit,
                runs,
                active: active == Some(entity),
            };
            (order, title, item)
        })
        .collect::<Vec<_>>();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    let mut groups = Vec::<Group>::new();
    for (_, title, item) in keyed {
        match groups.last_mut() {
            Some(group) if group.title == title => {
                group.items.push(item);
            }
            _ => groups.push(Group {
                title,
                items: vec![item],
            }),
        }
    }
    let listed = Listed(groups);
    if *world.resource::<Listed>() != listed {
        world.insert_resource(listed);
    }
}

/// Watches the active terminal while the panel is shown, and no
/// other. With none active, the first listed is.
fn keep_watching(world: &mut World) {
    let shown =
        world.try_query_filtered::<(), With<Canvas>>().is_some_and(
            |mut canvases| canvases.iter(world).next().is_some(),
        );
    let mut active =
        world.resource::<Active>().0.filter(|terminal| {
            world.get::<terminals::TerminalId>(*terminal).is_some()
        });
    if active.is_none() && shown {
        active = world.resource::<Listed>().first();
    }
    if world.resource::<Active>().0 != active {
        world.resource_mut::<Active>().0 = active;
    }
    let watched = world
        .try_query_filtered::<Entity, With<Watched>>()
        .map(|mut query| query.iter(world).collect::<Vec<_>>())
        .unwrap_or_default();
    let keep = active.filter(|_| shown);
    for terminal in watched {
        if Some(terminal) != keep {
            terminals::unwatch(world, terminal);
        }
    }
    if let Some(terminal) = keep {
        terminals::watch(world, terminal);
    }
}

/// Shows a terminal the panel opened, and gives it the keys.
fn opened(
    opened: On<TerminalOpened>,
    mut commands: bevy::ecs::system::Commands,
) {
    let (request, terminal) =
        (opened.request.clone(), opened.terminal);
    commands.queue(move |world: &mut World| {
        if world.resource::<Opening>().0.as_ref() != Some(&request) {
            return;
        }
        world.resource_mut::<Opening>().0 = None;
        world.resource_mut::<Active>().0 = Some(terminal);
        show_tab(world, "terminal");
        let canvas = world
            .try_query_filtered::<Entity, With<Canvas>>()
            .and_then(|mut query| query.iter(world).next());
        if let Some(canvas) = canvas {
            world
                .resource_mut::<InputFocus>()
                .set(canvas, FocusCause::Pressed);
        }
    });
}

/// Opens a shell in `hamlet`, or an unbounded one, at the size the
/// canvas has room for.
fn open(world: &mut World, hamlet: Option<String>) {
    world.resource_mut::<Adding>().0 = None;
    let (cols, rows) =
        world.resource::<Room>().0.unwrap_or(FIRST_SIZE);
    let request = terminals::open(world, hamlet, cols, rows);
    world.resource_mut::<Opening>().0 = Some(request);
}

fn activate(world: &mut World, terminal: Entity) {
    world.resource_mut::<Active>().0 = Some(terminal);
}

/// The panel's contents.
fn panel() -> impl View {
    with_cx(|cx| {
        let look = &cx.theme().terminal;
        row((
            column((heading(), body()))
                .grow(1.0)
                .min_width(px(0.0))
                .fills_height()
                .gap(0.0)
                .fill(look.background),
            frame().width(px(1.0)).fill(cx.theme().palette.border),
            side(),
        ))
        .fills_width()
        .fills_height()
        .gap(0.0)
    })
}

/// The active terminal's name over its screen, and whether it takes
/// keys.
fn heading() -> impl View {
    keyed(
        derived(|world: &World| {
            let active = world.resource::<Active>().0?;
            world.resource::<Listed>().find(active).cloned()
        })
        .when(every_frame()),
        |item| match item {
            Some(item) => with_cx({
                let item = item.clone();
                move |cx| {
                    let space = &cx.theme().space;
                    let mut hints = Vec::<AnyView>::new();
                    if item.runs {
                        hints.push(
                            badge("READ-ONLY", Tone::Accent).boxed(),
                        );
                    }
                    if let Some(code) = item.exit {
                        hints.push(
                            badge(format!("EXIT {code}"), Tone::Dim)
                                .boxed(),
                        );
                    }
                    if !item.sandboxed {
                        hints.push(
                            badge("UNSANDBOXED", Tone::Critical)
                                .boxed(),
                        );
                    }
                    row((
                        glyph(Glyph::Terminal, ICON).tone(Tone::Dim),
                        label(item.name.clone())
                            .size(cx.theme().text.small)
                            .bold(true)
                            .wrap(false),
                        row((label(item.owner.clone())
                            .size(cx.theme().text.small)
                            .tone(Tone::Faint)
                            .wrap(false),))
                        .grow(1.0)
                        .min_width(px(0.0)),
                        row(hints).gap(space.gap),
                    ))
                    .gap(space.gap)
                    .align(AlignItems::Center)
                    .padding(UiRect::axes(px(space.section), px(6.0)))
                }
            })
            .boxed(),
            None => frame().boxed(),
        },
    )
    .within(column(()).gap(0.0))
}

/// Small capitals in `tone`.
fn badge(text: impl Into<String>, tone: Tone) -> impl View {
    caption(text.into(), tone)
}

/// The screen, or the words of an empty panel.
fn body() -> impl View {
    keyed(
        resource::<Listed, _>(|listed| listed.0.is_empty()),
        |empty| {
            if *empty {
                with_cx(|cx| {
                    let section = cx.theme().space.section;
                    column((description(
                        "No terminals yet. Press + to open a shell \
                         in a hamlet, or an unbounded one. An \
                         agent's commands show here as they run.",
                    ),))
                    .padding(UiRect::all(px(section * 2.0)))
                    .fills_width()
                })
                .boxed()
            } else {
                frame()
                    .grow(1.0)
                    .fills_width()
                    .min_height(px(0.0))
                    .overflow(Overflow::clip())
                    .tagged(Canvas)
                    .boxed()
            }
        },
    )
    .within(
        column(())
            .gap(0.0)
            .grow(1.0)
            .fills_width()
            .min_height(px(0.0)),
    )
}

/// The list of terminals and the press that opens another.
fn side() -> impl View {
    with_cx(|cx| {
        let space = &cx.theme().space;
        column((
            row((
                row((caption("TERMINALS".to_string(), Tone::Faint),))
                    .grow(1.0),
                button(glyph(Glyph::Plus, ICON).tone(Tone::Dim))
                    .padding(UiRect::all(px(4.0)))
                    .rules(link_row)
                    .on_activate_with(|world, node| {
                        let anchor =
                            crate::menu::rect_of(world, node);
                        world.resource_mut::<Adding>().0 = anchor;
                    }),
            ))
            .align(AlignItems::Center)
            .justify(JustifyContent::SpaceBetween)
            .padding(UiRect::new(
                px(space.section),
                px(space.gap),
                px(6.0),
                px(6.0),
            )),
            hairline(cx.theme().palette.border),
            scroll_area((keyed(
                resource::<Listed, _>(Clone::clone),
                |listed| groups(listed).boxed(),
            )
            .within(column(()).gap(0.0)),)),
        ))
        .gap(0.0)
        .width(px(cx.theme().terminal.list))
        .fill(cx.theme().palette.card)
    })
}

fn groups(listed: &Listed) -> impl View {
    let groups = listed
        .0
        .iter()
        .map(|group| {
            let rows = group
                .items
                .iter()
                .map(|item| entry(item.clone()))
                .collect::<Vec<_>>();
            let title = group.title.to_uppercase();
            with_cx(move |cx| {
                let space = &cx.theme().space;
                column((
                    row((caption(title, Tone::Dim),)).padding(
                        UiRect::new(
                            px(space.section),
                            px(space.section),
                            px(space.gap),
                            px(2.0),
                        ),
                    ),
                    column(rows).gap(0.0),
                ))
                .gap(0.0)
            })
            .boxed()
        })
        .collect::<Vec<_>>();
    column(groups).gap(0.0).padding(UiRect::bottom(px(8.0)))
}

/// One terminal: its name and owner, what it is, and its close.
fn entry(item: Item) -> AnyView {
    with_cx(move |cx| {
        let theme = cx.theme();
        let small = theme.text.small;
        let terminal = item.terminal;
        let mut about = vec![item.owner.clone()];
        if item.runs {
            about.push("runs".into());
        }
        if !item.sandboxed {
            about.push("unsandboxed".into());
        }
        if let Some(code) = item.exit {
            about.push(format!("exit {code}"));
        }
        let mark = if item.runs {
            Glyph::Play
        } else {
            Glyph::Terminal
        };
        let tone = if item.active { Tone::Accent } else { Tone::Dim };
        let fill = if item.active {
            theme.palette.button
        } else {
            bevy::color::Color::NONE
        };
        row((
            button(
                row((
                    glyph(mark, ICON).tone(tone),
                    column((
                        label(item.name.clone())
                            .size(small)
                            .bold(item.active)
                            .tone(if item.active {
                                Tone::Body
                            } else {
                                Tone::Dim
                            })
                            .wrap(false),
                        label(about.join(" \u{b7} "))
                            .size(theme.text.caption)
                            .tone(
                                if item
                                    .exit
                                    .is_some_and(|code| code != 0)
                                {
                                    Tone::Critical
                                } else {
                                    Tone::Faint
                                },
                            )
                            .wrap(false),
                    ))
                    .gap(1.0)
                    .min_width(px(0.0)),
                ))
                .gap(theme.space.gap)
                .align(AlignItems::Center),
            )
            .grow(1.0)
            .min_width(px(0.0))
            .justify(JustifyContent::FlexStart)
            .padding(UiRect::new(
                px(theme.space.section),
                px(4.0),
                px(5.0),
                px(5.0),
            ))
            .rules(link_row)
            .on_activate(move |world| activate(world, terminal)),
            close_button(move |world| {
                terminals::close(world, terminal);
            }),
        ))
        .align(AlignItems::Center)
        .padding(UiRect::right(px(theme.space.gap)))
        .fill(fill)
    })
    .boxed()
}

/// The hamlets a terminal can be opened in, by id and name.
fn hamlets(world: &World) -> Vec<(String, String)> {
    let mut found = world
        .try_query::<(Entity, &HamletId)>()
        .map(|mut query| {
            query
                .iter(world)
                .map(|(entity, id)| (entity, id.0.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|(entity, id)| {
            let name = realm::name_of(world, entity);
            (id.clone(), if name.is_empty() { id } else { name })
        })
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    found
}

/// The menu of where to open a terminal, while it is open.
pub fn open_menu() -> impl View {
    keyed(
        derived(|world: &World| {
            (world.resource::<Adding>().0, hamlets(world))
        })
        .when(every_frame()),
        |(anchor, hamlets)| match anchor {
            Some(anchor) => {
                let anchor = *anchor;
                let mut items = vec![place(
                    Glyph::Terminal,
                    "Unbounded".into(),
                    None,
                )];
                items.extend(hamlets.iter().map(|(id, name)| {
                    place(
                        Glyph::Hamlet,
                        name.clone(),
                        Some(id.clone()),
                    )
                }));
                with_cx(move |cx| {
                    popup(anchor, items)
                        .on_dismiss(|world, _| {
                            world.resource_mut::<Adding>().0 = None;
                        })
                        .min_width(px(cx.theme().space.menu_width))
                })
                .boxed()
            }
            None => frame().boxed(),
        },
    )
    .within(overlay(()))
}

fn place(
    which: Glyph,
    name: String,
    hamlet: Option<String>,
) -> AnyView {
    with_cx(move |cx| {
        menu_item(
            row((
                glyph(which, ICON).tone(Tone::Faint),
                label(name).wrap(false).tagged(DisplayText),
            ))
            .gap(cx.theme().space.menu_gap)
            .align(AlignItems::Center),
        )
        .on_activate(move |world| open(world, hamlet.clone()))
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client as protocol;
    use serde_json::json;

    use super::*;

    fn view(
        id: &str,
        hamlet: Option<&str>,
        agent: Option<u64>,
        kind: &str,
    ) -> terminals::TerminalView {
        serde_json::from_value(json!({
            "agent": agent.map(|n| json!({"hamlet": hamlet, "number": n})),
            "cols": 80, "rows": 24, "cwd": "/", "exit": null,
            "hamlet": hamlet, "id": id, "kind": kind, "name": id,
            "sandboxed": hamlet.is_some(), "title": ""
        }))
        .unwrap()
    }

    #[test]
    fn terminals_are_listed_by_hamlet_with_each_agents_runs_first_and_the_unbounded_last()
     {
        let mut app = crate::testing::panel_app();
        let mut state = hamlet_testing::realm::state();
        state.revision = 1;
        state.hamlets.push(hamlet_testing::realm::hamlet("mill"));
        state.terminals = vec![
            view("loose", None, None, "shell"),
            view("chief", Some("home"), None, "shell"),
            view("shell", Some("home"), Some(0), "shell"),
            view("runs", Some("home"), Some(0), "runs"),
            view("milled", Some("mill"), None, "shell"),
        ];
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.init_resource::<Active>().init_resource::<Listed>();
        list(app.world_mut());
        let listed = app
            .world()
            .resource::<Listed>()
            .0
            .iter()
            .map(|group| {
                let names = group
                    .items
                    .iter()
                    .map(|item| {
                        format!("{}:{}", item.owner, item.name)
                    })
                    .collect::<Vec<_>>();
                (group.title.clone(), names)
            })
            .collect::<Vec<_>>();
        let group = |title: &str, names: &[&str]| {
            (
                title.to_owned(),
                names.iter().map(|n| (*n).to_owned()).collect(),
            )
        };
        assert_eq!(
            listed,
            [
                group(
                    "home",
                    &["founder:runs", "founder:shell", "chief:chief"]
                ),
                group("mill", &["chief:milled"]),
                group("Unbounded", &["chief:loose"]),
            ]
        );
    }
}
