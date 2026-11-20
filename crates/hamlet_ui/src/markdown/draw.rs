//! Markdown drawn: a view for each block.

use std::sync::Arc;

use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::world::World;
use bevy::text::{
    FontSize, FontSource, FontStyle, FontWeight, LineHeight,
    Strikethrough, TextBackgroundColor, TextColor, TextFont,
    TextSpan, Underline,
};
use bevy::ui::{
    AlignItems, Display, FlexDirection, JustifyContent,
    MaxTrackSizingFunction, MinTrackSizingFunction, Node,
    RepeatedGridTrack, UiRect, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{TextTokens, Tone};
use bevy_fynix::views::{
    Atom, BehaviorExt, FrameProps, ItalicFaces, LineColor,
    Span as Run, column, frame, label, row,
};
use bevy_fynix::{
    EntityCursor, ScopedExt, Signal, ViewExt, derived, each, keyed,
    resource,
};

use super::chips::seed;
use super::parse::{parse, plain};
use super::{
    AgentLinks, Align, Anchor, Area, Block, IsAddress, Kind, Marker,
    PathLinks, PictureView, Row, Span, Style, Target, is_address_in,
};
use crate::kinds::inset;
use crate::styles::{Fills, bare, small_rules};
use crate::theme::Palette;
use crate::widgets::platform::{Mono, MonoText};
use crate::{AnyView, Cx, View};

/// `blocks` set as growing words: addresses as words, and no link on
/// a trailing path or address, which may still grow.
pub fn unsettle(blocks: &mut [Arc<Block>]) {
    let names_agent = |spans: &[Span]| {
        spans
            .iter()
            .any(|span| matches!(span.link, Some(Target::Agent(_))))
    };
    for block in blocks.iter_mut() {
        let named = match &block.kind {
            Kind::Paragraph(spans) | Kind::Heading(_, spans) => {
                names_agent(spans)
            }
            Kind::Table { rows, .. } => {
                rows.iter().flatten().any(|cell| names_agent(cell))
            }
            _ => false,
        };
        if !named {
            continue;
        }
        match &mut Arc::make_mut(block).kind {
            Kind::Paragraph(spans) | Kind::Heading(_, spans) => {
                spell_out(spans);
            }
            Kind::Table { rows, .. } => {
                rows.iter_mut().flatten().for_each(|c| spell_out(c));
            }
            _ => {}
        }
    }
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
        && matches!(
            last.link,
            Some(Target::Path(_) | Target::AgentWords(_))
        )
    {
        last.link = None;
    }
}

fn spell_out(spans: &mut [Span]) {
    for span in spans {
        if let Some(Target::Agent(address)) = &span.link {
            span.link = Some(Target::AgentWords(address.clone()));
        }
    }
}

/// The quote bars and list levels drawn, however deep a block sits.
const DEEPEST: u8 = 6;

/// The width of a list level, which a marker sits in.
const STEP: f32 = 22.0;

/// How far apart the lines of a page are, in its font's size.
const PAGE_LEADING: f32 = 1.4;

/// The character set between the letters of inline code on a page,
/// so that no line breaks inside it. It is drawn as nothing.
pub const JOINER: char = '\u{2060}';

/// How a document's words are set.
#[derive(Clone, Copy, PartialEq)]
enum Setting {
    /// As they come, a line breaking wherever one may.
    Running,
    /// To be read as a page: the lines further apart, and inline code
    /// kept whole on one line.
    Page,
}

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

/// A part of a drawn line.
enum Piece {
    Words(Span, Option<Anchor>),
    Chip(Atom),
    /// An address drawn as words in the link's colour.
    Address(Span),
}

/// `spans`, drawn under `under`, with the place each link leads. A
/// link that leads nowhere joins the words beside it.
fn anchored(
    world: &mut World,
    under: Option<Entity>,
    spans: Vec<Span>,
) -> Vec<Piece> {
    let paths =
        world.get_resource::<PathLinks>().map(|links| links.resolve);
    let agents = world
        .get_resource::<AgentLinks>()
        .map(|links| links.resolve);
    let mut all = Vec::<Piece>::new();
    for (at, mut span) in spans.into_iter().enumerate() {
        let anchor = match span.link.take() {
            Some(Target::Web(address)) => Some(Anchor::Web(address)),
            Some(Target::Path(path)) => paths
                .and_then(|resolve| resolve(world, under, &path))
                .map(Anchor::File),
            Some(Target::Agent(address)) => {
                let chip = |world: &mut World, words: &str| {
                    agents.and_then(|resolve| {
                        resolve(
                            world,
                            &address,
                            words,
                            at.to_string(),
                        )
                    })
                };
                if let Some(chip) = chip(world, &span.text) {
                    all.push(Piece::Chip(chip));
                    continue;
                }
                // Bracketed words that are not its name follow the
                // chip.
                if let Some(rest) = span
                    .text
                    .strip_prefix(address.as_str())
                    .filter(|rest| !rest.is_empty())
                    && let Some(chip) = chip(world, &address)
                {
                    all.push(Piece::Chip(chip));
                    span.text = rest.to_string();
                }
                None
            }
            Some(Target::AgentWords(_)) => {
                all.push(Piece::Address(span));
                continue;
            }
            None => None,
        };
        match all.last_mut() {
            Some(Piece::Words(last, None))
                if anchor.is_none() && last.style == span.style =>
            {
                last.text.push_str(&span.text);
            }
            _ => all.push(Piece::Words(span, anchor)),
        }
    }
    all
}

/// A wrapped paragraph of `spans`: a label with a text span under it
/// for each. A link is a span the pointer finds by itself, on every
/// line it runs over.
pub fn rich(spans: &[Span], ink: Ink) -> AnyView {
    set_rich(spans, ink, Setting::Running)
}

/// `text` with a [`JOINER`] between each two of its letters.
fn kept_whole(text: &str) -> String {
    let mut joined = String::with_capacity(text.len() * 4);
    for (at, letter) in text.chars().enumerate() {
        if at > 0 {
            joined.push(JOINER);
        }
        joined.push(letter);
    }
    joined
}

/// `view`, built again when [`PathLinks`] or [`AgentLinks`] change.
fn relinked(
    view: impl Fn() -> AnyView + Send + Sync + 'static,
) -> AnyView {
    let mut paths = resource::<PathLinks, _>(|_| ());
    let mut agents = resource::<AgentLinks, _>(|_| ());
    keyed(
        derived(|world: &World| {
            (
                world
                    .get_resource::<PathLinks>()
                    .map(|links| links.generation),
                world
                    .get_resource::<AgentLinks>()
                    .map(|links| links.generation),
            )
        })
        .when(move |world: &World| {
            // Both are asked, so neither change is missed.
            let paths = paths.changed(world);
            agents.changed(world) || paths
        }),
        move |_| view(),
    )
    .within(bare())
    .boxed()
}

/// A [`rich`] paragraph set as `setting` says.
fn set_rich(spans: &[Span], ink: Ink, setting: Setting) -> AnyView {
    let spans = spans.to_vec();
    relinked(move || {
        let spans = spans.clone();
        AnyView::new(move |cx: &mut Cx| {
            let under = cx.parent();
            let pieces = anchored(cx.world, under, spans);
            if pieces
                .iter()
                .any(|piece| matches!(piece, Piece::Chip(_)))
            {
                return chipped(cx, pieces, ink);
            }
            let spans =
                pieces.into_iter().filter_map(|piece| match piece {
                    Piece::Words(span, anchor) => {
                        Some((span, anchor, false))
                    }
                    Piece::Address(span) => Some((span, None, true)),
                    Piece::Chip(_) => None,
                });
            let theme = cx.theme();
            let size = ink.size.unwrap_or(theme.text.body);
            let root = cx.build(label("").size(size));
            let leading = match setting {
                Setting::Running => LineHeight::default(),
                Setting::Page => {
                    LineHeight::RelativeToFont(PAGE_LEADING)
                }
            };
            cx.world.entity_mut(root).insert(leading);
            let mono =
                cx.world.get_resource::<Mono>().map(|m| m.0.clone());
            let faces = cx
                .world
                .get_resource::<ItalicFaces>()
                .map(|faces| ItalicFaces(faces.0.clone()))
                .unwrap_or_default();
            for (span, anchor, address) in spans {
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
                let style = if italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                };
                let face = mono
                    .clone()
                    .filter(|_| code)
                    .map(FontSource::from)
                    .unwrap_or_default();
                let font = TextFont {
                    font: faces.face(face, style),
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
                    style,
                    ..TextFont::default()
                };
                let text = if code && setting == Setting::Page {
                    kept_whole(&span.text)
                } else {
                    span.text
                };
                let mut entity = cx.world.spawn((
                    TextSpan(text),
                    leading,
                    font,
                    TextColor(theme.tone(tone)),
                    ChildOf(root),
                ));
                // Names and links are references, in blue.
                if address {
                    entity.insert(TextColor(
                        theme.tone(Tone::Reference),
                    ));
                }
                if code {
                    entity.insert((
                        MonoText,
                        TextColor(theme.tone(Tone::Reference)),
                        TextBackgroundColor(
                            theme.palette.status.info.fill,
                        ),
                    ));
                }
                if let Some(anchor) = anchor {
                    entity.insert((
                        anchor,
                        TextColor(theme.tone(Tone::Reference)),
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
    })
}

/// A [`set_rich`] paragraph with chips in its line.
fn chipped(cx: &mut Cx, pieces: Vec<Piece>, ink: Ink) -> Entity {
    let theme = cx.theme();
    let size = ink.size.unwrap_or(theme.text.body);
    let reference = theme.tone(Tone::Reference);
    let code_fill = theme.palette.status.info.fill;
    let mono = cx.world.get_resource::<Mono>().map(|m| m.0.clone());
    let mut runs = Vec::new();
    // Whether each run is code, and the place it leads.
    let mut marks = Vec::<(bool, Option<Anchor>)>::new();
    let mut chips = Vec::new();
    for piece in pieces {
        let (span, anchor, address) = match piece {
            Piece::Words(span, anchor) => (span, anchor, false),
            Piece::Address(span) => (span, None, true),
            Piece::Chip(atom) => {
                chips.push(atom.text().to_string());
                runs.push(Run::atom(atom));
                marks.push((false, None));
                continue;
            }
        };
        let Style {
            bold,
            italic,
            code,
            strike,
        } = span.style;
        let mut run = Run::new(span.text);
        if bold || ink.face != Face::Body {
            run = run.bold();
        }
        if italic {
            run = run.italic();
        }
        if code {
            run = run
                .size(size - 1.0)
                .color(reference)
                .background(code_fill);
            if let Some(mono) = &mono {
                run = run.font(mono.clone());
            }
        }
        if address {
            run = run.color(reference);
        }
        if anchor.is_some() {
            run = run.color(reference).underline(LineColor::Ink);
        }
        if strike {
            run = run.strikethrough(LineColor::Ink);
        }
        runs.push(run);
        marks.push((code, anchor));
    }
    let body = cx.build(
        bevy_fynix::views::rich(runs)
            .size(size)
            .tone(ink.tone)
            .atoms()
            // In a row the text would never wrap, as in a table cell.
            .direction(FlexDirection::Column)
            .max_width(percent(100.0)),
    );
    seed(cx.world, body, chips);
    for (kid, (code, anchor)) in
        runs_of(cx.world, body).into_iter().zip(marks)
    {
        if code {
            cx.world.entity_mut(kid).insert(MonoText);
        }
        if let Some(anchor) = anchor {
            cx.world.entity_mut(kid).insert((
                anchor,
                EntityCursor(SystemCursorIcon::Pointer),
            ));
        }
    }
    body
}

/// The node of each run of a rich text with atoms, in order.
fn runs_of(world: &World, body: Entity) -> Vec<Entity> {
    world
        .get::<Children>(body)
        .and_then(|kids| kids.first().copied())
        .and_then(|text| world.get::<Children>(text))
        .map(|kids| kids.to_vec())
        .unwrap_or_default()
}

fn code_view(cx: &Cx, lang: &str, text: &str) -> impl View {
    let theme = cx.theme();
    let (text, size) = (text.to_string(), theme.text.small);
    let body = AnyView::new(move |cx: &mut Cx| {
        // A long line wraps: nothing in the chat scrolls sideways.
        let node = cx.build(label(text).size(size));
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
                .tone(Tone::Dim)
                .rules(small_rules)
                .boxed(),
        );
    }
    parts.push(body);
    inset(parts)
}

/// Columns at least as wide as their widest word or chip, sharing the
/// rest in proportion to `widths`, so the table never outgrows the
/// chat.
fn shared(widths: &[usize]) -> Vec<RepeatedGridTrack> {
    widths
        .iter()
        .map(|&width| {
            RepeatedGridTrack::minmax(
                1,
                MinTrackSizingFunction::MinContent,
                MaxTrackSizingFunction::Fraction(width.max(1) as f32),
            )
        })
        .collect()
}

/// The letters in each column's widest cell.
fn column_widths(rows: &[Row], columns: usize) -> Vec<usize> {
    (0..columns)
        .map(|at| {
            rows.iter()
                .filter_map(|row| row.get(at))
                .map(|cell| {
                    cell.iter()
                        .map(|span| span.text.chars().count())
                        .sum::<usize>()
                })
                .max()
                .unwrap_or(0)
        })
        .collect()
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
    let widths = column_widths(rows, columns);
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
                    .padding(UiRect::axes(px(12.0), px(6.0)))
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
            ui.grid_template_columns = shared(&widths);
        }
        node
    })
}

fn block_view(
    cx: &Cx,
    at: usize,
    block: &Block,
    tone: Tone,
    setting: Setting,
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
        Kind::Paragraph(spans) => set_rich(spans, body, setting),
        Kind::Heading(level, spans) => set_rich(
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
            setting,
        ),
        Kind::Code { lang, text } => {
            code_view(cx, lang, text).boxed()
        }
        Kind::Rule => frame()
            .height(px(crate::styles::LINE))
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
                .margin(UiRect::right(px(8.0)))
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
                let view = block_view(
                    cx,
                    at,
                    &block,
                    tone,
                    Setting::Running,
                );
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
        Target::Path(path)
        | Target::Agent(path)
        | Target::AgentWords(path) => path,
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
pub fn typed(
    text: &str,
    tail: &str,
    is_address: IsAddress,
) -> Option<AnyView> {
    let mut spans = plain(text, is_address);
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

/// The character that stands for an atom in the words of
/// [`typed_with`]. It is white space, so it ends the word before it
/// as a view in the line does.
pub const ATOM: char = '\u{2028}';

/// The words each atom of a [`typed_with`] text is copied as, by the
/// byte it stands at.
#[derive(Component, Clone, Default, PartialEq, Debug)]
pub struct AtomWords(pub Vec<(usize, String)>);

/// Words set as [`typed`] sets them, whether or not a link is in
/// them, in `tone`, with the next of `atoms` in the place of each
/// [`ATOM`] and `tail` after them.
pub fn typed_with(
    text: &str,
    tail: &str,
    tone: Tone,
    atoms: Vec<Atom>,
) -> AnyView {
    let tail = Span {
        text: tail.to_string(),
        style: Style::default(),
        link: None,
    };
    let text = text.to_string();
    relinked(move || {
        let (text, tail, atoms) =
            (text.clone(), tail.clone(), atoms.clone());
        AnyView::new(move |cx: &mut Cx| {
            let mut spans = plain(&text, is_address_in(cx.world));
            spans.push(tail);
            let accent = cx.theme().tone(Tone::Accent);
            let mut atoms = atoms.into_iter();
            let mut runs = Vec::new();
            // The place each run leads to, in the order of the runs.
            let mut leads = Vec::new();
            let (mut at_byte, mut copies) = (0, AtomWords::default());
            let under = cx.parent();
            for piece in anchored(cx.world, under, spans) {
                let (span, anchor) = match piece {
                    Piece::Words(span, anchor) => (span, anchor),
                    // A chief's line is whole, so this is never one.
                    Piece::Address(span) => (span, None),
                    Piece::Chip(chip) => {
                        copies
                            .0
                            .push((at_byte, chip.text().to_string()));
                        runs.push(Run::atom(chip));
                        leads.push(None);
                        continue;
                    }
                };
                let pieces =
                    span.text.split(ATOM).collect::<Vec<_>>();
                let last = pieces.len() - 1;
                for (at, piece) in pieces.into_iter().enumerate() {
                    if !piece.is_empty() {
                        at_byte += piece.len();
                        let run = Run::new(piece);
                        runs.push(match anchor {
                            Some(_) => run
                                .color(accent)
                                .underline(LineColor::Ink),
                            None => run,
                        });
                        leads.push(anchor.clone());
                    }
                    if at < last
                        && let Some(atom) = atoms.next()
                    {
                        copies
                            .0
                            .push((at_byte, atom.text().to_string()));
                        runs.push(Run::atom(atom));
                        leads.push(None);
                    }
                }
            }
            let body = cx.build(
                bevy_fynix::views::rich(runs)
                    .tone(tone)
                    .atoms()
                    .direction(FlexDirection::Column)
                    .max_width(percent(100.0)),
            );
            let chips =
                copies.0.iter().map(|(_, words)| words.clone());
            seed(cx.world, body, chips.collect());
            if let Some(text) = cx
                .world
                .get::<Children>(body)
                .and_then(|kids| kids.first().copied())
            {
                cx.world.entity_mut(text).insert(copies);
            }
            for (kid, lead) in
                runs_of(cx.world, body).into_iter().zip(leads)
            {
                if let Some(anchor) = lead {
                    cx.world.entity_mut(kid).insert((
                        anchor,
                        EntityCursor(SystemCursorIcon::Pointer),
                    ));
                }
            }
            body
        })
    })
}

/// A Markdown document that is drawn once, one view per block. Its
/// words are selected as one.
pub fn fixed(text: &str, tone: Tone) -> impl View {
    let text = text.to_string();
    AnyView::new(move |cx: &mut Cx| {
        let blocks = parse(&text, is_address_in(cx.world));
        cx.build(drawn(blocks, tone, Setting::Running))
    })
}

/// A [`fixed`] document of words given to a model, set as a page: its
/// paths and addresses are code and its lines stand further apart.
pub fn page(text: &str, tone: Tone) -> impl View {
    let text = text.to_string();
    AnyView::new(move |cx: &mut Cx| {
        let blocks = parse(&text, is_address_in(cx.world));
        cx.build(drawn(paged(blocks), tone, Setting::Page))
    })
}

fn paged(mut blocks: Vec<Block>) -> Vec<Block> {
    for block in &mut blocks {
        let spans = match &mut block.kind {
            Kind::Paragraph(spans) | Kind::Heading(_, spans) => {
                spans.iter_mut().collect::<Vec<_>>()
            }
            Kind::Table { rows, .. } => {
                rows.iter_mut().flatten().flatten().collect()
            }
            _ => Vec::new(),
        };
        for span in spans {
            if matches!(
                span.link,
                Some(Target::Path(_) | Target::Agent(_))
            ) {
                span.link = None;
                span.style.code = true;
            }
        }
    }
    blocks
}

/// `blocks` drawn once, one view per block, set as `setting` says.
fn drawn(
    blocks: Vec<Block>,
    tone: Tone,
    setting: Setting,
) -> impl View {
    AnyView::new(move |cx: &mut Cx| {
        let views = blocks
            .iter()
            .enumerate()
            .map(|(at, block)| {
                block_view(cx, at, block, tone, setting)
            })
            .collect::<Vec<_>>();
        cx.build(column(views).gap(0.0).tagged(Area))
    })
}

#[cfg(test)]
mod tests {
    use bevy::text::TextLayoutInfo;
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use bevy_fynix::{FynixPlugin, mount};

    use super::*;
    use crate::testing::ui_app;
    use crate::theme::{self, HamletTheme};

    fn chip(
        _: &mut World,
        address: &str,
        words: &str,
        key: String,
    ) -> Option<Atom> {
        if words != address {
            return None;
        }
        Some(Atom::new::<HamletTheme>(key, words, || {
            row((label("Usability").size(12.0).wrap(false),))
                .padding(UiRect::axes(px(6.0), px(1.0)))
                .boxed()
        }))
    }

    #[test]
    fn the_words_in_a_tables_cells_wrap_inside_them() {
        let mut app = ui_app(1000, 1.0);
        app.add_plugins(FynixPlugin::<HamletTheme>::default())
            .insert_resource(AgentLinks {
                is_address: |word| word == "agent#2",
                resolve: chip,
                generation: 0,
            });
        theme::show(app.world_mut(), HamletTheme::designed());
        let text = "| Agent | Role |\n|---|---|\n| agent#2 (Hamlet \
                    Reorg Design) | Design for reorganizing \
                    hamlets, with words enough to wrap over two \
                    lines or more |\n| agent#2 (Stop and Errors) | \
                    Stop |";
        let table = mount::<HamletTheme>(
            app.world_mut(),
            fixed(text, Tone::Body),
        );
        let room = app
            .world_mut()
            .spawn(Node {
                width: px(320.0),
                ..Node::default()
            })
            .id();
        app.world_mut().entity_mut(table).insert(ChildOf(room));
        for _ in 0..4 {
            app.update();
        }

        let world = app.world_mut();
        let grid = world
            .query::<(Entity, &Node)>()
            .iter(world)
            .find(|(_, node)| node.display == Display::Grid)
            .map(|(grid, _)| grid)
            .unwrap();
        // The left of a node and the right of its content.
        let edges = |world: &World, node: Entity| {
            let size = world.get::<ComputedNode>(node).unwrap();
            let at = world.get::<UiGlobalTransform>(node).unwrap();
            let left = at.translation.x - size.size().x / 2.0;
            (left, left + size.size().x - size.padding.max_inset.x)
        };
        let texts = world
            .query::<(Entity, &TextLayoutInfo)>()
            .iter(world)
            .map(|(text, laid)| {
                let ends = laid.glyphs.iter().map(|glyph| {
                    glyph.position.x
                        + glyph.atlas_info.rect.width() / 2.0
                });
                (text, ends.fold(0.0, f32::max))
            })
            .collect::<Vec<_>>();
        let mut checked = 0;
        for (text, inked) in texts {
            let mut cell = text;
            while let Some(up) = world.get::<ChildOf>(cell)
                && up.parent() != grid
            {
                cell = up.parent();
            }
            if world.get::<ChildOf>(cell).is_none() {
                continue;
            }
            let (left, _) = edges(world, text);
            let (_, end) = edges(world, cell);
            assert!(
                left + inked <= end + 1.0,
                "words past their cell"
            );
            checked += 1;
        }
        assert!(checked >= 6);
        let wide = world.get::<ComputedNode>(grid).unwrap().size().x;
        assert!(wide <= 320.0, "a table {wide} wide");
        // The column of names wraps, so the wordier one has the room.
        let cells = world.get::<Children>(grid).unwrap();
        let width = |at: usize| {
            world.get::<ComputedNode>(cells[at]).unwrap().size().x
        };
        assert!(width(0) < width(1), "{} and {}", width(0), width(1));
    }
}
