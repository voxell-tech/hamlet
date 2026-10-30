//! The Terminal panel: the screen of the active terminal, and beside
//! it the terminals by hamlet, each agent's runs first, with the
//! press that opens another.

mod input;
pub mod keys;
mod list;
mod screen;

use bevy::app::{App, Update};
use bevy::color::{Alpha, Color};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::Rect;
use bevy::picking::Pickable;
use bevy::ui::{
    AlignItems, Outline, Overflow, PositionType, UiRect, px,
};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, menu_item,
    overlay, popup, row,
};
use bevy_fynix::{
    Theme, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::outbox::RequestId;
use hamlet_client::realm::{self, HamletId};
use hamlet_client::terminals::{self, TerminalOpened, Watched};
use hamlet_ui::styles::{Fills, ICON, caption, description};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{AnyView, View, with_cx};

use self::list::Listed;
pub use self::screen::Canvas;
pub(crate) use self::screen::{Chosen, Room};
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
    .init_resource::<Handing>()
    .init_resource::<Adding>()
    .init_resource::<Opening>()
    .init_resource::<Chosen>()
    .init_resource::<Room>()
    .add_observer(opened)
    .add_systems(
        Update,
        (
            (hand_keys, keep_watching, screen::draw, screen::fit)
                .chain(),
            light_edge,
        ),
    )
    .add_plugins((input::plugin, list::plugin));
}

/// The terminal the panel shows.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub struct Active(pub Option<Entity>);

/// A faint line just inside the panel's edge, lit while its screen
/// has the keys. It is the panel's last child, so drawn over the
/// rest.
#[derive(Component)]
#[require(Pickable = Pickable::IGNORE)]
struct Edge;

/// How strongly the [`Edge`] shows the accent.
const EDGE_ALPHA: f32 = 0.45;

/// Lights each panel's [`Edge`] while a terminal's screen has the
/// keys.
fn light_edge(
    focus: Option<Res<InputFocus>>,
    canvases: Query<(), With<Canvas>>,
    theme: Option<Res<Theme<HamletTheme>>>,
    edges: Query<(Entity, Option<&Outline>), With<Edge>>,
    mut commands: Commands,
) {
    let focused = focus
        .and_then(|focus| focus.get())
        .is_some_and(|node| canvases.contains(node));
    let lit = theme.map_or(Color::NONE, |theme| {
        theme.0.palette.gold_4.with_alpha(EDGE_ALPHA)
    });
    let color = if focused { lit } else { Color::NONE };
    for (edge, outline) in &edges {
        if outline.is_none_or(|outline| outline.color != color) {
            commands.entity(edge).try_insert(Outline::new(
                px(1.0),
                px(-1.0),
                color,
            ));
        }
    }
}

/// Whether the screen is to be given the keys once it is there.
#[derive(Resource, Default)]
struct Handing(bool);

/// Where the menu that opens a terminal hangs, while it is open.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
struct Adding(Option<Rect>);

/// The open the panel waits on, to show what it opens.
#[derive(Resource, Default)]
struct Opening(Option<RequestId>);

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
fn opened(opened: On<TerminalOpened>, mut commands: Commands) {
    let (request, terminal) =
        (opened.request.clone(), opened.terminal);
    commands.queue(move |world: &mut World| {
        if world.resource::<Opening>().0.as_ref() != Some(&request) {
            return;
        }
        world.resource_mut::<Opening>().0 = None;
        show_tab(world, "terminal");
        activate(world, terminal);
    });
}

/// Gives the screen the keys once it is there: the first terminal's
/// canvas is built only after the terminal arrives.
fn hand_keys(world: &mut World) {
    if !world.resource::<Handing>().0 {
        return;
    }
    let canvas = world
        .try_query_filtered::<Entity, With<Canvas>>()
        .and_then(|mut query| query.iter(world).next());
    if let Some(canvas) = canvas {
        world.resource_mut::<Handing>().0 = false;
        if let Some(mut focus) =
            world.get_resource_mut::<InputFocus>()
        {
            focus.set(canvas, FocusCause::Pressed);
        }
    }
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

/// Shows `terminal` and gives it the keys, as a press on its screen
/// would.
fn activate(world: &mut World, terminal: Entity) {
    world.resource_mut::<Active>().0 = Some(terminal);
    world.resource_mut::<Handing>().0 = true;
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
            list::side(),
            frame()
                .position(PositionType::Absolute)
                .inset(UiRect::all(px(0.0)))
                .tagged(Edge),
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
    fn a_terminal_shown_from_the_list_or_opened_gets_the_keys_once_its_screen_is_there()
     {
        let mut app = App::new();
        app.add_plugins(bevy::input_focus::InputFocusPlugin)
            .init_resource::<Active>()
            .init_resource::<Handing>()
            .add_systems(Update, hand_keys);
        let terminal = app.world_mut().spawn_empty().id();
        activate(app.world_mut(), terminal);
        app.update();
        assert!(app.world().resource::<Handing>().0);
        let canvas = app.world_mut().spawn(Canvas).id();
        app.update();
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(canvas)
        );
        assert!(!app.world().resource::<Handing>().0);
    }

    #[test]
    fn a_press_on_the_screen_keeps_the_keys_there_and_lights_the_edge()
     {
        use bevy::ecs::hierarchy::ChildOf;
        use bevy::input_focus::pointer_focus::PointerFocusPlugin;
        use bevy::input_focus::{AcquireFocus, InputFocusPlugin};
        use bevy::window::{PrimaryWindow, Window};
        use hamlet_assets::Tuning;

        let mut app = App::new();
        app.add_plugins((
            bevy::input::InputPlugin,
            InputFocusPlugin,
            PointerFocusPlugin,
        ))
        .add_systems(Update, light_edge);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.update();
        let panel = app.world_mut().spawn(ChildOf(window)).id();
        let canvas =
            app.world_mut().spawn((Canvas, ChildOf(panel))).id();
        let edge = app.world_mut().spawn((Edge, ChildOf(panel))).id();
        app.world_mut().trigger(AcquireFocus {
            focused_entity: canvas,
            window,
        });
        app.update();
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(canvas)
        );
        app.world_mut()
            .insert_resource(Theme(HamletTheme::designed()));
        app.update();
        let lit = |app: &App| {
            app.world()
                .get::<Outline>(edge)
                .is_some_and(|outline| outline.color != Color::NONE)
        };
        assert!(lit(&app));
        app.world_mut().trigger(AcquireFocus {
            focused_entity: panel,
            window,
        });
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), None);
        assert!(!lit(&app));
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
        list::list(app.world_mut());
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
