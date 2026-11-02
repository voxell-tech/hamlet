//! The Explorer panel, after VS Code's: a choice of hamlet, a head
//! with its folder's name and the tools over its tree, and a row for
//! each entry shown.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, JustifyContent, Overflow, UiRect, percent, px,
};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, Icon, TooltipExt,
    button, column, dropdown, frame, label, menu_item, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, component, derived, each, keyed, style,
};
use hamlet_desktop::files::Entry;
use hamlet_ui::styles::{
    Fills, ICON, back_link, filling, note, scroll_area,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::sync::{self, Explorer};
use super::tree::{Line, Row};
use super::{Browsing, acts, hamlets, shown};
use crate::roll::{either, hamlets_moved, moved};

/// How often the chosen folder's presence on the disk is looked at.
const LOOK_AGAIN: Duration = Duration::from_secs(1);

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

/// The panel: the hamlet chosen, and the files of the folder it
/// works on.
pub(super) fn panel() -> impl View {
    column((picker(), body())).gap(8.0).fills_height()
}

/// The choice of the hamlet whose files are shown.
fn picker() -> impl View {
    let list =
        derived(|world: &World| hamlets(world)).when(hamlets_moved());
    keyed(list, |list| {
        if list.is_empty() {
            return column(()).boxed();
        }
        let names = list.iter().map(|(_, name)| name.clone());
        let entities = list
            .iter()
            .map(|(hamlet, _)| *hamlet)
            .collect::<Vec<_>>();
        let order = entities.clone();
        let at = derived(move |world: &World| {
            shown(world)
                .and_then(|hamlet| {
                    order.iter().position(|one| *one == hamlet)
                })
                .unwrap_or_default()
        })
        .when(either(moved::<Browsing>(), hamlets_moved()));
        dropdown(
            names,
            at,
            Glyph::Chevron.image(ICON),
            move |world, at| {
                world.resource_mut::<Browsing>().0 =
                    entities.get(at).copied();
            },
        )
        .width(percent(100.0))
        .max_width(percent(100.0))
        .boxed()
    })
    .within(column(()))
}

/// The chosen hamlet and its folder, with whether the folder is on
/// this machine.
type Showing = Option<(Entity, Option<(String, bool)>)>;

/// The tree of the chosen hamlet's folder, or why there is none.
fn body() -> impl View {
    let showing = derived(|world: &World| -> Showing {
        shown(world).map(|hamlet| {
            let folder = super::folder(world, hamlet).map(|folder| {
                let here = Path::new(&folder).is_dir();
                (folder, here)
            });
            (hamlet, folder)
        })
    })
    .when(either(
        either(moved::<Browsing>(), hamlets_moved()),
        // A folder can come onto the disk, or leave it, at any time.
        every(LOOK_AGAIN),
    ));
    keyed(showing, |showing| match showing {
        None => note("There is no hamlet yet.").boxed(),
        Some((_, None)) => note(
            "This hamlet works on no folder. Choose one on its page \
             to see its files here.",
        )
        .boxed(),
        Some((_, Some((folder, false)))) => note(format!(
            "{folder} is not a folder on this machine, so its files \
             cannot be shown here."
        ))
        .boxed(),
        Some((hamlet, Some((folder, true)))) => {
            explorer(*hamlet, PathBuf::from(folder)).boxed()
        }
    })
    .within(filling())
}

/// A check that fires once `gap` has passed since it last did.
fn every(
    gap: Duration,
) -> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut last = None::<Instant>;
    move |_| {
        let now = Instant::now();
        let due = last.is_none_or(|at| now - at >= gap);
        if due {
            last = Some(now);
        }
        due
    }
}

fn explorer(hamlet: Entity, folder: PathBuf) -> impl View {
    hamlet_ui::built(move |cx| {
        sync::open(cx.world, hamlet, &folder);
        cx.build(
            column((
                head(hamlet, &folder),
                scroll_area((rows(hamlet),)),
            ))
            .gap(4.0)
            .fills_height(),
        )
    })
}

/// The folder's name, and the tools over the whole tree.
fn head(hamlet: Entity, folder: &Path) -> impl View {
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| folder.display().to_string());
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
            sync::refresh(world, hamlet);
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
) -> impl View {
    button(glyph(which, ICON))
        .padding(UiRect::all(px(4.0)))
        .tooltip(move || note(tip))
        .rules(back_link)
        .tagged(Variant::Text)
        .on_activate(act)
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
        Line::Entry { .. } => entry_row(hamlet, row),
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

/// An entry: pressed, it is selected, and a folder opens or closes;
/// right-clicked, what else can be done with it.
fn entry_row(hamlet: Entity, row_of: &Row) -> AnyView {
    let Line::Entry {
        entry,
        open,
        selected,
    } = row_of.line.clone()
    else {
        return remark(row_of.depth, String::new());
    };
    let depth = row_of.depth;
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
    row((
        guides(depth),
        cell(TWISTY, None),
        label(words).size(12.0).tone(Tone::Dim).wrap(false),
    ))
    .gap(0.0)
    .height(px(ROW))
    .padding(UiRect::horizontal(px(4.0)))
    .align(AlignItems::Center)
    .overflow(Overflow::clip_x())
    .boxed()
}

/// A place `width` wide with `inside` in its middle.
fn cell(width: f32, inside: Option<Icon>) -> impl View {
    row(inside.into_iter().collect::<Vec<_>>())
        .width(px(width))
        .justify(JustifyContent::Center)
        .align(AlignItems::Center)
        .shrink(0.0)
}

/// A line down through each of the `depth` folders a row is in,
/// under each one's chevron.
fn guides(depth: usize) -> impl View {
    with_cx(move |cx: &Cx| {
        let colour = cx.theme().palette.gold_6;
        let guide = move || {
            row((frame().width(px(1.0)).height(px(ROW)).fill(colour),))
                .width(px(INDENT))
                .padding(UiRect::left(px(TWISTY / 2.0)))
                .shrink(0.0)
        };
        row((0..depth).map(|_| guide()).collect::<Vec<_>>()).gap(0.0)
    })
}

/// The items of `entry`'s right-click menu.
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
        acts::copy_path(world, path);
    }));
    items.push(item("Copy Relative Path", acts::copy_relative));
    items
}

/// A row of the tree: no fill at rest, and a faint one under the
/// pointer.
fn twig(cx: &mut Cx) {
    row_look(cx, |_| Color::NONE, |t| t.palette.gold_hover);
}

/// The row selected: lit with the tint of what is selected.
fn picked(cx: &mut Cx) {
    row_look(cx, |t| t.palette.border, |t| t.palette.border);
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
