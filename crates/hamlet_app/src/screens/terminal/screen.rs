//! The screen of the active terminal: a row of styled text per line,
//! the colours behind it and the cursor, laid out by the size of one
//! cell, and the size the terminal is given from the room it has.

use bevy::color::{Alpha, Color};
use bevy::ecs::change_detection::{DetectChanges, Tick};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::math::Vec2;
use bevy::picking::Pickable;
use bevy::text::{
    FontFeatureTag, FontFeatures, FontSize, FontStyle, FontWeight,
    LineHeight, TextColor, TextFont, TextLayout, TextLayoutInfo,
    TextSpan, Underline,
};
use bevy::time::Time;
use bevy::ui::widget::Text;
use bevy::ui::{
    BackgroundColor, BorderColor, ComputedNode, Display, Node,
    PositionType, UiRect, Val, ZIndex, percent, px,
};
use hamlet_client::terminals::{
    self, Terminal, TerminalKind, TerminalScreen,
};
use hamlet_ui::theme::{HamletTheme, TerminalLook};
use hamlet_ui::widgets::platform::TerminalFace;

use super::Active;

/// The letters the width of a cell is measured over.
const PROBE: &str = "0123456789012345678901234567890123456789";

/// The node a terminal's screen is drawn in. A press gives it the
/// keys only with a [`TabIndex`]: without one, the press's
/// `AcquireFocus` reaches the window, which takes them back. A
/// negative one keeps it out of the Tab order, so Tab reaches the
/// shell.
#[derive(Component)]
#[require(TabIndex = TabIndex(-1))]
pub struct Canvas;

/// What is drawn in a [`Canvas`].
#[derive(Component)]
pub(crate) struct Drawn {
    terminal: Option<Entity>,
    rows: Vec<Entity>,
    lines: Vec<Option<Line>>,
    cursor: Entity,
    probe: Entity,
    /// The size of a cell, in logical pixels.
    pub(crate) cell: Vec2,
    pub(crate) margin: f32,
    /// What the rows were drawn from.
    seen: Option<Seen>,
    /// A size the terminal waits to be given, since when.
    sizing: Option<((u16, u16), f32)>,
}

#[derive(Clone, PartialEq)]
struct Seen {
    tick: Tick,
    focused: bool,
    chosen: Option<Span>,
    cell: Vec2,
}

/// The cells chosen with the pointer, from where the press was to
/// where the pointer is, by row and column of what is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Span {
    pub(crate) from: (u16, u16),
    pub(crate) to: (u16, u16),
}

impl Span {
    /// The first and the last cell, in reading order.
    fn ends(self) -> ((u16, u16), (u16, u16)) {
        if self.from <= self.to {
            (self.from, self.to)
        } else {
            (self.to, self.from)
        }
    }

    /// The columns chosen on `row`, the end excluded.
    fn columns(self, row: u16, cols: u16) -> Option<(u16, u16)> {
        let ((first_row, first), (last_row, last)) = self.ends();
        if row < first_row || row > last_row {
            return None;
        }
        let start = if row == first_row { first } else { 0 };
        let end = if row == last_row { last + 1 } else { cols };
        (start < end).then_some((start, end.min(cols)))
    }

    /// The text of the chosen cells on `screen`.
    pub(crate) fn text(self, screen: &vt100::Screen) -> String {
        let ((first_row, first), (last_row, last)) = self.ends();
        screen.contents_between(first_row, first, last_row, last + 1)
    }
}

/// The cells chosen in the active terminal, and the cell a drag that
/// chooses them starts from.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub(crate) struct Chosen(
    pub(crate) Option<Span>,
    pub(crate) Option<(u16, u16)>,
);

/// The columns and rows the screen has room for, for a terminal
/// still to be opened.
#[derive(Resource, Default, Clone, Copy)]
pub(crate) struct Room(pub(crate) Option<(u16, u16)>);

/// One row of the screen as it is drawn.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Line {
    runs: Vec<Run>,
    backs: Vec<Back>,
}

/// Letters of one style, one after another.
#[derive(Clone, PartialEq, Debug)]
struct Run {
    text: String,
    fg: Color,
    bold: bool,
    italic: bool,
    underline: bool,
}

/// A colour behind `cols` cells from `col`.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Back {
    col: u16,
    cols: u16,
    color: Color,
}

/// The style of one cell.
#[derive(Clone, Copy, PartialEq)]
struct Style {
    fg: Color,
    bg: Option<Color>,
    bold: bool,
    italic: bool,
    underline: bool,
}

/// The colour a program names, `default` for none. A bold one of the
/// first eight is drawn bright.
fn colour(
    named: vt100::Color,
    default: Color,
    bright: bool,
    look: &TerminalLook,
) -> Color {
    match named {
        vt100::Color::Default => default,
        vt100::Color::Idx(index) if index < 8 && bright => {
            look.ansi(index + 8)
        }
        vt100::Color::Idx(index) if index < 16 => look.ansi(index),
        vt100::Color::Idx(index) if index < 232 => {
            let level =
                |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
            let at = index - 16;
            Color::srgb_u8(
                level(at / 36),
                level(at / 6 % 6),
                level(at % 6),
            )
        }
        vt100::Color::Idx(index) => {
            let grey = 8 + (index - 232) * 10;
            Color::srgb_u8(grey, grey, grey)
        }
        vt100::Color::Rgb(r, g, b) => Color::srgb_u8(r, g, b),
    }
}

fn style(cell: &vt100::Cell, look: &TerminalLook) -> Style {
    let fg =
        colour(cell.fgcolor(), look.foreground, cell.bold(), look);
    let bg = match cell.bgcolor() {
        vt100::Color::Default => None,
        named => Some(colour(named, look.background, false, look)),
    };
    let (fg, bg) = if cell.inverse() {
        (bg.unwrap_or(look.background), Some(fg))
    } else {
        (fg, bg)
    };
    let fg = if cell.dim() {
        fg.with_alpha(fg.alpha() * 0.6)
    } else {
        fg
    };
    Style {
        fg,
        bg,
        bold: cell.bold(),
        italic: cell.italic(),
        underline: cell.underline(),
    }
}

/// Row `row` of `screen`, `cols` wide, with the block of a cursor at
/// `cursor` and `chosen` columns lit.
pub(crate) fn line(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    cursor: Option<u16>,
    chosen: Option<(u16, u16)>,
    look: &TerminalLook,
) -> Line {
    let mut line = Line::default();
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let mut style = style(cell, look);
        if cursor == Some(col) {
            style.fg = look.background;
            style.bg = Some(look.cursor);
        }
        let width = if cell.is_wide() { 2 } else { 1 };
        if let Some(color) = style.bg {
            match line.backs.last_mut() {
                Some(back)
                    if back.color == color
                        && back.col + back.cols == col =>
                {
                    back.cols += width;
                }
                _ => line.backs.push(Back {
                    col,
                    cols: width,
                    color,
                }),
            }
        }
        let text = if cell.has_contents() {
            cell.contents()
        } else {
            " "
        };
        match line.runs.last_mut() {
            Some(run)
                if run.fg == style.fg
                    && run.bold == style.bold
                    && run.italic == style.italic
                    && run.underline == style.underline =>
            {
                run.text.push_str(text);
            }
            _ => line.runs.push(Run {
                text: text.to_owned(),
                fg: style.fg,
                bold: style.bold,
                italic: style.italic,
                underline: style.underline,
            }),
        }
    }
    // Blanks at the end draw nothing.
    while let Some(run) = line.runs.last_mut() {
        if run.underline {
            break;
        }
        let kept = run.text.trim_end_matches(' ').len();
        run.text.truncate(kept);
        if !run.text.is_empty() {
            break;
        }
        line.runs.pop();
    }
    if let Some((start, end)) = chosen {
        line.backs.push(Back {
            col: start,
            cols: end - start,
            color: look.selection,
        });
    }
    line
}

/// Draws the active terminal in every canvas.
pub(crate) fn draw(world: &mut World) {
    let Some(canvases) = world
        .try_query_filtered::<Entity, With<Canvas>>()
        .map(|mut query| query.iter(world).collect::<Vec<_>>())
    else {
        return;
    };
    let Some(look) = world
        .get_resource::<bevy_fynix::Theme<HamletTheme>>()
        .map(|theme| theme.0.terminal.clone())
    else {
        return;
    };
    for canvas in canvases {
        draw_in(world, canvas, &look);
    }
}

fn draw_in(world: &mut World, canvas: Entity, look: &TerminalLook) {
    let Some(face) = world
        .get_resource::<TerminalFace>()
        .map(|face| face.0.clone())
    else {
        return;
    };
    let font = TextFont {
        font: face.into(),
        font_size: FontSize::Px(look.size),
        // Each letter keeps its own cell.
        font_features: FontFeatures::builder()
            .set(FontFeatureTag::STANDARD_LIGATURES, 0)
            .set(FontFeatureTag::CONTEXTUAL_ALTERNATES, 0)
            .build(),
        ..TextFont::default()
    };
    let height = (look.size * look.line).round();
    if world.get::<Drawn>(canvas).is_none() {
        let probe = world
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::Flex,
                    ..Node::default()
                },
                Text::new(PROBE),
                font.clone(),
                TextLayout::no_wrap(),
                TextColor(Color::NONE),
                Pickable::IGNORE,
                ChildOf(canvas),
            ))
            .id();
        let cursor = world
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    border: UiRect::all(px(1.0)),
                    display: Display::None,
                    ..Node::default()
                },
                BorderColor::all(look.cursor),
                // Over the rows.
                ZIndex(1),
                Pickable::IGNORE,
                ChildOf(canvas),
            ))
            .id();
        world.entity_mut(canvas).insert(Drawn {
            terminal: None,
            rows: Vec::new(),
            lines: Vec::new(),
            cursor,
            probe,
            cell: Vec2::new(look.size * 0.6, height),
            margin: look.margin,
            seen: None,
            sizing: None,
        });
    }
    let Some(probe) = world.get::<Drawn>(canvas).map(|d| d.probe)
    else {
        return;
    };
    let measured = world
        .get::<TextLayoutInfo>(probe)
        .filter(|info| info.size.x > 0.0 && info.scale_factor > 0.0)
        .map(|info| {
            info.size.x / info.scale_factor / PROBE.len() as f32
        });
    let active = world
        .get_resource::<Active>()
        .and_then(|active| active.0)
        .filter(|terminal| {
            world.get::<TerminalScreen>(*terminal).is_some()
        });
    let focused =
        world.get_resource::<InputFocus>().and_then(InputFocus::get)
            == Some(canvas);
    let chosen = world.get_resource::<Chosen>().and_then(|c| c.0);
    let mut drawn = world.entity_mut(canvas).take::<Drawn>();
    let Some(drawn_now) = drawn.as_mut() else {
        return;
    };
    if let Some(width) = measured {
        drawn_now.cell = Vec2::new(width, height);
    }
    if drawn_now.terminal != active {
        drawn_now.terminal = active;
        drawn_now.seen = None;
        drawn_now.lines.iter_mut().for_each(|line| *line = None);
        if world.get_resource::<Chosen>().is_some() {
            world.resource_mut::<Chosen>().0 = None;
        }
    }
    redraw(world, canvas, drawn_now, focused, chosen, &font, look);
    if let Some(drawn) = drawn {
        world.entity_mut(canvas).insert(drawn);
    }
}

fn redraw(
    world: &mut World,
    canvas: Entity,
    drawn: &mut Drawn,
    focused: bool,
    chosen: Option<Span>,
    font: &TextFont,
    look: &TerminalLook,
) {
    let Some(terminal) = drawn.terminal else {
        for row in drawn.rows.drain(..) {
            world.despawn(row);
        }
        drawn.lines.clear();
        if let Some(mut node) = world.get_mut::<Node>(drawn.cursor) {
            node.display = Display::None;
        }
        return;
    };
    let read_only = world
        .get::<Terminal>(terminal)
        .is_some_and(|view| view.0.kind == TerminalKind::Runs);
    let Some(screen) =
        world.entity(terminal).get_ref::<TerminalScreen>()
    else {
        return;
    };
    let seen = Seen {
        tick: screen.last_changed(),
        focused,
        chosen,
        cell: drawn.cell,
    };
    if drawn.seen.as_ref() == Some(&seen) {
        return;
    }
    let moved =
        drawn.seen.as_ref().is_none_or(|was| was.cell != seen.cell);
    drawn.seen = Some(seen);
    let shown = screen.screen();
    let (rows, cols) = shown.size();
    let (rows, cols) = (rows.get(), cols.get());
    let (cursor_row, cursor_col) = shown.cursor_position();
    let cursor_seen =
        !shown.hide_cursor() && shown.scrollback() == 0 && !read_only;
    let lines = (0..rows)
        .map(|row| {
            let block = (focused && cursor_seen && row == cursor_row)
                .then_some(cursor_col);
            let lit = chosen.and_then(|span| span.columns(row, cols));
            line(shown, row, cols, block, lit, look)
        })
        .collect::<Vec<_>>();
    let cell = drawn.cell;
    let margin = drawn.margin;
    let place = |at: usize| Node {
        position_type: PositionType::Absolute,
        left: px(margin),
        top: px(margin + at as f32 * cell.y),
        width: percent(100.0),
        height: px(cell.y),
        ..Node::default()
    };
    while drawn.rows.len() > lines.len() {
        if let Some(row) = drawn.rows.pop() {
            world.despawn(row);
        }
        drawn.lines.pop();
    }
    while drawn.rows.len() < lines.len() {
        // A row added as the screen grows is placed now, since the
        // others are placed again only when the cell changes.
        let row = world
            .spawn((
                place(drawn.rows.len()),
                Pickable::IGNORE,
                ChildOf(canvas),
            ))
            .id();
        drawn.rows.push(row);
        drawn.lines.push(None);
    }
    for (at, line) in lines.into_iter().enumerate() {
        let row = drawn.rows[at];
        if moved && let Some(mut node) = world.get_mut::<Node>(row) {
            *node = place(at);
        }
        if drawn.lines[at].as_ref() == Some(&line) {
            continue;
        }
        draw_row(world, row, &line, cell, font);
        drawn.lines[at] = Some(line);
    }
    let outline = !focused && cursor_seen;
    if let Some(mut node) = world.get_mut::<Node>(drawn.cursor) {
        node.display = if outline {
            Display::Flex
        } else {
            Display::None
        };
        node.left = px(drawn.margin + f32::from(cursor_col) * cell.x);
        node.top = px(drawn.margin + f32::from(cursor_row) * cell.y);
        node.width = px(cell.x);
        node.height = px(cell.y);
    }
    if let Some(mut border) =
        world.get_mut::<BorderColor>(drawn.cursor)
    {
        *border = BorderColor::all(look.cursor);
    }
}

fn draw_row(
    world: &mut World,
    row: Entity,
    line: &Line,
    cell: Vec2,
    font: &TextFont,
) {
    world.entity_mut(row).despawn_related::<Children>();
    for back in &line.backs {
        world.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(f32::from(back.col) * cell.x),
                top: Val::Px(0.0),
                width: px(f32::from(back.cols) * cell.x),
                height: percent(100.0),
                ..Node::default()
            },
            BackgroundColor(back.color),
            Pickable::IGNORE,
            ChildOf(row),
        ));
    }
    if line.runs.is_empty() {
        return;
    }
    let text = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(0.0),
                ..Node::default()
            },
            Text::new(""),
            font.clone(),
            LineHeight::Px(cell.y),
            TextLayout::no_wrap(),
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    for run in &line.runs {
        let mut span = world.spawn((
            TextSpan::new(run.text.clone()),
            TextFont {
                weight: if run.bold {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
                style: if run.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
                ..font.clone()
            },
            LineHeight::Px(cell.y),
            TextColor(run.fg),
            ChildOf(text),
        ));
        if run.underline {
            span.insert(Underline);
        }
    }
}

/// Gives the active terminal the columns and rows its canvas has room
/// for, once the canvas has held that size a moment.
pub(crate) fn fit(world: &mut World) {
    let Some(settle) = world
        .get_resource::<bevy_fynix::Theme<HamletTheme>>()
        .map(|theme| theme.0.terminal.settle)
    else {
        return;
    };
    let now =
        world.get_resource::<Time>().map_or(0.0, Time::elapsed_secs);
    let Some(canvases) = world
        .try_query_filtered::<Entity, (With<Canvas>, With<Drawn>)>()
        .map(|mut query| query.iter(world).collect::<Vec<_>>())
    else {
        return;
    };
    for canvas in canvases {
        let Some(room) = room(world, canvas) else {
            continue;
        };
        world.insert_resource(Room(Some(room)));
        let Some(terminal) =
            world.get::<Drawn>(canvas).and_then(|d| d.terminal)
        else {
            continue;
        };
        let ended = world
            .get::<Terminal>(terminal)
            .is_none_or(|view| view.0.exit.is_some());
        let size =
            world.get::<TerminalScreen>(terminal).map(|screen| {
                let (rows, cols) = screen.screen().size();
                (cols.get(), rows.get())
            });
        let Some(mut drawn) = world.get_mut::<Drawn>(canvas) else {
            continue;
        };
        if ended || size == Some(room) {
            drawn.sizing = None;
            continue;
        }
        match drawn.sizing {
            Some((wanted, since)) if wanted == room => {
                if now - since >= settle {
                    drawn.sizing = None;
                    terminals::resize(
                        world, terminal, room.0, room.1,
                    );
                }
            }
            _ => drawn.sizing = Some((room, now)),
        }
    }
}

/// The columns and rows `canvas` has room for.
fn room(world: &World, canvas: Entity) -> Option<(u16, u16)> {
    let drawn = world.get::<Drawn>(canvas)?;
    let node = world.get::<ComputedNode>(canvas)?;
    let size = node.size() * node.inverse_scale_factor()
        - Vec2::splat(2.0 * drawn.margin);
    let cols = (size.x / drawn.cell.x).floor();
    let rows = (size.y / drawn.cell.y).floor();
    (cols >= 2.0 && rows >= 1.0).then(|| {
        let most = f32::from(terminals::TERMINAL_SIZE);
        (cols.min(most) as u16, rows.min(most) as u16)
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use hamlet_assets::Tuning;

    use super::*;

    fn look() -> TerminalLook {
        HamletTheme::designed().terminal
    }

    fn screen(bytes: &str) -> vt100::Parser {
        let mut parser = vt100::Parser::new(
            NonZeroU16::new(2).unwrap(),
            NonZeroU16::new(20).unwrap(),
            0,
        );
        parser.process(bytes.as_bytes());
        parser
    }

    #[test]
    fn a_row_merges_cells_of_one_style_and_puts_colours_behind_them()
    {
        let look = look();
        let parser =
            screen("\x1b[31mab\x1b[1mc\x1b[0m \x1b[7mx\x1b[0m   ");
        let line = line(parser.screen(), 0, 20, None, None, &look);
        let texts = line
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, ["ab", "c", " ", "x"]);
        // Bold red is drawn bright.
        assert_eq!(line.runs[0].fg, look.red);
        assert_eq!(line.runs[1].fg, look.bright_red);
        // Inverse puts the plain colours the other way round.
        assert_eq!(line.runs[3].fg, look.background);
        assert_eq!(
            line.backs,
            [Back {
                col: 4,
                cols: 1,
                color: look.foreground
            }]
        );
    }

    #[test]
    fn the_cursor_and_the_chosen_cells_light_their_columns() {
        let look = look();
        let parser = screen("abcdef");
        let line = line(
            parser.screen(),
            0,
            20,
            Some(2),
            Some((1, 4)),
            &look,
        );
        let back = |col, cols, color| Back { col, cols, color };
        assert_eq!(
            line.backs,
            [back(2, 1, look.cursor), back(1, 3, look.selection)]
        );
        let span = Span {
            from: (1, 3),
            to: (0, 5),
        };
        assert_eq!(span.columns(0, 20), Some((5, 20)));
        assert_eq!(span.columns(1, 20), Some((0, 4)));
        assert_eq!(span.columns(2, 20), None);
    }

    #[test]
    fn the_256_colours_are_the_cube_and_the_greys() {
        let look = look();
        let at = |index| {
            colour(
                vt100::Color::Idx(index),
                Color::NONE,
                false,
                &look,
            )
        };
        assert_eq!(at(16), Color::srgb_u8(0, 0, 0));
        assert_eq!(at(196), Color::srgb_u8(255, 0, 0));
        assert_eq!(at(110), Color::srgb_u8(135, 175, 215));
        assert_eq!(at(232), Color::srgb_u8(8, 8, 8));
        assert_eq!(at(255), Color::srgb_u8(238, 238, 238));
    }
}
