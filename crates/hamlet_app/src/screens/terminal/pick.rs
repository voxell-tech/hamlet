//! The menu the list's plus opens: a new terminal of the chief's own,
//! and the agents by hamlet, whose bash logs are pinned to the list
//! or let go. A search narrows the agents by name.
use bevy::app::{App, PostUpdate};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::Rect;
use bevy::picking::Pickable;
use bevy::text::{EditableText, TextEdit};
use bevy::ui::{
    AlignItems, Display, JustifyContent, PositionType, UiRect, Val,
    percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, menu_item, overlay, popup, row, text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_protocol::client::AgentKind;
use hamlet_ui::styles::{ICON, link_row, note, text_input};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use super::list::{Branch, Group, Listed, Row, What};
use super::{PanelId, Pins, open};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Adding>()
        .init_resource::<Typed>()
        .add_systems(PostUpdate, (read_search, focus_search));
}

/// Where the menu hangs, and the panel it opens a terminal in, while
/// it is open.
#[derive(Resource, Default, Clone, PartialEq)]
pub(super) struct Adding(Option<(Rect, PanelId)>);

/// What the search holds.
#[derive(Resource, Default, Clone, PartialEq)]
struct Typed(String);

/// The root of the search field.
#[derive(Component)]
struct Find;

/// Opens the menu of `panel` below `anchor`, as a press there would.
pub fn show_menu(world: &mut World, panel: &PanelId, anchor: Rect) {
    world.resource_mut::<Typed>().0.clear();
    world.resource_mut::<Adding>().0 = Some((anchor, panel.clone()));
}

/// Opens the menu from the press `node` of `panel`, or closes it.
pub(super) fn toggle(
    world: &mut World,
    node: bevy::ecs::entity::Entity,
    panel: &PanelId,
) {
    let shut = world.resource::<Adding>().0.is_some();
    world.resource_mut::<Typed>().0.clear();
    world.resource_mut::<Adding>().0 = if shut {
        None
    } else {
        crate::menu::rect_of(world, node)
            .map(|anchor| (anchor, panel.clone()))
    };
}

fn read_search(
    root: Query<&Children, With<Find>>,
    texts: Query<&EditableText>,
    mut typed: ResMut<Typed>,
) {
    let Some(text) = root
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    if text.value() != typed.0.as_str() {
        typed.0 = text.value().to_string();
    }
}

/// Puts the keyboard in the search as the menu opens.
fn focus_search(
    fields: Query<&Children, Added<Find>>,
    mut texts: Query<&mut EditableText>,
    mut focus: ResMut<InputFocus>,
) {
    for children in &fields {
        if let Some(&text) =
            children.iter().find(|&&kid| texts.contains(kid))
        {
            focus.set(text, FocusCause::Navigated);
            if let Ok(mut field) = texts.get_mut(text) {
                field.queue_edit(TextEdit::SelectAll);
            }
        }
    }
}

/// The menu of what to open or pin, while it is open.
pub fn open_menu() -> impl View {
    keyed(
        derived(|world: &World| world.resource::<Adding>().0.clone())
            .when(every_frame()),
        |adding| match adding {
            Some((anchor, panel)) => {
                let anchor = *anchor;
                let panel = panel.clone();
                with_cx(move |cx| {
                    popup(anchor, content(&panel))
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

fn content(panel: &PanelId) -> Vec<AnyView> {
    let panel = panel.clone();
    let new = with_cx(move |cx| {
        menu_item(
            row((
                glyph(Glyph::Plus, ICON).tone(Tone::Faint),
                label("New terminal").wrap(false),
            ))
            .gap(cx.theme().space.menu_gap)
            .align(AlignItems::Center),
        )
        .on_activate({
            let panel = panel.clone();
            move |world| {
                world.resource_mut::<Adding>().0 = None;
                open(world, &panel);
            }
        })
    })
    .boxed();
    let tree = keyed(
        derived(|world: &World| {
            (
                world.resource::<Listed>().clone(),
                world.resource::<Pins>().clone(),
                world.resource::<Typed>().0.clone(),
            )
        })
        .when(every_frame()),
        |(listed, pins, typed)| {
            let mut rows = Vec::new();
            for group in &listed.0 {
                rows.extend(group_rows(group, pins, typed));
            }
            column(rows).gap(0.0).boxed()
        },
    )
    .within(column(()).gap(0.0))
    .boxed();
    vec![
        new,
        with_cx(|cx| {
            frame()
                .height(px(1.0))
                .fill(cx.theme().palette.border)
                .margin(UiRect::axes(px(0.0), px(4.0)))
        })
        .boxed(),
        search().boxed(),
        tree,
    ]
}

/// The search, with a word in it while it is empty.
fn search() -> impl View {
    column((
        text_field(String::new(), |_, _| {})
            .width(percent(100.0))
            .rules(text_input)
            .tagged(Find),
        // The word that stands in an empty field, which the pointer
        // passes through to the field.
        column((label("Search agents")
            .tone(Tone::Faint)
            .wrap(false),))
        .position(PositionType::Absolute)
        .inset(UiRect::new(px(8.0), Val::Auto, px(0.0), px(0.0)))
        .justify(JustifyContent::Center)
        .display(resource::<Typed, _>(|typed| {
            if typed.0.is_empty() {
                Display::Flex
            } else {
                Display::None
            }
        }))
        .tagged(Pickable::IGNORE),
    ))
    .width(percent(100.0))
    .margin(UiRect::bottom(px(4.0)))
}

/// The rows of `group` the search leaves, under the group's title.
fn group_rows(
    group: &Group,
    pins: &Pins,
    typed: &str,
) -> Vec<AnyView> {
    let agents = visible(group, typed);
    if agents.is_empty() {
        return Vec::new();
    }
    let mut views = vec![hamlet_row(group.title.clone())];
    for (at, (row, dim)) in agents.iter().enumerate() {
        let What::Agent(branch) = &row.what else {
            unreachable!("only agents are visible");
        };
        let later = |path: &[String]| {
            agents[at + 1..].iter().any(|(next, _)| next.path == path)
        };
        let depth = row.path.len();
        // Each level under the top keeps its guide while a sibling of
        // the agent there is still to come.
        let rails = (1..depth)
            .map(|level| later(&row.path[..level]))
            .collect();
        views.push(agent_row(
            branch.clone(),
            Tree {
                depth,
                rails,
                last: !later(&row.path),
            },
            *dim,
            pins.0.contains(&branch.key),
        ));
    }
    views
}

/// The agents of `group` that `typed` leaves, each with whether it
/// stands only as the way to one that matches. A group whose title
/// matches keeps them all; an agent that matches keeps those above
/// it.
fn visible<'a>(
    group: &'a Group,
    typed: &str,
) -> Vec<(&'a Row, bool)> {
    let word = typed.trim().to_lowercase();
    let agents = group
        .rows
        .iter()
        .filter(|row| matches!(&row.what, What::Agent(Branch { .. })))
        .collect::<Vec<_>>();
    if word.is_empty() || group.title.to_lowercase().contains(&word) {
        return agents.into_iter().map(|row| (row, false)).collect();
    }
    let hits = agents
        .iter()
        .filter(|row| matches!(&row.what, What::Agent(b) if called(b).to_lowercase().contains(&word)))
        .collect::<Vec<_>>();
    agents
        .iter()
        .filter_map(|row| {
            let What::Agent(branch) = &row.what else {
                return None;
            };
            let hit =
                hits.iter().any(|found| std::ptr::eq(**found, *row));
            let above = hits
                .iter()
                .any(|found| found.path.contains(&branch.key));
            (hit || above).then_some((*row, !hit))
        })
        .collect()
}

/// What an agent is called: its name, or its address.
pub(super) fn called(branch: &Branch) -> String {
    branch
        .name
        .clone()
        .unwrap_or_else(|| branch.address.clone())
}

fn hamlet_row(title: String) -> AnyView {
    with_cx(move |cx| {
        row((
            glyph(Glyph::Hamlet, ICON).tone(Tone::Dim),
            label(title.clone()).wrap(false),
        ))
        .gap(cx.theme().space.menu_gap)
        .align(AlignItems::Center)
        .padding(UiRect::axes(px(8.0), px(5.0)))
    })
    .boxed()
}

/// Where an agent stands in its hamlet's tree.
struct Tree {
    depth: usize,
    rails: Vec<bool>,
    last: bool,
}

/// How tall a row of the menu is, for its tree lines.
const ROW: f32 = 28.0;

/// One agent in its tree, with the pin of its log when it has one;
/// `dim` when it only leads to a match.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "dimmed by the search, and pinned: two plain facts of \
              a row"
)]
fn agent_row(
    branch: Branch,
    tree: Tree,
    dim: bool,
    pinned: bool,
) -> AnyView {
    with_cx(move |cx| {
        let who = match branch.kind {
            AgentKind::Hand => Glyph::Shield,
            AgentKind::Hire if branch.mayor => Glyph::Mayor,
            AgentKind::Hire => Glyph::Robot,
            AgentKind::Worker => Glyph::Chip,
        };
        let tone = if dim || branch.log.is_none() {
            Tone::Faint
        } else {
            Tone::Body
        };
        let mut parts = Vec::<AnyView>::new();
        if tree.depth > 0 {
            parts.push(
                crate::screens::hub::guides(
                    tree.rails, tree.last, ROW,
                )
                .boxed(),
            );
        }
        parts.extend([
            glyph(who, ICON).tone(Tone::Dim).boxed(),
            label(called(&branch)).tone(tone).wrap(false).boxed(),
            row(()).grow(1.0).boxed(),
        ]);
        if branch.log.is_some() {
            let key = branch.key.clone();
            parts.push(
                button(glyph(
                    if pinned { Glyph::Pinned } else { Glyph::Pin },
                    ICON,
                ))
                .padding(UiRect::all(px(3.0)))
                .rules(link_row)
                .tooltip(move || {
                    note(if pinned {
                        "Unpin the bash"
                    } else {
                        "Pin the bash"
                    })
                })
                .on_activate(move |world| pin(world, &key))
                .boxed(),
            );
        }
        row(parts)
            .gap(cx.theme().space.menu_gap)
            .align(AlignItems::Center)
            .width(percent(100.0))
            .height(px(ROW))
            .padding(UiRect::new(px(26.0), px(4.0), px(0.0), px(0.0)))
    })
    .boxed()
}

/// Pins the bash of the agent of `key`, or lets it go.
pub fn pin(world: &mut World, key: &str) {
    let mut pins = world.resource_mut::<Pins>();
    if !pins.0.remove(key) {
        pins.0.insert(key.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, name: &str, path: &[&str]) -> Row {
        Row {
            path: path.iter().map(|p| format!("agent#{p}")).collect(),
            what: What::Agent(Branch {
                key: format!("agent#{key}"),
                address: format!("a{key}"),
                name: Some(name.into()),
                kind: AgentKind::Worker,
                mayor: false,
                log: None,
                held: 0,
            }),
        }
    }

    fn names(found: &[(&Row, bool)]) -> Vec<(String, bool)> {
        found
            .iter()
            .map(|(row, dim)| {
                let What::Agent(branch) = &row.what else {
                    unreachable!()
                };
                (called(branch), *dim)
            })
            .collect()
    }

    fn docs() -> Group {
        Group {
            key: "docs".into(),
            title: "docs".into(),
            rows: vec![
                row("1", "Ada", &[]),
                row("2", "Bo", &["1"]),
                row("3", "Scout 6", &["1", "2"]),
                row("4", "Dee", &["1"]),
            ],
        }
    }

    #[test]
    fn a_match_keeps_the_agents_above_it_dimmed() {
        let group = docs();
        assert_eq!(
            names(&visible(&group, "SCOUT")),
            [
                ("Ada".to_owned(), true),
                ("Bo".to_owned(), true),
                ("Scout 6".to_owned(), false),
            ]
        );
    }

    #[test]
    fn a_hamlet_that_matches_keeps_every_agent() {
        let group = docs();
        assert_eq!(visible(&group, "doc").len(), 4);
        assert_eq!(visible(&group, "").len(), 4);
        assert!(visible(&group, "zzz").is_empty());
    }
}
