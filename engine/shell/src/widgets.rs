//! A widget tree, as a plugin describes a panel, drawn with the
//! shell's widgets.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, checkbox, column, frame, label,
    row, scroll, text_field,
};
use hamlet_engine_api::{
    Action, Widget, WidgetId, WidgetNode, WidgetTree,
};

use crate::fonts::DisplayText;
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
        let theme = cx.theme();
        let looks = Looks {
            gap: theme.space.gap,
            line: theme.space.divider,
            hairline: theme.colors.hairline,
        };
        let mut at = 0;
        cx.build(node_view(&nodes, &mut at, &looks, &act))
    })
}

/// What a tree's widgets take from the theme.
struct Looks {
    gap: f32,
    line: f32,
    hairline: bevy::color::Color,
}

/// The node at `at` and those under it, with `at` moved past them.
fn node_view<F>(
    nodes: &[WidgetNode],
    at: &mut usize,
    looks: &Looks,
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
        .map(|_| node_view(nodes, at, looks, act))
        .collect::<Vec<_>>();
    let id = node.id.clone();
    let act = act.clone();
    match &node.widget {
        Widget::Text(text) => label(text.clone()).boxed(),
        Widget::Heading(text) => {
            label(text.clone()).tagged(DisplayText).boxed()
        }
        // The engine has no set of icons yet: one shows its name.
        Widget::Icon(name) => {
            label(name.clone()).tone(Tone::Faint).boxed()
        }
        Widget::Button(text) => button(label(text.clone()))
            .on_activate(move |world| {
                act(world, id.clone(), Action::Click)
            })
            .boxed(),
        Widget::Toggle { label: text, on } => row((
            checkbox(*on).on_change(move |world, on| {
                act(world, id.clone(), Action::Toggle(on));
            }),
            label(text.clone()),
        ))
        .gap(looks.gap)
        .align(AlignItems::Center)
        .boxed(),
        Widget::Field { value, placeholder } => {
            let field =
                text_field(value.clone(), move |world, text| {
                    act(world, id.clone(), Action::Text(text));
                });
            if placeholder.is_empty() {
                return field.boxed();
            }
            // The fields have no placeholder: it is written above.
            column((
                label(placeholder.clone()).tone(Tone::Dim),
                field,
            ))
            .gap(looks.gap / 2.0)
            .boxed()
        }
        Widget::List => column(children).gap(looks.gap / 2.0).boxed(),
        Widget::Row => row(children)
            .gap(looks.gap)
            .align(AlignItems::Center)
            .boxed(),
        Widget::Column => column(children).gap(looks.gap).boxed(),
        Widget::Scroll => scroll(children)
            .gap(looks.gap)
            .grow(1.0)
            .min_height(px(0.0))
            .boxed(),
        Widget::Divider => frame()
            .height(px(looks.line))
            .fill(looks.hairline)
            .boxed(),
    }
}
