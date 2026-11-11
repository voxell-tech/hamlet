//! The bar over the canvas and the tools that go in it, each a view
//! whoever lays the editor out puts in the bar: the open asset and
//! what to do with it as a file, the note of the last file done with,
//! the edit history, the framing, and the switches of the cards over
//! the canvas.

use std::marker::PhantomData;

use bevy::ecs::component::Component;
use bevy::ecs::query::With;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Overflow, UiRect, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, dropdown, frame,
    label, menu_button, row, text_field,
};
use bevy_fynix::{Hovered, ScopedExt, ViewExt, keyed, resource};
use hamlet_ui::styles::{
    self, CONTROL, ICON, checked_when, cluster, description, note,
    primary_button, text_input, toggle_button, tool,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, with_cx};

use super::files::{self, Files, NamingField, Purpose};
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

/// The open asset and what to do with it as a file: a menu of every
/// asset of its kind that opens the one chosen, whether it has
/// changes not saved or is built in, Save while it has changes, a
/// button that makes a new one, and a menu of the rest. While a name
/// is asked for, to save a copy under or rename the file to, the
/// field it is asked in takes their place.
pub fn asset<K: EditorKey>() -> AnyView {
    keyed(
        resource::<Files<K>, _>(|files| {
            files.naming.as_ref().map(|_| files.purpose)
        }),
        |purpose: &Option<Purpose>| match *purpose {
            Some(purpose) => naming::<K>(purpose),
            None => chooser::<K>(),
        },
    )
    .within(row(()).gap(0.0))
    .boxed()
}

/// The bar's file controls while no name is asked for, built again as
/// the assets of the editor's kind or the open one change.
fn chooser<K: EditorKey>() -> AnyView {
    with_cx(|cx| {
        let kind = cx.world.resource::<Editor<K>>().setup.kind;
        keyed(
            resource::<Library, _>(move |library| {
                library
                    .items
                    .iter()
                    .filter(|item| item.kind == kind)
                    .cloned()
                    .collect::<Vec<_>>()
            }),
            move |items: &Vec<Item>| {
                let items = items.clone();
                keyed(
                    resource::<Editor<K>, _>(|editor| Opened {
                        name: editor.name.clone(),
                        own: editor.is_own(),
                        dirty: editor.is_dirty(),
                    }),
                    move |opened: &Opened| {
                        controls::<K>(kind, &items, opened)
                    },
                )
                .within(row(()).gap(0.0))
                .boxed()
            },
        )
        .within(row(()).gap(0.0))
    })
    .boxed()
}

/// What the bar shows of the open asset.
#[derive(Clone, PartialEq)]
struct Opened {
    name: String,
    /// Whether it is a file of the library, not built in.
    own: bool,
    /// Whether it has changes not saved.
    dirty: bool,
}

/// The mark of an asset with changes not saved: a small amber dot.
pub fn unsaved() -> AnyView {
    with_cx(|cx| {
        let amber = cx.theme().palette.accent;
        row((frame()
            .width(px(8.0))
            .height(px(8.0))
            .radius(4.0)
            .fill(amber),))
        .padding(UiRect::all(px(4.0)))
        .tooltip(|| note("Not saved"))
    })
    .boxed()
}

/// The file controls of the open asset, among `items`.
fn controls<K: EditorKey>(
    kind: Kind,
    items: &[Item],
    opened: &Opened,
) -> AnyView {
    let Opened { name, own, dirty } = opened.clone();
    // The library's own first, then the built-in ones.
    let (mine, built_in): (Vec<Item>, Vec<Item>) =
        items.iter().cloned().partition(Item::is_own);
    let order: Vec<Item> = mine.into_iter().chain(built_in).collect();
    let words: Vec<String> = order
        .iter()
        .map(|item| {
            let word = item.name.replace('_', " ");
            if item.is_own() {
                word
            } else {
                format!("{word} (built-in)")
            }
        })
        .collect();
    let at = order
        .iter()
        .position(|item| item.name == *name && item.is_own() == own)
        .unwrap_or(0);
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let order = order.clone();
        let pick = dropdown(
            words.clone(),
            at,
            Glyph::Chevron.image(ICON),
            move |world, at| {
                if let Some(item) = order.get(at) {
                    // A refusal is written in the toolbar's note.
                    let _ = files::open_item::<K>(world, item);
                }
            },
        )
        .width(px(200.0))
        .tooltip(|| note("Open another"));
        let mut parts =
            vec![glyph(kind.glyph(), ICON).boxed(), pick.boxed()];
        if dirty {
            parts.push(unsaved());
        }
        let save: AnyView = if own {
            tool(Glyph::Save, "Save (Cmd+S)", files::save::<K>)
                .boxed()
        } else {
            tool(
                Glyph::Save,
                "Save as a new one of your own: a built-in one \
                 cannot change (Cmd+S)",
                files::save::<K>,
            )
            .boxed()
        };
        parts.push(
            cluster((
                save,
                tool(Glyph::Plus, new_tip(kind), files::create::<K>),
                more::<K>(own),
            ))
            .boxed(),
        );
        row(parts).gap(gap).align(AlignItems::Center)
    })
    .boxed()
}

/// The menu of what else can be done with the open asset as a file.
fn more<K: EditorKey>(own: bool) -> AnyView {
    let mut actions: Vec<(&'static str, fn(&mut World))> =
        vec![("Save as new...", files::save_as::<K>)];
    if own {
        actions.push(("Rename...", files::start_rename::<K>));
    }
    actions.push(("Import...", files::import::<K>));
    actions.push(("Export...", files::export::<K>));
    let entries: Vec<&'static str> =
        actions.iter().map(|(word, _)| *word).collect();
    menu_button("", entries, move |world, at| {
        if let Some((_, act)) = actions.get(at) {
            act(world);
        }
    })
    .icon(Glyph::More.image(ICON))
    .padding(UiRect::axes(px(9.0), px(0.0)))
    .min_height(px(CONTROL))
    .tooltip(|| note("More"))
    .boxed()
}

/// A lettered button, with `mark` before its word when it has one.
fn action(
    mark: Option<Glyph>,
    word: &'static str,
    tip: &'static str,
    variant: Variant,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    let mut face: Vec<AnyView> = Vec::new();
    if let Some(mark) = mark {
        face.push(glyph(mark, ICON).boxed());
    }
    face.push(label(word).wrap(false).boxed());
    let button = button(row(face).gap(4.0).align(AlignItems::Center))
        .padding(UiRect::axes(px(8.0), px(2.0)))
        .min_height(px(CONTROL))
        .tooltip(move || note(tip));
    match variant {
        Variant::Primary => button
            .rules(primary_button)
            .tagged(variant)
            .on_activate(act)
            .boxed(),
        _ => button
            .rules(styles::button)
            .tagged(variant)
            .on_activate(act)
            .boxed(),
    }
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

/// Marks the button that stops asking for a name.
#[derive(Component)]
struct StopNaming<K: EditorKey>(PhantomData<K>);

/// The field a name is asked in, to save a copy under or rename the
/// file to, and the buttons that do it and that stop asking. Enter
/// does it, and Escape puts the field back.
fn naming<K: EditorKey>(purpose: Purpose) -> AnyView {
    let (word, done, tip) = match purpose {
        Purpose::SaveAs => (
            "Save as",
            "Save",
            "Save a copy under this name, and go on editing it",
        ),
        Purpose::Rename => {
            ("Rename to", "Rename", "Give it this name")
        }
    };
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let kind = cx.world.resource::<Editor<K>>().setup.kind;
        row((
            glyph(kind.glyph(), ICON),
            label(word).wrap(false),
            text_field(
                resource::<Files<K>, _>(|files| {
                    files.naming.clone().unwrap_or_default()
                }),
                |world, name| {
                    // Focus leaving after an edit commits the field,
                    // and pressing Cancel takes it: then it stops.
                    let stopping = world
                        .query_filtered::<(), (
                            With<StopNaming<K>>,
                            With<Hovered>,
                        )>()
                        .iter(world)
                        .next()
                        .is_some();
                    if stopping {
                        files::stop_naming::<K>(world);
                    } else {
                        files::named::<K>(world, &name);
                    }
                },
            )
            .width(px(200.0))
            .tagged(NamingField::<K>(PhantomData))
            .rules(text_input),
            action(None, done, tip, Variant::Primary, |world| {
                let name =
                    world.resource::<Files<K>>().naming.clone();
                if let Some(name) = name {
                    files::named::<K>(world, &name);
                }
            }),
            button(label("Cancel").wrap(false))
                .padding(UiRect::axes(px(8.0), px(2.0)))
                .min_height(px(CONTROL))
                .rules(styles::button)
                .tagged(Variant::Default)
                .tagged(StopNaming::<K>(PhantomData))
                .on_activate(files::stop_naming::<K>),
        ))
        .gap(gap)
        .align(AlignItems::Center)
    })
    .boxed()
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
