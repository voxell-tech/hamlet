//! Markdown drawn: a view for each block.

use bevy::color::Color;
use bevy::ecs::entity::Entity;
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
use bevy_fynix::views::{FrameProps, column, frame, label, row};
use bevy_fynix::{EntityCursor, Signal, ViewExt, component, each};
use hamlet_core::session::{Entry, Transcript, reply_in};

use super::parse::{parse, plain};
use super::picture::picture_view;
use super::{Align, Block, Kind, Marker, Row, Span, Style, Target};
use crate::links::{Anchor, seek};
use crate::looks::{Fills, bare};
use crate::platform::{
    Display as DisplayFace, DisplayText, Mono, MonoText,
};
use crate::theme::{Palette, paint, themed};
use crate::{AnyView, Cx, View};

fn words(world: &World, session: Entity, id: u64) -> Option<&str> {
    match world.get::<Transcript>(session)?.get(id)? {
        Entry::Assistant(text) | Entry::Agent { text, .. } => {
            Some(text)
        }
        // An agent's reply, carried back to whoever wrote it.
        Entry::User { user, text } => reply_in(user, text),
        _ => None,
    }
}

/// True while the entry `id` is the last of its conversation and
/// may have more words to come.
fn growing(world: &World, session: Entity, id: u64) -> bool {
    world.get::<Transcript>(session).is_some_and(|transcript| {
        transcript.answering()
            && transcript.items().last().map(|item| item.id)
                == Some(id)
    })
}

/// Takes the link off a path that `blocks` end with, which may be
/// the start of a longer one.
fn unsettle(blocks: &mut [Block]) {
    let spans = match blocks.last_mut().map(|block| &mut block.kind) {
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

/// The blocks of an entry of a transcript, parsed again only when
/// that entry's own text has changed or has stopped growing.
pub(crate) fn entry(session: Entity, id: u64) -> Signal<Vec<Block>> {
    let mut moved = component::<Transcript, _>(session, |_| ());
    let mut seen = None::<(String, bool)>;
    Signal::new(
        move |world: &World| {
            let mut blocks = words(world, session, id)
                .map(parse)
                .unwrap_or_default();
            if growing(world, session, id) {
                unsettle(&mut blocks);
            }
            blocks
        },
        move |world: &World| {
            if !moved.changed(world) {
                return false;
            }
            let now = words(world, session, id);
            let grows = growing(world, session, id);
            if seen
                .as_ref()
                .map(|(text, grew)| (text.as_str(), *grew))
                == now.map(|text| (text, grows))
            {
                return false;
            }
            seen = now.map(|text| (text.to_string(), grows));
            true
        },
    )
}

/// The quote bars and list levels drawn, however deep a block sits.
const DEEPEST: u8 = 6;

/// The width of a list level, which a marker sits in.
const STEP: f32 = 22.0;

/// The face a block's text is set in.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Face {
    Body,
    Strong,
    Heading,
}

/// The size, the tone and the face of a block's text.
#[derive(Clone, Copy)]
pub(super) struct Ink {
    /// `None` for the theme's body size.
    pub size: Option<f32>,
    pub tone: Tone,
    pub face: Face,
}

/// `spans` with the place each link leads. A path that names no
/// file is no link, and its words join those beside it again.
fn anchored(
    world: &mut World,
    spans: Vec<Span>,
) -> Vec<(Span, Option<Anchor>)> {
    let mut all = Vec::<(Span, Option<Anchor>)>::new();
    for mut span in spans {
        let anchor = match span.link.take() {
            Some(Target::Web(address)) => Some(Anchor::Web(address)),
            Some(Target::Path(path)) => {
                seek(world, &path).map(Anchor::File)
            }
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
pub(super) fn rich(spans: &[Span], ink: Ink) -> AnyView {
    let spans = spans.to_vec();
    AnyView::new(move |cx: &mut Cx| {
        let spans = anchored(cx.world, spans);
        let theme = cx.theme();
        let size = ink.size.unwrap_or(theme.text.body);
        let root = cx.build(label("").size(size));
        let mono =
            cx.world.get_resource::<Mono>().map(|m| m.0.clone());
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
                font: face.map(FontSource::from).unwrap_or_default(),
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
                    TextBackgroundColor(theme.palette.plate),
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
}

fn code_view(lang: &str, text: &str) -> impl View {
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
        .fill(paint(|p| p.plate))
        .border(1.0)
        .border_color(paint(|p| p.brass_6))
        .radius(themed(|t| t.space.round))
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
fn table_view(align: &[Align], rows: &[Row], tone: Tone) -> AnyView {
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
                (Face::Strong, |p| p.plate)
            } else {
                (Face::Body, |p| p.black)
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
                    .fill(paint(fill))
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
                .fill(paint(|p| p.brass_6)),
        );
        if let Some(mut ui) = cx.world.get_mut::<Node>(node) {
            ui.display = Display::Grid;
            ui.grid_template_columns = shared(columns as u16);
        }
        node
    })
}

fn block_view(at: usize, block: &Block, tone: Tone) -> AnyView {
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
        Kind::Code { lang, text } => code_view(lang, text).boxed(),
        Kind::Rule => frame()
            .height(px(1.0))
            .margin(UiRect::vertical(px(4.0)))
            .fill(paint(|p| p.brass_6))
            .boxed(),
        Kind::Table { align, rows } => table_view(align, rows, tone),
        Kind::Picture { caption, target } => {
            picture_view(caption, target, body)
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
                .fill(paint(|p| p.brass_5))
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

/// A Markdown document, one view per block. A block that has not
/// changed keeps its nodes while the document grows.
pub(crate) fn document(
    blocks: Signal<Vec<Block>>,
    tone: Tone,
) -> impl View {
    each(
        blocks.map(|blocks| {
            blocks.into_iter().enumerate().collect::<Vec<_>>()
        }),
        |placed: &(usize, Block)| placed.clone(),
        move |(at, block)| block_view(*at, block, tone),
    )
    .within(bare())
}

/// Words set as they were typed, with `tail` after them, if there
/// may be a link in them.
pub(crate) fn typed(text: &str, tail: &str) -> Option<AnyView> {
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

/// A Markdown document that is drawn once, one view per block.
pub fn fixed(text: &str, tone: Tone) -> impl View {
    let blocks = parse(text)
        .iter()
        .enumerate()
        .map(|(at, block)| block_view(at, block, tone))
        .collect::<Vec<_>>();
    column(blocks).gap(0.0)
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::query::With;
    use bevy::time::TimePlugin;
    use bevy::ui::widget::Text;
    use bevy_fynix::{FynixPlugin, mount};
    use hamlet_assets::Tuning;
    use hamlet_core::agent::Event as AgentEvent;
    use hamlet_core::session::Reply;
    use serde_json::json;

    use super::super::parse::tests::{plain, top};
    use super::*;
    use crate::chat;
    use crate::links::Roots;
    use crate::me::Me;
    use crate::theme::{self, HamletTheme};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<HamletTheme>::default(),
        ));
        theme::insert_theme(app.world_mut(), HamletTheme::designed());
        app.update();
        app
    }

    /// A session where each of `replies` answers a message.
    fn talk(app: &mut App, replies: &[&str]) -> (Entity, Vec<u64>) {
        let log = replies
            .iter()
            .flat_map(|reply| {
                [
                    json!({"type": "user", "user": "a", "content": "go"}),
                    json!({"type": "assistant", "message": {"content": reply}}),
                ]
            })
            .collect::<Vec<_>>();
        let transcript = Transcript::from_log(&log);
        let ids = transcript.ids();
        (app.world_mut().spawn(transcript).id(), ids)
    }

    fn grow(app: &mut App, session: Entity, more: &str) {
        app.world_mut()
            .get_mut::<Transcript>(session)
            .unwrap()
            .apply(&Reply::Event(AgentEvent::Text(more.into())));
    }

    /// Every text span in the world, with what it says.
    fn spans(app: &mut App) -> Vec<(Entity, String)> {
        let world = app.world_mut();
        world
            .query::<(Entity, &TextSpan)>()
            .iter(world)
            .map(|(entity, span)| (entity, span.0.clone()))
            .collect()
    }

    fn span(app: &mut App, text: &str) -> Entity {
        spans(app)
            .into_iter()
            .find(|(_, said)| said == text)
            .unwrap_or_else(|| panic!("no span says {text:?}"))
            .0
    }

    #[test]
    fn a_reply_is_built_as_styled_nodes() {
        let mut app = app();
        let (session, ids) = talk(
            &mut app,
            &["# Plan\n\nUse **care** with `rm` and \
               [docs](http://x).\n\n```sh\nls -la\n```\n\n| a | b \
               |\n|---|---|\n| 1 | 2 |\n"],
        );
        mount::<HamletTheme>(
            app.world_mut(),
            document(entry(session, ids[1]), Tone::Body),
        );
        app.update();

        let world = app.world_mut();
        let code = world
            .query_filtered::<&Text, With<MonoText>>()
            .iter(world)
            .map(|text| text.0.clone())
            .collect::<Vec<_>>();
        assert_eq!(code, ["ls -la"]);
        let grids = world
            .query::<&Node>()
            .iter(world)
            .filter(|node| node.display == Display::Grid)
            .map(|node| node.grid_template_columns.clone())
            .collect::<Vec<_>>();
        assert_eq!(grids, [shared(2)]);

        let title = span(&mut app, "Plan");
        let care = span(&mut app, "care");
        let rm = span(&mut app, "rm");
        let docs = span(&mut app, "docs");
        let world = app.world();
        assert!(world.get::<DisplayText>(title).is_some());
        let font = |e| world.get::<TextFont>(e).unwrap();
        assert_eq!(font(title).font_size, FontSize::Px(20.0));
        assert_eq!(font(care).weight, FontWeight::BOLD);
        assert!(world.get::<MonoText>(rm).is_some());
        assert!(world.get::<TextBackgroundColor>(rm).is_some());
        assert!(world.get::<Underline>(docs).is_some());
        assert_eq!(
            world.get::<TextColor>(docs).unwrap().0,
            HamletTheme::designed().tone(Tone::Accent)
        );
    }

    #[test]
    fn a_growing_reply_rebuilds_only_its_last_block() {
        let mut app = app();
        let (session, _) =
            talk(&mut app, &["An *old* reply.", "First.\n\nSec"]);
        app.insert_resource(Me::of(
            session,
            Entity::PLACEHOLDER,
            "a",
            None,
        ))
        .init_resource::<chat::Unfolded>()
        .init_resource::<chat::Lines>()
        .init_resource::<chat::Reach>()
        .add_systems(bevy::app::Update, chat::fold_lines);
        mount::<HamletTheme>(app.world_mut(), chat::transcript());
        app.update();
        app.update();
        let before = spans(&mut app);
        let last = span(&mut app, "Sec");

        grow(&mut app, session, "ond *grows*");
        app.update();
        let after = spans(&mut app);
        for (entity, said) in &before {
            assert_eq!(
                after.contains(&(*entity, said.clone())),
                *entity != last,
                "{said:?}"
            );
        }
        span(&mut app, "Second ");
        span(&mut app, "grows");

        grow(&mut app, session, "\n\n```\nlet x");
        app.update();
        let grown = spans(&mut app);
        assert!(after.iter().all(|span| grown.contains(span)));
    }

    #[test]
    fn a_path_is_a_link_where_its_file_is_and_its_word_is_whole() {
        let folder = hamlet_testing::Scratch::new("markdown-links");
        std::fs::write(folder.join("plan.md"), "").unwrap();
        let mut app = app();
        app.insert_resource(Roots::new([folder.to_path_buf()]));
        let (session, ids) = talk(&mut app, &[]);
        assert!(ids.is_empty());
        let mut transcript =
            app.world_mut().get_mut::<Transcript>(session).unwrap();
        for event in [
            AgentEvent::Question {
                from: "a".into(),
                text: "go".into(),
                at: 0,
            },
            AgentEvent::Text(
                "Read plan.md and gone.md, then plan.md".into(),
            ),
        ] {
            transcript.apply(&Reply::Event(event));
        }
        mount::<HamletTheme>(
            app.world_mut(),
            document(entry(session, 1), Tone::Body),
        );
        app.update();
        let said = |app: &mut App| {
            let mut spans = spans(app)
                .into_iter()
                .map(|(span, text)| {
                    (text, app.world().get::<Anchor>(span).cloned())
                })
                .collect::<Vec<_>>();
            spans.sort();
            spans
        };
        let file = Some(Anchor::File(folder.join("plan.md")));
        let no = |text: &str| (text.to_string(), None);
        assert_eq!(
            said(&mut app),
            [
                no(" and gone.md, then plan.md"),
                no("Read "),
                ("plan.md".to_string(), file.clone()),
            ]
        );

        app.world_mut()
            .get_mut::<Transcript>(session)
            .unwrap()
            .apply(&Reply::Done(String::new()));
        app.update();
        assert_eq!(
            said(&mut app),
            [
                no(" and gone.md, then "),
                no("Read "),
                ("plan.md".to_string(), file.clone()),
                ("plan.md".to_string(), file),
            ]
        );
    }

    #[test]
    fn only_the_entry_that_changed_is_read_again() {
        let mut app = app();
        let (session, ids) = talk(&mut app, &["old", "new"]);
        let mut old = entry(session, ids[1]);
        let mut new = entry(session, ids[3]);
        assert!(old.changed(app.world()));
        assert!(new.changed(app.world()));

        grow(&mut app, session, " words");
        assert!(!old.changed(app.world()));
        assert!(new.changed(app.world()));
        assert_eq!(
            new.get(app.world()),
            [top(Kind::Paragraph(vec![plain("new words")]))]
        );
    }
}
