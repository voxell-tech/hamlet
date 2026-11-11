//! A picture in a reply drawn (docs/chat.md): the picture in its
//! frame, the plate that holds its place while it is read, and the
//! chip of one that is on the web or cannot be shown.

use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius, Node,
    UiRect, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label, row};
use bevy_fynix::{
    EntityCursor, ScopedExt, ViewExt, derived, keyed, resource,
};
use hamlet_ui::kinds::inset;
use hamlet_ui::markdown::{Face, Ink, Span, Style, Target, rich};
use hamlet_ui::styles::{ICON, action, bare, note, small_rules};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View};

use crate::chat::ChatSettings;
use crate::pictures::{
    Drawn, Pictures, Source, State, fetch, find, host,
};

fn plain(text: &str) -> Span {
    Span {
        text: text.to_string(),
        style: Style::default(),
        link: None,
    }
}

/// The picture `drawn`, no larger than `most`, over its caption. A
/// click leads where `source` is.
fn shown(
    drawn: &Drawn,
    source: &Source,
    caption: &str,
    most: Vec2,
) -> impl View {
    let size = drawn.fitted(most);
    let (image, anchor) = (drawn.image.clone(), source.anchor());
    let picture = AnyView::new(move |cx: &mut Cx| {
        let edge = 1.0;
        let outer = size + 2.0 * edge;
        // The picture's width, or the room there is, set on a node
        // apart from the picture's own: with both on one, its height
        // is laid out at the width it would have had uncapped.
        let within = cx.build(
            bare().width(px(outer.x)).max_width(percent(100.0)),
        );
        let theme = cx.theme();
        // A node of its own, whose image no paint of a view tints
        // and whose height follows the width it ends up with.
        cx.world.spawn((
            Node {
                width: percent(100.0),
                aspect_ratio: Some(outer.x / outer.y),
                border: UiRect::all(px(edge)),
                border_radius: BorderRadius::all(px(theme
                    .space
                    .round)),
                ..Node::default()
            },
            BackgroundColor(theme.palette.code),
            BorderColor::all(theme.palette.gold_6),
            ImageNode::new(image).with_mode(NodeImageMode::Stretch),
            anchor,
            EntityCursor(SystemCursorIcon::Pointer),
            ChildOf(within),
        ));
        within
    });
    let mut parts = vec![picture];
    if !caption.is_empty() {
        parts.push(note(caption.to_string()).boxed());
    }
    column(parts)
        .gap(4.0)
        .width(percent(100.0))
        .align(AlignItems::FlexStart)
}

/// The small words of a picture's chip.
const SMALL: Ink = Ink {
    size: Some(12.0),
    tone: Tone::Dim,
    face: Face::Body,
};

/// The chip of the picture at `address`, on the web, which is fetched
/// when its button is pressed.
fn web_chip(
    cx: &Cx,
    address: &str,
    caption: &str,
    state: &State,
) -> impl View {
    let said = if caption.is_empty() {
        "A picture"
    } else {
        caption
    };
    let from = [
        plain(&format!("{said} · ")),
        Span {
            link: Some(Target::Web(address.to_string())),
            ..plain(&host(address).unwrap_or_default())
        },
    ];
    let mut parts = vec![
        glyph(Glyph::Mountains, ICON).tone(Tone::Faint).boxed(),
        row((rich(&from, SMALL),)).min_width(px(0.0)).boxed(),
    ];
    if *state == State::Failed {
        parts.push(
            label("Nothing came from there.")
                .tone(Tone::Critical)
                .rules(small_rules)
                .boxed(),
        );
    }
    parts.push(match state {
        State::Loading => note("Fetching...").boxed(),
        _ => {
            let address = address.to_string();
            action(Glyph::Eye, "Show picture", move |world| {
                fetch(world, &address);
            })
        }
    });
    chip(
        row(parts)
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center),
    )
}

/// `content` on an inset the window draws in a reply, set apart from
/// what the agent wrote around it without a second frame in the
/// bubble's.
fn chip(content: impl View) -> impl View {
    inset((content,)).max_width(percent(100.0))
}

/// The chip of a picture that cannot be shown: its caption, the name
/// it was written by and `why`.
fn missing(caption: &str, target: &Target, why: &str) -> impl View {
    let written = match target {
        Target::Web(address) => address,
        Target::Path(path) => path,
    };
    let mut said = Vec::new();
    if !caption.is_empty() && caption != written {
        said.push(plain(&format!("{caption} · ")));
    }
    said.push(Span {
        link: Some(target.clone()),
        ..plain(written)
    });
    said.push(plain(&format!(" {why}")));
    chip(
        row((
            glyph(Glyph::Mountains, ICON).tone(Tone::Faint),
            row((rich(&said, SMALL),)).min_width(px(0.0)),
        ))
        .gap(8.0)
        .align(AlignItems::Center),
    )
}

/// The picture written as `target` with the words `caption`: the
/// picture itself once it is read, and a card that says so if it is
/// none that can be drawn.
pub(crate) fn picture_view(
    caption: &str,
    target: &Target,
    _: Ink,
) -> AnyView {
    let (caption, target) = (caption.to_string(), target.clone());
    keyed(
        derived(|world: &World| {
            world.get_resource::<crate::links::Roots>().cloned()
        })
        .when(crate::roll::moved::<crate::links::Roots>()),
        move |_| {
            let (caption, target) = (caption.clone(), target.clone());
            AnyView::new(move |cx: &mut Cx| {
                let Some(source) = find(cx.world, &target) else {
                    let why = match &target {
                        Target::Path(path)
                            if crate::links::seek(cx.world, path)
                                .is_some() =>
                        {
                            "is no picture"
                        }
                        Target::Path(_) => "not found",
                        Target::Web(_) => "is no web address",
                    };
                    let card = missing(&caption, &target, why);
                    return cx.build(card);
                };
                cx.world.init_resource::<Pictures>();
                let settings = cx
                    .world
                    .get_resource::<ChatSettings>()
                    .cloned()
                    .unwrap_or_default();
                let most = Vec2::new(
                    settings.picture_width,
                    settings.picture_height,
                );
                let of = source.clone();
                let state = resource::<Pictures, _>(move |all| {
                    all.state(&of)
                });
                let from = source.clone();
                let view =
                    keyed(state, move |state| match (&from, state) {
                        (_, State::Ready(drawn)) => {
                            shown(drawn, &from, &caption, most)
                                .boxed()
                        }
                        (Source::Web(address), state) => {
                            let (address, caption, state) = (
                                address.clone(),
                                caption.clone(),
                                state.clone(),
                            );
                            AnyView::new(move |cx: &mut Cx| {
                                let card = web_chip(
                                    cx, &address, &caption, &state,
                                );
                                cx.build(card)
                            })
                        }
                        (Source::File(_), State::Failed) => {
                            let (caption, target) =
                                (caption.clone(), target.clone());
                            AnyView::new(move |cx: &mut Cx| {
                                let card = missing(
                                    &caption,
                                    &target,
                                    "could not be read",
                                );
                                cx.build(card)
                            })
                        }
                        (Source::File(_), _) => {
                            let (wide, tall) =
                                (most.x, settings.picture_wait);
                            AnyView::new(move |cx: &mut Cx| {
                                let plate = inset(())
                                    .max_width(percent(100.0))
                                    .width(px(wide))
                                    .height(px(tall));
                                cx.build(plate)
                            })
                        }
                    })
                    .within(
                        bare()
                            .width(percent(100.0))
                            .align(AlignItems::FlexStart),
                    );
                let node = cx.build(view);
                cx.world.entity_mut(node).insert(source);
                node
            })
        },
    )
    .within(bare())
    .boxed()
}
