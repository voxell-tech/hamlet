//! Markdown text drawn as a column of blocks: headings, paragraphs,
//! list items, code and rules. A link shows as its words.

mod parse;
#[cfg(test)]
mod tests;

use bevy::ecs::hierarchy::ChildOf;
use bevy::text::{
    FontSize, FontSource, FontStyle, FontWeight, Strikethrough,
    TextBackgroundColor, TextColor, TextFont, TextSpan,
};
use bevy::ui::{UiRect, px};
use bevy_fynix::tokens::{TextTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row,
};
use bevy_fynix::{ViewExt, keyed};

use self::parse::{Block, Kind, Span, Style, parse};
use super::bind::At;
use crate::looks::{Fills, bare};
use crate::platform::{Display, DisplayText, Mono, MonoText};
use crate::{AnyView, Cx};

/// The space between two blocks.
const GAP: f32 = 6.0;

/// The width of a list level, which a mark sits in.
const STEP: f32 = 18.0;

/// The size of a heading of each level, and of the rest.
const HEADINGS: [f32; 3] = [18.0, 15.0, 13.0];
const BODY: f32 = 13.0;

/// The view of the markdown node at `at`, drawn again when its text
/// changes.
pub(super) fn view(at: &At) -> AnyView {
    keyed(at.text(), |text| blocks(parse(text)))
        .within(bare())
        .boxed()
}

fn blocks(blocks: Vec<Block>) -> AnyView {
    column(blocks.iter().map(block).collect::<Vec<_>>())
        .gap(GAP)
        .fills_width()
        .boxed()
}

fn block(block: &Block) -> AnyView {
    let inside = match &block.kind {
        Kind::Text(spans) => rich(spans, BODY, false),
        Kind::Heading(level, spans) => {
            let size = HEADINGS[usize::from(*level).clamp(1, 3) - 1];
            rich(spans, size, true)
        }
        Kind::Code(text) => code(text),
        Kind::Rule => AnyView::new(|cx: &mut Cx| {
            let line = cx.theme().palette.brass_6;
            cx.build(frame().height(px(1.0)).fill(line))
        }),
    };
    let mut cells = Vec::<AnyView>::new();
    let marked = usize::from(block.mark.is_some());
    let levels = block.indent.saturating_sub(marked);
    if levels > 0 {
        cells.push(
            frame()
                .width(px(STEP * levels as f32))
                .shrink(0.0)
                .boxed(),
        );
    }
    if let Some(mark) = &block.mark {
        cells.push(
            column((label(mark.clone())
                .size(BODY)
                .tone(Tone::Dim)
                .wrap(false),))
            .width(px(STEP))
            .shrink(0.0)
            .boxed(),
        );
    }
    cells.push(column((inside,)).fills_width().boxed());
    row(cells).fills_width().boxed()
}

/// A wrapped run of `spans`: a label with a text span under it for
/// each.
fn rich(spans: &[Span], size: f32, heading: bool) -> AnyView {
    let spans = spans.to_vec();
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let root = cx.build(label("").size(size));
        let mono =
            cx.world.get_resource::<Mono>().map(|m| m.0.clone());
        let display =
            cx.world.get_resource::<Display>().map(|d| d.0.clone());
        for span in spans {
            let Style {
                bold,
                italic,
                code,
                strike,
            } = span.style;
            let face = match (code, heading) {
                (true, _) => mono.clone(),
                (false, true) => display.clone(),
                _ => None,
            };
            let font = TextFont {
                font: face.map(FontSource::from).unwrap_or_default(),
                font_size: FontSize::Px(if code {
                    size - 1.0
                } else {
                    size
                }),
                weight: if bold || heading {
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
                TextColor(theme.tone(Tone::Body)),
                ChildOf(root),
            ));
            if code {
                entity.insert((
                    MonoText,
                    TextBackgroundColor(theme.palette.plate),
                ));
            } else if heading {
                entity.insert(DisplayText);
            }
            if strike {
                entity.insert(Strikethrough);
            }
        }
        root
    })
}

/// A code block: its lines in the monospace face on a plate.
fn code(text: &str) -> AnyView {
    let text = text.to_string();
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let plate = theme.palette.plate;
        let edge = theme.palette.brass_6;
        let radius = theme.space.round;
        cx.build(
            column((label(text).size(BODY - 1.0).tagged(MonoText),))
                .padding(UiRect::axes(px(10.0), px(8.0)))
                .fills_width()
                .fill(plate)
                .border(1.0)
                .border_color(edge)
                .radius(radius),
        )
    })
}
