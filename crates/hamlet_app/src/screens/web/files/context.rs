//! Add to chat: what the Files panel's page hands the window, a
//! file's lines or a box on what a viewer shows, chosen again in a
//! dialog over the panel and put in the chat's field for the chief to
//! send.
//!
//! The page (the fork's `hamlet.js`) emits `{hamletContext}` with the
//! lines and the file's text from a file shown as text, or, for any
//! other (a PDF, an image, a sheet, slides), with the part of the
//! page its viewer fills, which the window captures as it draws it.

use std::io::Cursor;

use bevy::app::{App, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::math::{Rect, UVec2, Vec2};
use bevy::picking::Pickable;
use bevy::picking::events::{
    PointerDrag, PointerDragEnter, PointerPress,
};
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use bevy::render::view::screenshot::{
    Screenshot, ScreenshotCaptured,
};
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BackgroundColor, BoxShadow, Display, JustifyContent,
    Node, Overflow, PositionType, UiRect, Val, px,
};
use bevy::window::{PrimaryWindow, Window};
use bevy_cef::prelude::Receive;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, overlay,
    row, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_ui::styles::{
    Fills, ICON, close_button, link_row, note, note_rules,
    primary_button,
};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::{DisplayText, MonoText};
use hamlet_ui::{AnyView, View, with_cx};
use image::{ImageFormat, RgbaImage};
use serde::Deserialize;

use super::super::page::Page;
use super::id;
use crate::convo::Convo;
use crate::dock::{HUB, show_tab};
use crate::menu::rect_of;
use crate::screens::chat;

/// The width of the dialog's side that shows what is sent, at most
/// and at least, and of the panel's width.
const SIDE: (f32, f32, f32) = (340.0, 240.0, 0.36);

/// The room around the dialog in the panel, and inside it.
const MARGIN: f32 = 28.0;
const PADDING: f32 = 20.0;

/// The dialog's head.
const HEAD: f32 = 52.0;

/// The lines shown before a choice, and after it.
const BEFORE: usize = 40;
const AFTER: usize = 120;

/// A choice of lines, in the amber of the lamp.
const CHOSEN: Color = Color::srgba(1.0, 0.745, 0.384, 0.16);

/// The edge of a box drawn on a page.
const EDGE: Color = Color::srgb(1.0, 0.745, 0.384);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Adding>()
        .add_observer(handed)
        .add_observer(press_line)
        .add_observer(drag_over_line)
        .add_observer(press_picture)
        .add_observer(drag_on_picture)
        .add_systems(Update, (paint_lines, place_box));
}

/// What the page hands over.
#[derive(Deserialize, Debug)]
struct Handed {
    #[serde(rename = "hamletContext")]
    context: Ask,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Ask {
    /// Lines `start` to `end`, from 1, of the file at `path` in the
    /// folder, whose words are `text`.
    Lines {
        path: String,
        start: usize,
        end: usize,
        #[serde(default)]
        text: String,
    },
    /// The part of the page a viewer of the file at `path` fills, in
    /// the page's pixels.
    Region {
        path: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

/// What the dialog shows, while it is open.
#[derive(Resource, Default)]
struct Adding {
    shown: Option<Shown>,
    /// Counts what was shown, so the dialog is built again for each.
    generation: u64,
}

enum Shown {
    Lines(Lines),
    Picture(Picture),
}

struct Lines {
    path: String,
    /// The file's lines.
    lines: Vec<String>,
    /// The lines chosen, from 1, first to last.
    start: usize,
    end: usize,
    /// Where a drag over the lines began.
    anchor: usize,
}

struct Picture {
    path: String,
    image: Handle<Image>,
    /// The picture's pixels, RGBA, and its size.
    pixels: Vec<u8>,
    size: UVec2,
    /// How much smaller the dialog shows it.
    scale: f32,
    /// The box drawn on it, in its pixels.
    chosen: Option<Rect>,
    anchor: Vec2,
}

/// A line of the file in the dialog, by its number from 1.
#[derive(Component)]
struct LineRow(usize);

/// The picture in the dialog that a box is drawn on.
#[derive(Component)]
struct Drawn;

/// The box drawn on the picture.
#[derive(Component)]
struct ChosenBox;

/// The width of the side of what is sent, in a panel `wide`.
fn side(wide: f32) -> f32 {
    let (most, least, share) = SIDE;
    (wide * share).clamp(least, most)
}

fn open(world: &mut World, shown: Shown) {
    let mut adding = world.resource_mut::<Adding>();
    adding.shown = Some(shown);
    adding.generation += 1;
}

fn close(world: &mut World) {
    let mut adding = world.resource_mut::<Adding>();
    adding.shown = None;
    adding.generation += 1;
}

/// Opens the dialog on what the Files panel's page handed over.
fn handed(
    emitted: On<Receive<serde_json::Value>>,
    mut commands: Commands,
) {
    let Ok(handed) =
        serde_json::from_value::<Handed>(emitted.payload.clone())
    else {
        return;
    };
    let webview = emitted.webview;
    let ask = handed.context;
    commands.queue(move |world: &mut World| {
        let ours = world
            .get::<Page>(webview)
            .is_some_and(|page| page.0 == id());
        if !ours {
            return;
        }
        match ask {
            Ask::Lines {
                path,
                start,
                end,
                text,
            } => {
                let lines = text
                    .lines()
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                if lines.is_empty() {
                    return;
                }
                let start = start.clamp(1, lines.len());
                let end = end.clamp(start, lines.len());
                open(
                    world,
                    Shown::Lines(Lines {
                        path,
                        lines,
                        start,
                        end,
                        anchor: start,
                    }),
                );
            }
            Ask::Region {
                path,
                x,
                y,
                width,
                height,
            } => {
                let part = Rect::new(x, y, x + width, y + height);
                capture(world, webview, path, part);
            }
        }
    });
}

/// Captures the window as it draws the page, to keep `part` of the
/// page, in the page's pixels.
fn capture(
    world: &mut World,
    webview: Entity,
    path: String,
    part: Rect,
) {
    let Some(page) = rect_of(world, webview) else {
        return;
    };
    let region =
        Rect::from_corners(page.min + part.min, page.min + part.max)
            .intersect(page);
    // The dialog's side the picture is shown in, inside the panel.
    let room = Vec2::new(
        page.width()
            - 2.0 * MARGIN
            - side(page.width())
            - 3.0 * PADDING,
        page.height() - 2.0 * MARGIN - HEAD - PADDING - 40.0,
    )
    .max(Vec2::splat(120.0));
    world.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>,
              mut commands: Commands| {
            let image = captured.image.clone();
            let path = path.clone();
            commands.queue(move |world: &mut World| {
                captured_region(world, path, region, room, &image);
            });
        },
    );
}

fn captured_region(
    world: &mut World,
    path: String,
    region: Rect,
    room: Vec2,
    window: &Image,
) {
    let scale = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map_or(1.0, Window::scale_factor);
    let Some((pixels, size)) = cut(window, region, scale) else {
        return;
    };
    let shown = (room.x / size.x as f32)
        .min(room.y / size.y as f32)
        .min(1.0);
    let image = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    let image = world.resource_mut::<Assets<Image>>().add(image);
    open(
        world,
        Shown::Picture(Picture {
            path,
            image,
            pixels,
            size,
            scale: shown,
            chosen: None,
            anchor: Vec2::ZERO,
        }),
    );
}

/// The RGBA pixels of `region`, in logical pixels, of a capture of
/// the window drawn at `scale`, and their size.
fn cut(
    window: &Image,
    region: Rect,
    scale: f32,
) -> Option<(Vec<u8>, UVec2)> {
    let data = window.data.as_ref()?;
    let full = UVec2::new(window.width(), window.height());
    let bgra = matches!(
        window.texture_descriptor.format,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
    );
    let at = |value: f32, most: u32| {
        ((value * scale).round().max(0.0) as u32).min(most)
    };
    let (left, top) =
        (at(region.min.x, full.x), at(region.min.y, full.y));
    let (right, bottom) =
        (at(region.max.x, full.x), at(region.max.y, full.y));
    if right <= left || bottom <= top {
        return None;
    }
    let row = full.x as usize * 4;
    if data.len() < row * full.y as usize {
        return None;
    }
    let mut pixels = Vec::with_capacity(
        ((right - left) * (bottom - top) * 4) as usize,
    );
    for y in top..bottom {
        let start = y as usize * row + left as usize * 4;
        let end = y as usize * row + right as usize * 4;
        for [b, g, r, _] in data[start..end].as_chunks::<4>().0 {
            match bgra {
                true => {
                    pixels.extend([*r, *g, *b, 255]);
                }
                false => {
                    pixels.extend([*b, *g, *r, 255]);
                }
            }
        }
    }
    Some((pixels, UVec2::new(right - left, bottom - top)))
}

/// Starts a choice of lines at the line pressed.
fn press_line(
    press: On<PointerPress>,
    rows: Query<&LineRow>,
    mut commands: Commands,
) {
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    let at = row.0;
    commands.queue(move |world: &mut World| {
        if let Some(Shown::Lines(lines)) =
            &mut world.resource_mut::<Adding>().shown
        {
            lines.anchor = at;
            lines.start = at;
            lines.end = at;
        }
    });
}

/// Runs the choice of lines to the line dragged over.
fn drag_over_line(
    over: On<PointerDragEnter>,
    rows: Query<&LineRow>,
    mut commands: Commands,
) {
    let Ok(row) = rows.get(over.entity) else {
        return;
    };
    let at = row.0;
    commands.queue(move |world: &mut World| {
        if let Some(Shown::Lines(lines)) =
            &mut world.resource_mut::<Adding>().shown
        {
            lines.start = lines.anchor.min(at);
            lines.end = lines.anchor.max(at);
        }
    });
}

/// The point in the picture's pixels under `at`, a point in the
/// window, kept inside the picture.
fn on_picture(world: &World, node: Entity, at: Vec2) -> Option<Vec2> {
    let rect = rect_of(world, node)?;
    let Some(Shown::Picture(picture)) =
        &world.resource::<Adding>().shown
    else {
        return None;
    };
    let point = (at - rect.min) / picture.scale;
    Some(point.clamp(Vec2::ZERO, picture.size.as_vec2()))
}

/// Starts a box where the picture is pressed.
fn press_picture(
    press: On<PointerPress>,
    drawn: Query<(), With<Drawn>>,
    mut commands: Commands,
) {
    if !drawn.contains(press.entity) {
        return;
    }
    let (node, at) = (press.entity, press.pointer.position);
    commands.queue(move |world: &mut World| {
        let Some(point) = on_picture(world, node, at) else {
            return;
        };
        if let Some(Shown::Picture(picture)) =
            &mut world.resource_mut::<Adding>().shown
        {
            picture.anchor = point;
            picture.chosen = None;
        }
    });
}

/// Draws the box out to where the pointer is.
fn drag_on_picture(
    drag: On<PointerDrag>,
    drawn: Query<(), With<Drawn>>,
    mut commands: Commands,
) {
    if !drawn.contains(drag.entity) {
        return;
    }
    let (node, at) = (drag.entity, drag.pointer.position);
    commands.queue(move |world: &mut World| {
        let Some(point) = on_picture(world, node, at) else {
            return;
        };
        if let Some(Shown::Picture(picture)) =
            &mut world.resource_mut::<Adding>().shown
        {
            let chosen = Rect::from_corners(picture.anchor, point);
            picture.chosen = (chosen.width() >= 4.0
                && chosen.height() >= 4.0)
                .then_some(chosen);
        }
    });
}

/// Lights the lines chosen.
fn paint_lines(
    adding: Res<Adding>,
    mut rows: Query<(&LineRow, &mut BackgroundColor)>,
) {
    let Some(Shown::Lines(lines)) = &adding.shown else {
        return;
    };
    for (row, mut fill) in &mut rows {
        let want = match (lines.start..=lines.end).contains(&row.0) {
            true => CHOSEN,
            false => Color::NONE,
        };
        if fill.0 != want {
            fill.0 = want;
        }
    }
}

/// Puts the box over the part of the picture it was drawn on.
fn place_box(
    adding: Res<Adding>,
    mut boxes: Query<&mut Node, With<ChosenBox>>,
) {
    let Some(Shown::Picture(picture)) = &adding.shown else {
        return;
    };
    let display = match picture.chosen {
        Some(_) => Display::Flex,
        None => Display::None,
    };
    let (left, top, width, height) = match picture.chosen {
        Some(chosen) => (
            chosen.min.x * picture.scale,
            chosen.min.y * picture.scale,
            chosen.width() * picture.scale,
            chosen.height() * picture.scale,
        ),
        None => (0.0, 0.0, 0.0, 0.0),
    };
    for mut node in &mut boxes {
        if node.display != display {
            node.display = display;
        }
        if node.left != Val::Px(left)
            || node.top != Val::Px(top)
            || node.width != Val::Px(width)
            || node.height != Val::Px(height)
        {
            node.left = Val::Px(left);
            node.top = Val::Px(top);
            node.width = Val::Px(width);
            node.height = Val::Px(height);
        }
    }
}

/// The dialog, over the panel while something is being added.
pub(super) fn modal() -> impl View {
    keyed(
        resource::<Adding, _>(|adding| {
            (adding.generation, adding.shown.is_some())
        }),
        |&(_, open)| {
            if !open {
                return frame().boxed();
            }
            with_cx(|cx| {
                let agent = cx
                    .world
                    .get_resource::<Convo>()
                    .and_then(|convo| convo.with.clone())
                    .unwrap_or_else(|| "the Hand".to_owned());
                let world: &World = cx.world;
                let page = world
                    .try_query::<(Entity, &Page)>()
                    .and_then(|mut pages| {
                        pages
                            .iter(world)
                            .find(|(_, page)| page.0 == id())
                            .map(|(page, _)| page)
                    });
                let wide = page
                    .and_then(|page| rect_of(world, page))
                    .map_or(SIDE.0, |page| side(page.width()));
                let body = match &cx.world.resource::<Adding>().shown
                {
                    Some(Shown::Lines(lines)) => row((
                        lines_side(lines),
                        lines_sent(&agent, wide),
                    ))
                    .fills_height()
                    .boxed(),
                    Some(Shown::Picture(picture)) => row((
                        picture_side(picture),
                        picture_sent(&agent, wide),
                    ))
                    .fills_height()
                    .boxed(),
                    None => frame().boxed(),
                };
                let palette = cx.theme().palette.clone();
                let dialog = column((head(agent.clone()), body))
                    .fills_width()
                    .fills_height()
                    .padding(UiRect::new(
                        px(PADDING),
                        px(PADDING),
                        px(0.0),
                        px(PADDING),
                    ))
                    .fill(palette.surface)
                    .border(1.0)
                    .border_color(palette.border)
                    .radius(8.0);
                // The scrim takes the presses meant for the page.
                row((dialog,))
                    .fills_width()
                    .fills_height()
                    .padding(UiRect::all(px(MARGIN)))
                    .fill(Color::srgba(0.03, 0.02, 0.01, 0.62))
            })
            .boxed()
        },
    )
    .within(overlay(()))
}

fn head(agent: String) -> impl View {
    row((
        label(format!("Add to chat with {agent}"))
            .size(20.0)
            .tagged(DisplayText),
        frame().fills_width(),
        close_button(close),
    ))
    .height(px(HEAD))
    .align(AlignItems::Center)
    .shrink(0.0)
}

/// The strip over the file: its name, its folder and what to do.
fn strip(path: String, hint: &'static str) -> impl View {
    let (folder, name) = match path.rsplit_once('/') {
        Some((folder, name)) => (folder.to_owned(), name.to_owned()),
        None => (String::new(), path.to_owned()),
    };
    row((
        label(name).bold(true).wrap(false),
        label(folder).wrap(false).rules(note_rules),
        frame().fills_width(),
        label(hint).wrap(false).rules(note_rules),
    ))
    .gap(8.0)
    .overflow(Overflow::clip())
    .height(px(40.0))
    .padding(UiRect::horizontal(px(14.0)))
    .align(AlignItems::Center)
    .shrink(0.0)
}

fn lines_side(lines: &Lines) -> impl View {
    let first = lines.start.saturating_sub(BEFORE).max(1);
    let last = (lines.end + AFTER).min(lines.lines.len());
    let width = last.to_string().len().max(3);
    let shown = (first..=last)
        .map(|number| {
            let text = lines.lines[number - 1].replace('\t', "    ");
            match text.is_empty() {
                true => (number, " ".to_owned()),
                false => (number, text),
            }
        })
        .collect::<Vec<_>>();
    let path = lines.path.clone();
    with_cx(move |cx| {
        let sheet = cx.theme().palette.card;
        let rows = shown
            .iter()
            .map(|(number, text)| {
                row((
                    label(format!("{number:>width$}"))
                        .tone(Tone::Faint)
                        .wrap(false)
                        .tagged(MonoText),
                    label(text.clone()).wrap(false).tagged(MonoText),
                ))
                .gap(18.0)
                .padding(UiRect::horizontal(px(12.0)))
                .fills_width()
                .tagged(LineRow(*number))
                .tagged(BackgroundColor(Color::NONE))
                .boxed()
            })
            .collect::<Vec<_>>();
        column((
            strip(path.clone(), "Drag over the lines to choose"),
            scroll(rows)
                .fills_width()
                .fills_height()
                .overflow(Overflow::scroll_y()),
        ))
        .fills_width()
        .fills_height()
        .margin(UiRect::right(px(PADDING)))
        .fill(sheet)
        .radius(6.0)
        .overflow(Overflow::clip())
    })
}

fn picture_side(picture: &Picture) -> impl View {
    let size = picture.size.as_vec2() * picture.scale;
    let image = picture.image.clone();
    let path = picture.path.clone();
    with_cx(move |cx| {
        let sheet = cx.theme().palette.card;
        let chosen = frame()
            .position(PositionType::Absolute)
            .border(2.0)
            .border_color(EDGE)
            .tagged(BoxShadow::new(
                Color::srgba(0.1, 0.06, 0.02, 0.5),
                Val::Px(0.0),
                Val::Px(0.0),
                Val::Px(4000.0),
                Val::Px(0.0),
            ))
            .tagged(Pickable::IGNORE)
            .tagged(ChosenBox);
        let shown = row((chosen,))
            .width(px(size.x))
            .height(px(size.y))
            .shrink(0.0)
            .overflow(Overflow::clip())
            .tagged(
                ImageNode::new(image.clone())
                    .with_mode(NodeImageMode::Stretch),
            )
            .tagged(Drawn);
        column((
            strip(path.clone(), "Drag to draw a box"),
            row((shown,))
                .fills_width()
                .fills_height()
                .justify(JustifyContent::Center)
                .align(AlignItems::Center),
        ))
        .fills_width()
        .fills_height()
        .margin(UiRect::right(px(PADDING)))
        .fill(sheet)
        .radius(6.0)
        .overflow(Overflow::clip())
    })
}

/// The side of what is sent, around `card`, saying what the agent
/// gets, with the press that adds it.
fn sent(
    agent: &str,
    card: AnyView,
    gets: String,
    wide: f32,
) -> impl View {
    let agent = agent.to_owned();
    column((
        label(format!("Your message to {agent}"))
            .tone(Tone::Dim)
            .bold(true),
        card,
        note(gets),
        frame().fills_height(),
        row((
            button(label("Cancel").wrap(false))
                .padding(UiRect::axes(px(14.0), px(8.0)))
                .rules(link_row)
                .on_activate(close),
            button(
                row((
                    glyph(Glyph::Talk, ICON),
                    label(format!("Add to chat with {agent}"))
                        .wrap(false),
                ))
                .gap(8.0)
                .align(AlignItems::Center),
            )
            .padding(UiRect::axes(px(14.0), px(8.0)))
            .rules(primary_button)
            .on_activate(add),
        ))
        .gap(10.0)
        .justify(JustifyContent::End),
    ))
    .gap(12.0)
    .width(px(wide))
    .shrink(0.0)
    .fills_height()
}

fn lines_sent(agent: &str, wide: f32) -> impl View {
    let card = keyed(
        resource::<Adding, _>(|adding| match &adding.shown {
            Some(Shown::Lines(lines)) => Some((
                lines.path.clone(),
                lines.start,
                lines.end,
                lines.lines[lines.start - 1..lines.end]
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>(),
            )),
            _ => None,
        }),
        |chosen| {
            let Some((path, start, end, shown)) = chosen.clone()
            else {
                return frame().boxed();
            };
            let name =
                path.rsplit('/').next().unwrap_or(&path).to_owned();
            let which = match start == end {
                true => format!("{name}, line {start}"),
                false => format!("{name}, lines {start}–{end}"),
            };
            card(Glyph::Notes, which, path, move || {
                let code = shown
                    .iter()
                    .map(|line| {
                        label(line.replace('\t', "    "))
                            .size(12.0)
                            .wrap(false)
                            .tagged(MonoText)
                            .boxed()
                    })
                    .collect::<Vec<_>>();
                column(code).boxed()
            })
        },
    )
    .within(column(()));
    sent(
        agent,
        card.boxed(),
        format!("{agent} gets these lines, with the file's path."),
        wide,
    )
}

fn picture_sent(agent: &str, wide: f32) -> impl View {
    let card = keyed(
        resource::<Adding, _>(|adding| match &adding.shown {
            Some(Shown::Picture(picture)) => Some((
                picture.path.clone(),
                picture.image.clone(),
                picture.size,
                picture.chosen.map(|chosen| {
                    [
                        chosen.min.x,
                        chosen.min.y,
                        chosen.max.x,
                        chosen.max.y,
                    ]
                }),
            )),
            _ => None,
        }),
        move |chosen| {
            let Some((path, image, size, chosen)) = chosen.clone()
            else {
                return frame().boxed();
            };
            let name =
                path.rsplit('/').next().unwrap_or(&path).to_owned();
            let part = chosen.map_or(
                Rect::from_corners(Vec2::ZERO, size.as_vec2()),
                |[left, top, right, bottom]| {
                    Rect::new(left, top, right, bottom)
                },
            );
            let fit = ((wide - 26.0) / part.width())
                .min(150.0 / part.height());
            card(Glyph::Picture, name, path, move || {
                let mut thumbnail = ImageNode::new(image.clone())
                    .with_mode(NodeImageMode::Stretch);
                thumbnail.rect = Some(part);
                frame()
                    .width(px(part.width() * fit))
                    .height(px(part.height() * fit))
                    .tagged(thumbnail)
                    .boxed()
            })
        },
    )
    .within(column(()));
    sent(
        agent,
        card.boxed(),
        format!(
            "{agent} gets a picture of the box, with the file's \
             path."
        ),
        wide,
    )
}

/// What is sent, as a card: its mark, what it is, where it is from,
/// and a look at it.
fn card(
    which: Glyph,
    title: String,
    path: String,
    look: impl Fn() -> AnyView + Send + Sync + 'static,
) -> AnyView {
    with_cx(move |cx| {
        let palette = cx.theme().palette.clone();
        column((
            row((
                glyph(which, ICON).tone(Tone::Accent),
                column((
                    label(title.clone()).bold(true),
                    note(path.clone()),
                ))
                .gap(1.0),
            ))
            .gap(10.0)
            .align(AlignItems::Start),
            look(),
        ))
        .gap(10.0)
        .padding(UiRect::all(px(12.0)))
        .fill(palette.input)
        .border(1.0)
        .border_color(palette.border)
        .radius(5.0)
        .overflow(Overflow::clip())
    })
    .boxed()
}

/// Puts what was chosen in the chat's field, and closes the dialog.
fn add(world: &mut World) {
    let Some(shown) = world.resource_mut::<Adding>().shown.take()
    else {
        return;
    };
    // The chat shows, with its field.
    show_tab(world, HUB);
    if let Some(agent) =
        world.get_resource::<Convo>().map(|convo| convo.agent)
    {
        crate::convo::talk_to(world, agent);
    }
    match shown {
        Shown::Lines(lines) => {
            let code =
                lines.lines[lines.start - 1..lines.end].join("\n");
            let which = match lines.start == lines.end {
                true => format!("line {}", lines.start),
                false => {
                    format!("lines {}–{}", lines.start, lines.end)
                }
            };
            let words = format!(
                "From `{}`, {which}:\n```{}\n{code}\n```\n",
                lines.path,
                language(&lines.path),
            );
            chat::add_to_field(world, &words, Vec::new());
        }
        Shown::Picture(picture) => {
            let part = picture.chosen.unwrap_or_else(|| {
                Rect::from_corners(Vec2::ZERO, picture.size.as_vec2())
            });
            if let Some(png) =
                png(&picture.pixels, picture.size, part)
            {
                let words = format!("From `{}`:\n", picture.path);
                chat::add_to_field(world, &words, vec![png]);
            }
        }
    }
    close(world);
}

/// `part` of the RGBA `pixels` of `size`, as a PNG.
fn png(pixels: &[u8], size: UVec2, part: Rect) -> Option<Vec<u8>> {
    let whole = RgbaImage::from_raw(size.x, size.y, pixels.to_vec())?;
    let left = (part.min.x.max(0.0) as u32).min(size.x - 1);
    let top = (part.min.y.max(0.0) as u32).min(size.y - 1);
    let width = (part.width() as u32).clamp(1, size.x - left);
    let height = (part.height() as u32).clamp(1, size.y - top);
    let cut =
        image::imageops::crop_imm(&whole, left, top, width, height)
            .to_image();
    let mut bytes = Cursor::new(Vec::new());
    cut.write_to(&mut bytes, ImageFormat::Png).ok()?;
    Some(bytes.into_inner())
}

/// The name a fenced block of the file at `path` is marked with.
fn language(path: &str) -> &'static str {
    let extension =
        path.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension {
        "rs" => "rust",
        "ts" | "tsx" => "ts",
        "js" | "jsx" | "mjs" | "cjs" => "js",
        "py" => "python",
        "go" => "go",
        "md" => "markdown",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "sh" | "bash" => "sh",
        "html" => "html",
        "css" => "css",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" => "cpp",
        "csv" => "csv",
        _ => "",
    }
}
