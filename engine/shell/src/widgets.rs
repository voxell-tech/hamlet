//! A widget tree, as a plugin describes a panel, drawn with the
//! shell's styles.

use bevy::prelude::*;
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use hamlet_engine_api::{
    Action, Widget, WidgetId, WidgetNode, WidgetTree,
};

use crate::icons::{Glyph, mark};
use crate::style::{
    ButtonStyle, action, divider, field, heading, list, scroll_area,
    toggle,
};
use crate::{AnyView, Cx};

/// `tree` drawn, calling `act` with what the person does to one of
/// its widgets. A tree that does not hold together draws what it can.
pub fn tree_view<F>(tree: &WidgetTree, act: F) -> AnyView
where
    F: Fn(&mut World, WidgetId, Action)
        + Clone
        + Send
        + Sync
        + 'static,
{
    let nodes = tree.nodes.clone();
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        let mut at = 0;
        cx.build(node_view(&nodes, &mut at, gap, &act))
    })
}

/// The node at `at` and those under it, with `at` moved past them.
fn node_view<F>(
    nodes: &[WidgetNode],
    at: &mut usize,
    gap: f32,
    act: &F,
) -> AnyView
where
    F: Fn(&mut World, WidgetId, Action)
        + Clone
        + Send
        + Sync
        + 'static,
{
    let Some(node) = nodes.get(*at) else {
        return column(()).boxed();
    };
    *at += 1;
    let children = (0..node.children)
        .map(|_| node_view(nodes, at, gap, act))
        .collect::<Vec<_>>();
    let id = node.id.clone();
    let act = act.clone();
    match &node.widget {
        Widget::Text(text) => label(text.clone()).boxed(),
        Widget::Heading(text) => heading(text.clone()),
        Widget::Icon(name) => match Glyph::named(name) {
            Some(which) => mark(which, Tone::Faint),
            // A name the set does not have is shown as it is.
            None => label(name.clone()).tone(Tone::Dim).boxed(),
        },
        Widget::Button(text) => {
            action(ButtonStyle::Plain, text.clone())
                .on_activate(move |world| {
                    act(world, id.clone(), Action::Click)
                })
                .boxed()
        }
        Widget::Toggle { label: text, on } => row((
            toggle(*on, move |world, on| {
                act(world, id.clone(), Action::Toggle(on));
            }),
            label(text.clone()),
        ))
        .gap(gap)
        .align(AlignItems::Center)
        .boxed(),
        Widget::Field { value, placeholder } => field(
            value.clone(),
            placeholder.clone(),
            move |world, text| {
                act(world, id.clone(), Action::Text(text));
            },
        ),
        Widget::List => list(children),
        Widget::Row => {
            row(children).gap(gap).align(AlignItems::Center).boxed()
        }
        Widget::Column => column(children).gap(gap).boxed(),
        Widget::Scroll => scroll_area(children),
        Widget::Divider => divider(),
    }
}
