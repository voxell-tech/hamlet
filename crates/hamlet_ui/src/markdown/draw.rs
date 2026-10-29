//! Markdown drawn: a view for each block.

use std::sync::Arc;

use bevy::color::Color;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::world::World;
use bevy::text::{
    FontSize, FontSource, FontStyle, FontWeight, Strikethrough,
    TextBackgroundColor, TextColor, TextFont, TextSpan, Underline,
};
use bevy::ui::{
    AlignItems, Display, JustifyContent, MaxTrackSizingFunction,
    MinTrackSizingFunction, Node, RepeatedGridTrack, UiRect, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{TextTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row,
};
use bevy_fynix::{
    EntityCursor, Signal, ViewExt, derived, each, keyed, resource,
};

use super::parse::{parse, plain};
use super::{
    Align, Anchor, Area, Block, Kind, Marker, PathLinks, PictureView,
    Row, Span, Style, Target,
};
use crate::platform::{
    Display as DisplayFace, DisplayText, Mono, MonoText,
};
use crate::styles::{Fills, bare};
use crate::theme::Palette;
use crate::{AnyView, Cx, View};

/// Takes the link off a path that `blocks` end with, which may be
/// the start of a longer one, as while the words still grow.
pub fn unsettle(blocks: &mut [Arc<Block>]) {
    let spans = match blocks
        .last_mut()
        .map(|block| &mut Arc::make_mut(block).kind)
    {
        Some(Kind::Paragraph(spans) | Kind::Heading(_, spans)) => {
            spans
        }
        Some(Kind::Table { rows, .. }) => {
            match rows.last_mut().and_then(|row| row.last_mut()) {
                Some(cell) => cell,
                None => return,
            }
        }
        _ => return,
    };
    if let Some(last) = spans.last_mut()
        && matches!(last.link, Some(Target::Path(_)))
    {
        last.link = None;
    }
}

/// The quote bars and list levels drawn, however deep a block sits.
const DEEPEST: u8 = 6;

/// The width of a list level, which a marker sits in.
const STEP: f32 = 22.0;

/// The face a block's text is set in.
#[derive(Clone, Copy, PartialEq)]
pub enum Face {
    Body,
    Strong,
    Heading,
}

/// The size, the tone and the face of a block's text.
#[derive(Clone, Copy)]
pub struct Ink {
    /// `None` for the theme's body size.
    pub size: Option<f32>,
    pub tone: Tone,
    pub face: Face,
}

/// `spans` with the place each link leads. A path that names no
/// file, or any path without [`PathLinks`], is no link, and its words
/// join those beside it again.
fn anchored(
    world: &mut World,
    spans: Vec<Span>,
) -> Vec<(Span, Option<Anchor>)> {
    let resolve =
        world.get_resource::<PathLinks>().map(|links| links.resolve);
    let mut all = Vec::<(Span, Option<Anchor>)>::new();
    for mut span in spans {
        let anchor = match span.link.take() {
            Some(Target::Web(address)) => Some(Anchor::Web(address)),
            Some(Target::Path(path)) => resolve
                .and_then(|resolve| resolve(world, &path))
                .map(Anchor::File),
            None => None,
        };
        match all.last_mut() {
            Some((last, None))
                if anchor.is_none() && last.style == span.style =>
            {
                last.text.push_str(&span.text);
            }
            _ => all.push((span, anchor)),
        }
    }
    all
}

/// A wrapped paragraph of `spans`: a label with a text span under it
/// for each. A link is a span the pointer finds by itself, on every
/// line it runs over.
pub fn rich(spans: &[Span], ink: Ink) -> AnyView {
    let spans = spans.to_vec();
    let mut links = resource::<PathLinks, _>(|_| ());
    keyed(
        derived(|world: &World| {
            world
                .get_resource::<PathLinks>()
                .map(|links| links.generation)
        })
        .when(move |world: &World| links.changed(world)),
        move |_| {
            let spans = spans.clone();
            AnyView::new(move |cx: &mut Cx| {
                let spans = anchored(cx.world, spans);
                let theme = cx.theme();
                let size = ink.size.unwrap_or(theme.text.body);
                let root = cx.build(label("").size(size));
                let mono = cx
                    .world
                    .get_resource::<Mono>()
                    .map(|m| m.0.clone());
                let display = cx
                    .world
                    .get_resource::<DisplayFace>()
                    .map(|d| d.0.clone());
                for (span, anchor) in spans {
                    let Style {
                        bold,
                        italic,
                        code,
                        strike,
                    } = span.style;
                    let tone = match anchor {
                        Some(_) => Tone::Accent,
                        None => ink.tone,
                    };
                    let face = match (code, ink.face) {
                        (true, _) => mono.clone(),
                        (false, Face::Heading) => display.clone(),
                        _ => None,
                    };
                    let font = TextFont {
                        font: face
                            .map(FontSource::from)
                            .unwrap_or_default(),
                        font_size: FontSize::Px(if code {
                            size - 1.0
                        } else {
                            size
                        }),
                        weight: if bold || ink.face != Face::Body {
                            FontWeight::BOLD
                        } else {
                            FontWeight::NORMAL
                        },
                        style: if italic {
                            FontStyle::Italic
                        } else {
                            FontStyle::Normal
                        },
                        ..TextFont::default()
                    };
                    let mut entity = cx.world.spawn((
                        TextSpan(span.text),
                        font,
                        TextColor(theme.tone(tone)),
                        ChildOf(root),
                    ));
                    if code {
                        entity.insert((
                            MonoText,
                            TextBackgroundColor(theme.palette.code),
                        ));
                    } else if ink.face == Face::Heading {
                        entity.insert(DisplayText);
                    }
                    if let Some(anchor) = anchor {
                        entity.insert((
                            anchor,
                            Underline,
                            EntityCursor(SystemCursorIcon::Pointer),
                        ));
                    }
                    if strike {
                        entity.insert(Strikethrough);
                    }
                }
                root
            })
        },
    )
    .within(bare())
    .boxed()
}

fn code_view(cx: &Cx, lang: &str, text: &str) -> impl View {
    let theme = cx.theme();
    let text = text.to_string();
    let body = AnyView::new(move |cx: &mut Cx| {
        // A long line wraps: nothing in the chat scrolls sideways.
        let node = cx.build(label(text).size(13.0));
        let mono =
            cx.world.get_resource::<Mono>().map(|m| m.0.clone());
        let mut entity = cx.world.entity_mut(node);
        entity.insert(MonoText);
        if let Some(mono) = mono
            && let Some(mut font) = entity.get_mut::<TextFont>()
        {
            font.font = FontSource::Handle(mono);
        }
        node
    });
    let mut parts = Vec::<AnyView>::new();
    if !lang.is_empty() {
        parts.push(
            label(lang.to_string())
                .size(11.0)
                .tone(Tone::Dim)
                .boxed(),
        );
    }
    parts.push(body);
    column(parts)
        .gap(4.0)
        .padding(UiRect::axes(px(10.0), px(8.0)))
        .min_width(px(0.0))
        .fill(theme.palette.code)
        .border(1.0)
        .border_color(theme.palette.gold_6)
        .radius(theme.space.round)
}

/// Columns that share the width between them and may shrink to
/// nothing, so that the words in a cell wrap and the table never
/// grows wider than the chat.
fn shared(columns: u16) -> Vec<RepeatedGridTrack> {
    RepeatedGridTrack::minmax::<Vec<_>>(
        columns,
        MinTrackSizingFunction::Px(0.0),
        MaxTrackSizingFunction::Fraction(1.0),
    )
}

/// A grid whose lines are the fill showing between its cells.
fn table_view(
    cx: &Cx,
    align: &[Align],
    rows: &[Row],
    tone: Tone,
) -> AnyView {
    let palette = &cx.theme().palette;
    let widest = rows.iter().map(Vec::len).max().unwrap_or(0);
    let columns = align.len().max(widest).clamp(1, 64);
    let mut cells = Vec::<AnyView>::new();
    for (at, cell_row) in rows.iter().enumerate() {
        for column_at in 0..columns {
            let spans = cell_row
                .get(column_at)
                .map_or(&[][..], Vec::as_slice);
            let (face, fill): (_, fn(&Palette) -> Color) = if at == 0
            {
                (Face::Strong, |p| p.code)
            } else {
                (Face::Body, |p| p.surface)
            };
            let side = match align.get(column_at) {
                Some(Align::Center) => AlignItems::Center,
                Some(Align::End) => AlignItems::FlexEnd,
                _ => AlignItems::FlexStart,
            };
            let ink = Ink {
                size: Some(13.0),
                tone,
                face,
            };
            cells.push(
                column((rich(spans, ink),))
                    .padding(UiRect::axes(px(10.0), px(5.0)))
                    .min_width(px(0.0))
                    .align(side)
                    .fill(fill(palette))
                    .boxed(),
            );
        }
    }
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(
            column(cells)
                .gap(1.0)
                .padding(UiRect::all(px(1.0)))
                .min_width(px(0.0))
                .fill(cx.theme().palette.gold_6),
        );
        if let Some(mut ui) = cx.world.get_mut::<Node>(node) {
            ui.display = Display::Grid;
            ui.grid_template_columns = shared(columns as u16);
        }
        node
    })
}

fn block_view(
    cx: &Cx,
    at: usize,
    block: &Block,
    tone: Tone,
) -> AnyView {
    let above = match (at, &block.kind, block.marker) {
        (0, ..) => 0.0,
        (_, Kind::Heading(..), _) => 14.0,
        (_, _, Some(_)) => 4.0,
        _ => 8.0,
    };
    let tone = if block.quote > 0 { Tone::Dim } else { tone };
    let body = Ink {
        size: None,
        tone,
        face: Face::Body,
    };
    let inside = match &block.kind {
        Kind::Paragraph(spans) => rich(spans, body),
        Kind::Heading(level, spans) => rich(
            spans,
            Ink {
                size: Some(match level {
                    1 => 20.0,
                    2 => 17.0,
                    _ => 15.0,
                }),
                face: Face::Heading,
                ..body
            },
        ),
        Kind::Code { lang, text } => {
            code_view(cx, lang, text).boxed()
        }
        Kind::Rule => frame()
            .height(px(1.0))
            .margin(UiRect::vertical(px(4.0)))
            .fill(cx.theme().palette.gold_6)
            .boxed(),
        Kind::Table { align, rows } => {
            table_view(cx, align, rows, tone)
        }
        Kind::Picture { caption, target } => {
            let draw =
                cx.world.get_resource::<PictureView>().map_or(
                    picture_words as fn(_, _, _) -> _,
                    |view| view.0,
                );
            draw(caption, target, body)
        }
    };

    let mut cells = Vec::<AnyView>::new();
    // A bar runs the whole height of its row, so the bars of the
    // blocks of one quote meet.
    for _ in 0..block.quote.min(DEEPEST) {
        cells.push(
            frame()
                .width(px(3.0))
                .margin(UiRect::right(px(10.0)))
                .shrink(0.0)
                .fill(cx.theme().palette.gold_5)
                .boxed(),
        );
    }
    let marked = block.marker.is_some() as u8;
    let levels = block.indent.min(DEEPEST).saturating_sub(marked);
    if levels > 0 {
        cells.push(
            frame()
                .width(px(STEP * f32::from(levels)))
                .shrink(0.0)
                .boxed(),
        );
    }
    if let Some(marker) = block.marker {
        let mark = match marker {
            Marker::Bullet => "\u{2022}".to_string(),
            Marker::Number(n) => format!("{n}."),
            Marker::Task(true) => "[x]".to_string(),
            Marker::Task(false) => "[ ]".to_string(),
        };
        cells.push(
            row((label(mark).tone(Tone::Dim).wrap(false),))
                .min_width(px(STEP))
                .padding(UiRect::new(
                    px(0.0),
                    px(8.0),
                    px(above),
                    px(0.0),
                ))
                .justify(JustifyContent::FlexEnd)
                .shrink(0.0)
                .boxed(),
        );
    }
    cells.push(
        column((inside,))
            .fills_width()
            .padding(UiRect::top(px(above)))
            .boxed(),
    );
    row(cells).gap(0.0).boxed()
}

/// A block that is equal to the one it is compared with when it is
/// the same allocation, without looking inside.
#[derive(Clone)]
struct Same(Arc<Block>);

impl PartialEq for Same {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

/// A Markdown document, one view per block. A block that has not
/// changed keeps its nodes while the document grows. Its words are
/// selected as one.
pub fn document(
    blocks: Signal<Vec<Arc<Block>>>,
    tone: Tone,
) -> impl View {
    let blocks = each(
        blocks.map(|blocks| {
            blocks.into_iter().enumerate().collect::<Vec<_>>()
        }),
        |(at, block): &(usize, Arc<Block>)| {
            (*at, Same(block.clone()))
        },
        move |(at, block)| {
            let (at, block) = (*at, block.clone());
            AnyView::new(move |cx: &mut Cx| {
                let view = block_view(cx, at, &block, tone);
                cx.build(view)
            })
        },
    )
    .within(bare());
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(blocks);
        cx.world.entity_mut(node).insert(Area);
        node
    })
}

/// The words that stand for a picture, as a link to where it is.
pub fn picture_words(
    caption: &str,
    target: &Target,
    ink: Ink,
) -> AnyView {
    let written = match target {
        Target::Web(address) => address,
        Target::Path(path) => path,
    };
    let link = Span {
        text: if caption.is_empty() { written } else { caption }
            .to_string(),
        style: Style::default(),
        link: Some(target.clone()),
    };
    rich(&[link], ink)
}

/// Words set as they were typed, with `tail` after them, if there
/// may be a link in them.
pub fn typed(text: &str, tail: &str) -> Option<AnyView> {
    let mut spans = plain(text);
    if spans.iter().all(|span| span.link.is_none()) {
        return None;
    }
    spans.push(Span {
        text: tail.to_string(),
        style: Style::default(),
        link: None,
    });
    Some(rich(
        &spans,
        Ink {
            size: None,
            tone: Tone::Body,
            face: Face::Body,
        },
    ))
}

/// A Markdown document that is drawn once, one view per block. Its
/// words are selected as one.
pub fn fixed(text: &str, tone: Tone) -> impl View {
    let blocks = parse(text);
    AnyView::new(move |cx: &mut Cx| {
        let views = blocks
            .iter()
            .enumerate()
            .map(|(at, block)| block_view(cx, at, block, tone))
            .collect::<Vec<_>>();
        cx.build(column(views).gap(0.0).tagged(Area))
    })
}
