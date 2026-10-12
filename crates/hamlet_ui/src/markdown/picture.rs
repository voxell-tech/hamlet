//! A picture in a reply drawn (docs/chat.md): the picture in its
//! frame, the plate that holds its place while it is read, and the
//! card of one that is on the web.

use bevy::ecs::hierarchy::ChildOf;
use bevy::math::Vec2;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius, Node,
    UiRect, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{Frame, FrameProps, column, frame, label};
use bevy_fynix::{EntityCursor, ViewExt, keyed, resource};

use super::draw::{Face, Ink, rich};
use super::{Span, Style, Target};
use crate::chat::ChatSettings;
use crate::icons::Glyph;
use crate::looks::{action, bare, note};
use crate::pictures::{
    Drawn, Pictures, Source, State, fetch, find, host,
};
use crate::theme::{BRASS_6, Lamplight, PLATE};
use crate::{Build, Ui};

fn plain(text: &str) -> Span {
    Span {
        text: text.to_string(),
        style: Style::default(),
        link: None,
    }
}

/// The words that stand for a picture, as a link to where it is.
fn words(caption: &str, target: &Target, ink: Ink) -> Ui {
    let written = match target {
        Target::Web(address) => address,
        Target::Path(path) => path,
    };
    let link = Span {
        link: Some(target.clone()),
        ..plain(if caption.is_empty() { written } else { caption })
    };
    rich(&[link], ink)
}

/// A plate in the frame of a picture.
fn framed() -> Frame {
    frame()
        .max_width(percent(100.0))
        .fill(PLATE)
        .border(1.0)
        .border_color(BRASS_6)
        .radius(Lamplight::round())
}

/// The picture `drawn`, no larger than `most`, over its caption. A
/// click leads where `source` is.
fn shown(
    drawn: &Drawn,
    source: &Source,
    caption: &str,
    most: Vec2,
) -> Ui {
    let size = drawn.fitted(most);
    let (image, anchor) = (drawn.image.clone(), source.anchor());
    let picture = Ui::new(move |cx: &mut Build| {
        let within = cx.build(bare().width(percent(100.0)));
        // A node of its own, whose image no paint of a view tints
        // and whose height follows the width it ends up with.
        let edge = 1.0;
        let outer = size + 2.0 * edge;
        cx.world.spawn((
            Node {
                width: px(outer.x),
                max_width: percent(100.0),
                aspect_ratio: Some(outer.x / outer.y),
                border: UiRect::all(px(edge)),
                border_radius: BorderRadius::all(px(
                    Lamplight::round(),
                )),
                ..Node::default()
            },
            BackgroundColor(PLATE),
            BorderColor::all(BRASS_6),
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
        .boxed()
}

/// The card of the picture at `address`, which is fetched when its
/// button is pressed.
fn card(address: &str, caption: &str, state: &State) -> Ui {
    let said = if caption.is_empty() {
        "A picture"
    } else {
        caption
    };
    let from = [
        plain("On the web, at "),
        Span {
            link: Some(Target::Web(address.to_string())),
            ..plain(&host(address).unwrap_or_default())
        },
        plain(". It is fetched only when you ask."),
    ];
    let small = Ink {
        size: 12.0,
        tone: Tone::Dim,
        face: Face::Body,
    };
    let mut parts =
        vec![label(said.to_string()).boxed(), rich(&from, small)];
    if *state == State::Failed {
        parts.push(
            label("No picture came from there.")
                .size(12.0)
                .tone(Tone::Critical)
                .boxed(),
        );
    }
    parts.push(match state {
        State::Loading => note("Fetching the picture...").boxed(),
        _ => {
            let address = address.to_string();
            action(Glyph::Globe, "SHOW PICTURE", move |world| {
                fetch(world, &address)
            })
        }
    });
    column(parts)
        .gap(6.0)
        .padding(UiRect::axes(px(10.0), px(8.0)))
        .align(AlignItems::FlexStart)
        .min_width(px(0.0))
        .max_width(percent(100.0))
        .fill(PLATE)
        .border(1.0)
        .border_color(BRASS_6)
        .radius(Lamplight::round())
        .boxed()
}

/// The picture written as `target` with the words `caption`: the
/// picture itself once it is read, and its words as a link if it is
/// none that can be drawn.
pub(super) fn picture_view(
    caption: &str,
    target: &Target,
    ink: Ink,
) -> Ui {
    let (caption, target) = (caption.to_string(), target.clone());
    Ui::new(move |cx: &mut Build| {
        let Some(source) = find(cx.world, &target) else {
            return cx.build(words(&caption, &target, ink));
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
        let state =
            resource::<Pictures, _>(move |all| all.state(&of));
        let from = source.clone();
        let view = keyed(state, move |state| match (&from, state) {
            (_, State::Ready(drawn)) => {
                shown(drawn, &from, &caption, most)
            }
            (Source::Web(address), state) => {
                card(address, &caption, state)
            }
            (Source::File(_), State::Failed) => {
                words(&caption, &target, ink)
            }
            (Source::File(_), _) => framed()
                .width(px(most.x))
                .height(px(settings.picture_wait))
                .boxed(),
        })
        .within(
            bare().width(percent(100.0)).align(AlignItems::FlexStart),
        );
        let node = cx.build(view);
        cx.world.entity_mut(node).insert(source);
        node
    })
}
