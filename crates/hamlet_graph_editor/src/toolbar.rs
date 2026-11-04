//! The bar over the canvas. At its left the open asset, its name to
//! change in place, and what to do with it as a file; at its right
//! the edit history, the framing, and the cards over the canvas.

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
use super::flavor::Flavor;
use super::library::{Item, Kind, Library};
use super::split::{Layout, Pane};
use super::{Editor, canvas};

/// The bar: the open asset and its files, the tools of the flavor,
/// what is known of the last run and of the last file done with, and
/// the tools of the view.
pub(super) fn view<F: Flavor>() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        let own = F::tools();
        let own_shown = if own.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        row((
            open_asset::<F>(),
            cluster((
                opener::<F>(),
                tool(
                    Glyph::Plus,
                    new_tip(F::KIND),
                    files::create::<F>,
                ),
                tool(Glyph::Save, "Save (Cmd+S)", files::save::<F>),
                tool(
                    Glyph::Rename,
                    "Save as (Shift+Cmd+S)",
                    files::save_as::<F>,
                ),
                menu_button("", ["Import", "Export"], |world, at| {
                    if at == 0 {
                        files::import::<F>(world);
                    } else {
                        files::export::<F>(world);
                    }
                })
                .icon(Glyph::More.image(ICON))
                .padding(UiRect::axes(px(9.0), px(0.0)))
                .min_height(px(CONTROL)),
            )),
            naming::<F>(),
            row(own)
                .gap(2.0)
                .align(AlignItems::Center)
                .display(own_shown),
            frame().grow(1.0),
            description(resource::<Files<F>, _>(|files| {
                files.note.clone()
            })),
            description(resource::<Editor<F>, _>(F::status)),
            cluster((
                tool(Glyph::Undo, "Undo (Cmd+Z)", |world| {
                    world.resource_mut::<Editor<F>>().undo();
                }),
                tool(Glyph::Redo, "Redo (Shift+Cmd+Z)", |world| {
                    world.resource_mut::<Editor<F>>().redo();
                }),
            )),
            cluster((tool(
                Glyph::Aim,
                "Fit every node in sight (F)",
                canvas::fit::<F>,
            ),)),
            cluster((
                card_toggle::<F>(
                    Glyph::Sliders,
                    "Show or hide the parameters",
                    Pane::Parameters,
                ),
                card_toggle::<F>(
                    F::ICON,
                    "Show or hide the preview",
                    Pane::Preview,
                ),
            )),
        ))
        .width(percent(100.0))
        .min_width(px(0.0))
        .overflow(Overflow::clip())
        .padding(UiRect::all(px(gap)))
        .gap(gap)
        .align(AlignItems::Center)
        .fill(theme.palette.card)
    })
}

/// The tooltip of the button that makes a new asset of `kind`.
fn new_tip(kind: Kind) -> &'static str {
    match kind {
        Kind::Blueprint => "New blueprint",
        Kind::Material => "New material",
        Kind::Mesh | Kind::Texture => "New asset",
    }
}

/// The kind's icon and the open asset's name, a field to rename it in
/// while it is a file of the library, a star while it has changes not
/// saved, and a note while it is built in.
fn open_asset<F: Flavor>() -> impl View {
    let shown = |own: bool| {
        resource::<Editor<F>, _>(move |editor| {
            if editor.is_own() == own {
                Display::Flex
            } else {
                Display::None
            }
        })
    };
    row((
        glyph(F::KIND.glyph(), ICON),
        text_field(
            resource::<Editor<F>, _>(|editor| editor.name.clone()),
            |world, name| files::rename::<F>(world, &name),
        )
        .width(px(170.0))
        .display(shown(true))
        .rules(text_input),
        row((note(resource::<Editor<F>, _>(|editor| {
            editor.name.replace('_', " ")
        })),))
        .display(shown(false)),
        note(resource::<Editor<F>, _>(|editor| {
            if editor.is_dirty() { "*" } else { "" }.to_owned()
        })),
        description(resource::<Editor<F>, _>(|editor| {
            if editor.is_own() { "" } else { "(built-in)" }.to_owned()
        })),
    ))
    .gap(4.0)
    .align(AlignItems::Center)
}

/// The menu of every asset of the flavor's kind, the library's own
/// and then the built-in ones under headings, which opens the one
/// chosen. Built again as the library changes.
fn opener<F: Flavor>() -> impl View {
    keyed(
        resource::<Library, _>(|library| {
            library
                .items
                .iter()
                .filter(|item| item.kind == F::KIND)
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
                    let _ = files::open_item::<F>(world, item);
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
fn naming<F: Flavor>() -> impl View {
    row((
        text_field(
            resource::<Files<F>, _>(|files| {
                files.naming.clone().unwrap_or_default()
            }),
            |world, name| files::save_named::<F>(world, &name),
        )
        .width(px(170.0))
        .tagged(NamingField::<F>(PhantomData))
        .rules(text_input),
        tool(Glyph::Cross, "Do not save as", |world| {
            world.resource_mut::<Files<F>>().naming = None;
        }),
    ))
    .gap(2.0)
    .align(AlignItems::Center)
    .display(resource::<Files<F>, _>(|files| {
        if files.naming.is_some() {
            Display::Flex
        } else {
            Display::None
        }
    }))
}

/// A button that shows or hides the card `pane`, checked while it
/// shows.
fn card_toggle<F: Flavor>(
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
                world.resource_mut::<Layout<F>>().toggle(pane);
            }),
        resource::<Layout<F>, _>(move |layout| {
            layout.card(pane).visible
        }),
    )
}
