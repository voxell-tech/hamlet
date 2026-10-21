//! An explorer as drawn, after VS Code's: a head with the folder's
//! name and its tools over the tree, a row for each entry shown.

use std::path::{Path, PathBuf};

use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, Overflow, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, Icon, TooltipExt,
    button, column, frame, label, menu_item, row, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, component, each, keyed, style};
use hamlet_core::agents::Works;
use hamlet_desktop::files::Entry;

use super::acts;
use super::sync::{self, Explorer};
use super::tree::{Line, Row};
use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::looks::{Fills, ICON, filling, note, retrace};
use crate::platform::DisplayText;
use crate::theme::HamletTheme;
use crate::{AnyView, Cx, View, with_cx};

/// The height of a row.
const ROW: f32 = 22.0;

/// How far each folder's rows sit in from its own.
const INDENT: f32 = 12.0;

/// The width of the place a folder's chevron is drawn in.
const TWISTY: f32 = 16.0;

/// The width of the place an entry's icon is drawn in.
const MARK: f32 = 20.0;

/// The words of the menu item that shows an entry in its folder.
const REVEAL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(target_os = "windows") {
    "Reveal in File Explorer"
} else {
    "Open Containing Folder"
};

/// The explorer of the folder `hamlet` works on, filling the height
/// it is given.
pub(crate) fn panel(hamlet: Entity) -> AnyView {
    let folder = component::<Works, _>(hamlet, |works| {
        works.and_then(|works| works.0.folder.clone())
    });
    keyed(folder, move |folder| match folder {
        Some(folder) => explorer(hamlet, folder.clone()),
        None => column((note(
            "This hamlet works on no folder. Choose one above to \
             see its files here.",
        ),))
        .boxed(),
    })
    .within(filling())
    .boxed()
}

fn explorer(hamlet: Entity, folder: PathBuf) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        sync::open(cx.world, hamlet, &folder);
        cx.build(
            column((
                head(hamlet, &folder),
                scroll((rows(hamlet),)).fills_height(),
            ))
            .gap(4.0)
            .fills_height(),
        )
    })
}

/// The folder's name, and the tools over the whole tree.
fn head(hamlet: Entity, folder: &Path) -> impl View {
    let name = crate::folder::name_of(folder).to_uppercase();
    row((
        row((label(name)
            .size(12.0)
            .bold(true)
            .tone(Tone::Faint)
            .wrap(false)
            .tagged(DisplayText),))
        .overflow(Overflow::clip_x())
        .fills_width(),
        tool(Glyph::Restore, "Refresh Explorer", move |world| {
            sync::refresh(world, hamlet)
        }),
        tool(
            Glyph::Fold,
            "Collapse Folders in Explorer",
            move |world| acts::collapse(world, hamlet),
        ),
    ))
    .gap(2.0)
    .align(AlignItems::Center)
}

/// A button of an icon alone, named by `tip` under the pointer.
fn tool(
    which: Glyph,
    tip: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    button(glyph(which, ICON))
        .padding(UiRect::all(px(4.0)))
        .tooltip(move || label(tip).size(12.0))
        .rules(retrace)
        .tagged(Mood::Ink)
        .on_activate(act)
        .boxed()
}

/// The tree's rows. A row is built again only when what it shows
/// changes.
fn rows(hamlet: Entity) -> impl View {
    let rows = component::<Explorer, _>(hamlet, |explorer| {
        explorer
            .map(|explorer| explorer.0.rows())
            .unwrap_or_default()
    });
    each(rows, |row: &Row| row.clone(), move |row| line(hamlet, row))
        .within(column(()).gap(0.0))
}

fn line(hamlet: Entity, row: &Row) -> AnyView {
    let depth = row.depth;
    match &row.line {
        Line::Entry {
            entry,
            open,
            selected,
        } => {
            entry_row(hamlet, depth, entry.clone(), *open, *selected)
        }
        Line::Reading(_) => remark(depth, "Loading...".into()),
        Line::Failed(_, why) => remark(
            depth,
            format!("Could not read this folder: {why}"),
        ),
        Line::More(_, more) => {
            remark(depth, format!("{more} more not shown"))
        }
        Line::Empty => remark(depth, "This folder is empty.".into()),
    }
}

/// An entry: pressed, a folder opens or closes and a file opens;
/// right-clicked, what else can be done with it.
fn entry_row(
    hamlet: Entity,
    depth: usize,
    entry: Entry,
    open: bool,
    selected: bool,
) -> AnyView {
    // Ignored entries stand back, as VS Code greys them.
    let (words, mark) = if entry.ignored {
        (Tone::Dim, Tone::Dim)
    } else {
        (Tone::Body, Tone::Faint)
    };
    let (twisty, icon) = match (entry.is_folder(), open) {
        (true, true) => (Some(Glyph::Down), Glyph::Folder),
        (true, false) => (Some(Glyph::Into), Glyph::Folder),
        (false, _) => (None, Glyph::File),
    };
    let look: fn(&mut Cx) = if selected { picked } else { twig };
    let name = entry.name.clone();
    let menu = entry.clone();
    button(
        row((
            guides(depth),
            cell(
                TWISTY,
                twisty
                    .map(|which| glyph(which, 12.0).tone(Tone::Dim)),
            ),
            cell(MARK, Some(glyph(icon, ICON).tone(mark))),
            label(name).size(13.0).tone(words).wrap(false),
        ))
        .gap(0.0)
        .align(AlignItems::Center),
    )
    .height(px(ROW))
    .padding(UiRect::horizontal(px(4.0)))
    .overflow(Overflow::clip_x())
    .rules(look)
    .on_activate(move |world| acts::press(world, hamlet, &entry))
    .context_menu(move || items(hamlet, &menu))
    .boxed()
}

/// Dim words where an entry would be, `depth` folders in.
fn remark(depth: usize, words: String) -> AnyView {
    row((guides(depth), cell(TWISTY, None), note(words).wrap(false)))
        .gap(0.0)
        .height(px(ROW))
        .padding(UiRect::horizontal(px(4.0)))
        .align(AlignItems::Center)
        .overflow(Overflow::clip_x())
        .boxed()
}

/// A place `width` wide with `inside` in its middle.
fn cell(width: f32, inside: Option<Icon>) -> AnyView {
    row(inside.into_iter().collect::<Vec<_>>())
        .width(px(width))
        .justify(JustifyContent::Center)
        .align(AlignItems::Center)
        .shrink(0.0)
        .boxed()
}

/// A line down through each of the `depth` folders a row is in,
/// under each one's chevron.
fn guides(depth: usize) -> AnyView {
    with_cx(move |cx: &Cx| {
        let colour = cx.theme().palette.brass_6;
        let guide = move || {
            row((frame().width(px(1.0)).height(px(ROW)).fill(colour),))
                .width(px(INDENT))
                .padding(UiRect::left(px(TWISTY / 2.0)))
                .shrink(0.0)
                .boxed()
        };
        row((0..depth).map(|_| guide()).collect::<Vec<_>>()).gap(0.0)
    })
}

/// What can be done with `entry`, as its menu offers.
fn items(hamlet: Entity, entry: &Entry) -> Vec<AnyView> {
    let item = |text: &'static str,
                act: fn(&mut World, Entity, &Path)| {
        let path = entry.path.clone();
        menu_item(label(text).wrap(false))
            .on_activate(move |world| act(world, hamlet, &path))
            .boxed()
    };
    let mut items = Vec::new();
    if !entry.is_folder() {
        items.push(item("Open", acts::show));
    }
    items.push(item(REVEAL, acts::reveal));
    items.push(item("Copy Path", |world, _, path| {
        acts::copy_path(world, path)
    }));
    items.push(item("Copy Relative Path", acts::copy_relative));
    items
}

/// A row of the tree: no plate at rest, and a faint one under the
/// pointer.
fn twig(cx: &mut Cx) {
    row_look(cx, |_| Color::NONE, |t| t.palette.brass_hover);
}

/// The row selected: lit with the tint of what is selected.
fn picked(cx: &mut Cx) {
    row_look(cx, |t| t.palette.seam, |t| t.palette.seam);
}

/// The look of a row: filled by `rest`, and by `hover` under the
/// pointer.
fn row_look(
    cx: &mut Cx,
    rest: fn(&HamletTheme) -> Color,
    hover: fn(&HamletTheme) -> Color,
) {
    style::<HamletTheme>()
        .frame(move |f, t| {
            f.fill(rest(t))
                .radius(t.space.round)
                .justify(JustifyContent::FlexStart)
        })
        .hovered(move |s| s.fill(hover))
        .transition(Motion::Interact)
        .apply(cx);
}
