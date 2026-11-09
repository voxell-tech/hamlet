//! The bar over the canvas and the tools that go in it, each a view
//! whoever lays the editor out puts in the bar: the open asset and
//! what to do with it as a file, the note of the last file done with,
//! the edit history, the framing, and the switches of the cards over
//! the canvas.

use std::marker::PhantomData;

use bevy::ui::{AlignItems, Display, Overflow, UiRect, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, MenuEntry, TooltipExt, button, frame,
    menu_button, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_ui::styles::{
    CONTROL, ICON, checked_when, cluster, description, note,
    text_input, toggle_button, tool,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use super::files::{self, Files, NamingField};
use super::library::{Item, Kind, Library};
use super::setup::EditorKey;
use super::split::{Layout, Pane};
use super::{Editor, canvas};

/// The bar: `start` from its left and `end` from its right.
pub fn bar(start: Vec<AnyView>, end: Vec<AnyView>) -> AnyView {
    with_cx(move |cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        let mut parts = start;
        parts.push(frame().grow(1.0).boxed());
        parts.extend(end);
        row(parts)
            .width(percent(100.0))
            .min_width(px(0.0))
            .overflow(Overflow::clip())
            .padding(UiRect::all(px(gap)))
            .gap(gap)
            .align(AlignItems::Center)
            .fill(theme.palette.card)
    })
    .boxed()
}

/// The open asset and what to do with it as a file: its name, the
/// menu that opens another, and the tools that make, save, import and
/// export one; and the field Save as asks for a name in, while it
/// asks.
pub fn asset<K: EditorKey>() -> AnyView {
    with_cx(|cx| {
        let kind = cx.world.resource::<Editor<K>>().setup.kind;
        row((
            open_asset::<K>(kind),
            cluster((
                opener::<K>(kind),
                tool(Glyph::Plus, new_tip(kind), files::create::<K>),
                tool(Glyph::Save, "Save (Cmd+S)", files::save::<K>),
                tool(
                    Glyph::Rename,
                    "Save as (Shift+Cmd+S)",
                    files::save_as::<K>,
                ),
                menu_button("", ["Import", "Export"], |world, at| {
                    if at == 0 {
                        files::import::<K>(world);
                    } else {
                        files::export::<K>(world);
                    }
                })
                .icon(Glyph::More.image(ICON))
                .padding(UiRect::axes(px(9.0), px(0.0)))
                .min_height(px(CONTROL)),
            )),
            naming::<K>(),
        ))
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center)
    })
    .boxed()
}

/// What was said of the last file done with.
pub fn file_note<K: EditorKey>() -> AnyView {
    description(resource::<Files<K>, _>(|files| files.note.clone()))
        .boxed()
}

/// Undo and redo.
pub fn history<K: EditorKey>() -> AnyView {
    cluster((
        tool(Glyph::Undo, "Undo (Cmd+Z)", |world| {
            world.resource_mut::<Editor<K>>().undo();
        }),
        tool(Glyph::Redo, "Redo (Shift+Cmd+Z)", |world| {
            world.resource_mut::<Editor<K>>().redo();
        }),
    ))
    .boxed()
}

/// The button that fits every node in sight.
pub fn fit<K: EditorKey>() -> AnyView {
    cluster((tool(
        Glyph::Aim,
        "Fit every node in sight (F)",
        canvas::fit::<K>,
    ),))
    .boxed()
}

/// The buttons that show or hide the parameters and the preview,
/// named by `preview`.
pub fn cards<K: EditorKey>(preview: Glyph) -> AnyView {
    cluster((
        card_toggle::<K>(
            Glyph::Sliders,
            "Show or hide the parameters",
            Pane::Parameters,
        ),
        card_toggle::<K>(
            preview,
            "Show or hide the preview",
            Pane::Preview,
        ),
    ))
    .boxed()
}

/// The tooltip of the button that makes a new asset of `kind`.
fn new_tip(kind: Kind) -> &'static str {
    match kind {
        Kind::Blueprint => "New blueprint",
        Kind::Shader => "New shader",
        Kind::Prefab => "New prefab",
        Kind::Material | Kind::Mesh | Kind::Texture => "New asset",
    }
}

/// The icon of `kind` and the open asset's name, a field to rename it
/// in while it is a file of the library, a star while it has changes
/// not saved, and a note while it is built in.
fn open_asset<K: EditorKey>(kind: Kind) -> impl View {
    let shown = |own: bool| {
        resource::<Editor<K>, _>(move |editor| {
            if editor.is_own() == own {
                Display::Flex
            } else {
                Display::None
            }
        })
    };
    row((
        glyph(kind.glyph(), ICON),
        text_field(
            resource::<Editor<K>, _>(|editor| editor.name.clone()),
            |world, name| files::rename::<K>(world, &name),
        )
        .width(px(170.0))
        .display(shown(true))
        .rules(text_input),
        row((note(resource::<Editor<K>, _>(|editor| {
            editor.name.replace('_', " ")
        })),))
        .display(shown(false)),
        note(resource::<Editor<K>, _>(|editor| {
            if editor.is_dirty() { "*" } else { "" }.to_owned()
        })),
        description(resource::<Editor<K>, _>(|editor| {
            if editor.is_own() { "" } else { "(built-in)" }.to_owned()
        })),
    ))
    .gap(4.0)
    .align(AlignItems::Center)
}

/// The menu of every asset of `kind`, the library's own and then the
/// built-in ones under headings, which opens the one chosen. Built
/// again as the library changes.
fn opener<K: EditorKey>(kind: Kind) -> impl View {
    keyed(
        resource::<Library, _>(move |library| {
            library
                .items
                .iter()
                .filter(|item| item.kind == kind)
                .cloned()
                .collect::<Vec<_>>()
        }),
        |items: &Vec<Item>| {
            let (own, built_in): (Vec<Item>, Vec<Item>) =
                items.iter().cloned().partition(Item::is_own);
            let mut entries = Vec::new();
            for (heading, part) in
                [("Added", &own), ("Built-in", &built_in)]
            {
                if part.is_empty() {
                    continue;
                }
                entries.push(MenuEntry::section(heading));
                entries.extend(part.iter().map(|item| {
                    MenuEntry::from(item.name.replace('_', " "))
                }));
            }
            // Headings take no place in the order chosen from.
            let order =
                own.into_iter().chain(built_in).collect::<Vec<_>>();
            menu_button("", entries, move |world, at| {
                if let Some(item) = order.get(at) {
                    // A refusal is written in the toolbar's note.
                    let _ = files::open_item::<K>(world, item);
                }
            })
            .icon(Glyph::Folder.image(ICON))
            .padding(UiRect::axes(px(9.0), px(0.0)))
            .min_height(px(CONTROL))
            .boxed()
        },
    )
    .within(row(()).gap(0.0))
}

/// The field Save as asks for a name in, and the button that drops
/// it, shown while it asks. Enter saves.
fn naming<K: EditorKey>() -> impl View {
    row((
        text_field(
            resource::<Files<K>, _>(|files| {
                files.naming.clone().unwrap_or_default()
            }),
            |world, name| files::save_named::<K>(world, &name),
        )
        .width(px(170.0))
        .tagged(NamingField::<K>(PhantomData))
        .rules(text_input),
        tool(Glyph::Cross, "Do not save as", |world| {
            world.resource_mut::<Files<K>>().naming = None;
        }),
    ))
    .gap(2.0)
    .align(AlignItems::Center)
    .display(resource::<Files<K>, _>(|files| {
        if files.naming.is_some() {
            Display::Flex
        } else {
            Display::None
        }
    }))
}

/// A button that shows or hides the card `pane`, checked while it
/// shows.
fn card_toggle<K: EditorKey>(
    which: Glyph,
    tip: &'static str,
    pane: Pane,
) -> AnyView {
    checked_when(
        button(glyph(which, ICON))
            .padding(UiRect::axes(px(9.0), px(0.0)))
            .min_height(px(CONTROL))
            .tooltip(move || note(tip))
            .rules(toggle_button)
            .tagged(Variant::Default)
            .on_activate(move |world| {
                world.resource_mut::<Layout<K>>().toggle(pane);
            }),
        resource::<Layout<K>, _>(move |layout| {
            layout.card(pane).visible
        }),
    )
}
