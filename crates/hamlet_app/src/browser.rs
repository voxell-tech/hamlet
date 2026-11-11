//! The ASSETS panel: the library as a grid of tiles under a heading
//! for each kind, the person's own before the built-in ones, filtered
//! by kind and by the collection picked in the list beside it and
//! searched by name. A person's tile that stands in for a built-in
//! one of the same name says so, and so does the built-in one.
//!
//! A click selects a tile and a double click opens it. A tile dragged
//! onto a canvas adds what it is there, and onto a collection puts it
//! in that collection, which lights up while the tile is over it.
//! Delete moves the selected asset to the trash, F2 renames it, and
//! Ctrl+Z and Ctrl+Y undo and redo either, or a move between
//! collections, while the pointer is on the panel. The same things
//! are in each tile's right-click menu. Only an asset with settings
//! of its own opens a pane beside the grid: a material, for its
//! shader and values, and a mesh with material slots, for their
//! colours. Import adds `.glb` files as meshes and `.png` files as
//! textures.

mod material;

use std::collections::BTreeSet;

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::InputFocus;
use bevy::picking::events::{
    PointerDragDrop, PointerDragOver, PointerPress,
};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::time::Time;
use bevy::ui::{
    AlignItems, Display, JustifyContent, Overflow, PositionType,
    UiRect, Val, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, TooltipExt, button,
    column, frame, label, menu_item, row, scroll, text_field,
};
use bevy_fynix::{
    EntityCursor, ModifierExt, ScopedExt, ViewExt, keyed, resource,
    style,
};
use hamlet_desktop::open;
use hamlet_graph::Catalogue;
use hamlet_graph_editor::library::carry::{Carried, tile_of};
use hamlet_graph_editor::library::collections::within;
use hamlet_graph_editor::library::picker::{
    centred, picture, wrapped,
};
use hamlet_graph_editor::library::search::search;
use hamlet_graph_editor::library::{
    Item, Kind, Library, Removed, Source, Tile,
};
use hamlet_ui::styles::{
    self, Fills, ICON, action, description, note, section,
    segmented_words, text_input,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::dock::{PanelAppExt, flush_panel};
use crate::focus::TakesFocus;
use crate::sides::{self, Side};

/// A tile's width.
const TILE: f32 = 100.0;
/// The picture's side within a tile.
const PICTURE: f32 = 80.0;
/// The width the settings pane beside the grid starts at.
const DETAILS: f32 = 300.0;
/// The width of the search field.
const SEARCH: f32 = 180.0;
/// The height of a tile's name: two lines of a note.
const NAME: f32 = 32.0;
/// The width the list of collections starts at.
const LIST: f32 = 200.0;
/// How far a collection's row is set in for each collection above.
const INDENT: f32 = 14.0;
/// How long the last thing done stays in the bar, in seconds.
const NOTED: f32 = 4.0;

/// The kinds, in the order the grid shows them, each with its name in
/// the plural.
///
/// Blueprints are kept out of sight while their editor is: see
/// [`crate::graphs::blueprint::SHOWN`].
const KINDS: [(Kind, &str); 5] = [
    (Kind::Shader, "shaders"),
    (Kind::Material, "materials"),
    (Kind::Prefab, "prefabs"),
    (Kind::Mesh, "meshes"),
    (Kind::Texture, "textures"),
];

/// The words of the kind filter, one for each of [`KINDS`].
const FILTERS: [&str; 5] =
    ["Shaders", "Materials", "Prefabs", "Meshes", "Textures"];

/// Marks the ASSETS panel, to know when the pointer is on it, and
/// names its columns.
#[derive(Component)]
struct Panel;

pub(crate) fn plugin(app: &mut App) {
    sides::plugin::<Panel>(app, LIST, DETAILS);
    app.add_panel(
        "assets",
        DockWindowKind::new("ASSETS", || flush_panel(view()))
            .icon(Glyph::Folder.image(ICON)),
    )
    .add_plugins(material::plugin)
    .init_resource::<Browse>()
    .init_resource::<Shown>()
    .init_resource::<Undo>()
    .init_resource::<Over>()
    .add_observer(pressed)
    .add_observer(dropped_on_row)
    .add_observer(dragged_over_row)
    .add_systems(Update, (show, pick, light, carrying, keys, fade));
}

/// How the library is browsed: the kind shown, the collection shown,
/// one of that kind's or none for every asset of it, what is searched
/// for, the tile selected, and what is being renamed.
#[derive(Resource)]
struct Browse {
    kind: Kind,
    collection: Option<String>,
    search: String,
    selected: Option<Item>,
    renaming: Option<Renaming>,
}

impl Default for Browse {
    fn default() -> Self {
        Self {
            kind: Kind::Mesh,
            collection: None,
            search: String::new(),
            selected: None,
            renaming: None,
        }
    }
}

/// What a name is being typed for.
#[derive(Clone, PartialEq)]
enum Renaming {
    Item(Item),
    /// A collection, by its path.
    Collection(String),
}

/// The tiles shown under their headings, worked out from the library
/// and how it is browsed, so the grid is built again only when they
/// change, and the collections with the one shown.
#[derive(Resource, Default, Clone, PartialEq)]
struct Shown {
    groups: Vec<Group>,
    /// The selected asset, when it has settings for the pane.
    settings: Option<Item>,
    /// The kind shown, as its name in the plural.
    plural: &'static str,
    /// The collections of the kind shown, each with how many assets
    /// are in it or the collections inside it.
    collections: Vec<(String, usize)>,
    /// How many assets of the kind shown there are.
    total: usize,
    collection: Option<String>,
    search: String,
    renaming: Option<Renaming>,
}

/// Tiles under a heading: the person's own assets of a kind, or the
/// built-in ones.
#[derive(Clone, PartialEq)]
struct Group {
    title: String,
    tiles: Vec<(Item, Stand)>,
}

/// Whether an asset stands in for another of the same kind and name.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stand {
    /// It does not, and none stands in for it.
    Alone,
    /// The person's, in place of a built-in one.
    Replaces,
    /// Built in, with the person's in its place.
    Replaced,
}

/// What was done to the library from the panel, to undo and redo.
#[derive(Resource, Default)]
struct Undo {
    /// Done, the latest last.
    done: Vec<Done>,
    /// Undone, the latest undone last.
    undone: Vec<Done>,
}

/// One thing done to the library.
enum Done {
    /// An asset moved to the trash.
    Removed(Removed),
    /// An asset brought back from the trash.
    Restored { kind: Kind, name: String },
    /// An asset moved from one collection, or none, to another.
    Moved {
        kind: Kind,
        name: String,
        from: Option<String>,
        to: Option<String>,
    },
    /// An asset renamed.
    Renamed {
        kind: Kind,
        from: String,
        to: String,
    },
}

/// The collection row a tile is dragged over this frame.
#[derive(Resource, Default)]
struct Over(Option<Entity>);

/// Marks the collection row a tile is dragged over.
#[derive(Component)]
struct DropOver;

/// Marks every collection row while a tile is carried, as a place to
/// drop it.
#[derive(Component)]
struct DropTarget;

/// The collection every collection of `kind` is inside: its shelf's
/// name, so each kind has collections of its own.
fn root(kind: Kind) -> &'static str {
    kind.shelf()
}

/// The collection `path` as the person reads it, without its kind's.
fn spoken(path: &str) -> String {
    path.split_once('/')
        .map_or(path, |(_, rest)| rest)
        .replace('/', " / ")
        .replace('_', " ")
}

/// Puts the asset `name` of `kind`, just imported, in the collection
/// shown, when that is one of `kind`'s.
pub(crate) fn imported(world: &mut World, kind: Kind, name: &str) {
    let Some(collection) =
        world.resource::<Browse>().collection.clone()
    else {
        return;
    };
    if !within(&collection, root(kind)) {
        return;
    }
    let put = world.resource_mut::<Library>().change_collections(
        |collections| collections.put(kind, name, Some(&collection)),
    );
    if let Err(error) = put {
        world.resource_mut::<Library>().note = format!("{error:#}");
    }
}

/// Whether `item` has settings of its own for the pane: a material,
/// or a mesh the library added with material slots to colour.
fn has_settings(library: &Library, item: &Item) -> bool {
    match item.kind {
        Kind::Material => true,
        Kind::Mesh => library
            .catalogue
            .0
            .get(&item.name)
            .is_some_and(|asset| !asset.slots.is_empty()),
        _ => false,
    }
}

fn show(
    library: Res<Library>,
    browse: Res<Browse>,
    mut shown: ResMut<Shown>,
) {
    if !library.is_changed() && !browse.is_changed() {
        return;
    }
    let search = browse.search.to_lowercase();
    let shelf = root(browse.kind);
    // A collection the library no longer has, or of another kind,
    // shows every asset of the kind.
    let collection = browse.collection.clone().filter(|collection| {
        library.collections.paths.contains(collection)
            && within(collection, shelf)
            && collection != shelf
    });
    let named = |own: bool| {
        library
            .items
            .iter()
            .filter(|item| item.is_own() == own)
            .map(|item| (item.kind, item.name.clone()))
            .collect::<BTreeSet<_>>()
    };
    let own = named(true);
    // A built-in mesh the person's replaces is not listed at all.
    let mut built_in = named(false);
    built_in.extend(
        Catalogue::default()
            .names()
            .into_iter()
            .map(|name| (Kind::Mesh, name)),
    );
    let mut groups = Vec::new();
    for (kind, plural) in KINDS {
        let of = |mine: bool| {
            library.items.iter().filter(move |item| {
                item.kind == kind && item.is_own() == mine
            })
        };
        if browse.kind != kind {
            continue;
        }
        for (mine, whose) in [(true, "Your"), (false, "Built-in")] {
            let tiles = of(mine)
                .filter(|item| {
                    collection.as_ref().is_none_or(|collection| {
                        library
                            .collections
                            .holds(collection, item.kind, &item.name)
                    })
                })
                .filter(|item| {
                    search.is_empty()
                        || item.name.to_lowercase().contains(&search)
                })
                .map(|item| {
                    let key = (item.kind, item.name.clone());
                    let stand = if mine && built_in.contains(&key) {
                        Stand::Replaces
                    } else if !mine && own.contains(&key) {
                        Stand::Replaced
                    } else {
                        Stand::Alone
                    };
                    (item.clone(), stand)
                })
                .collect::<Vec<_>>();
            if tiles.is_empty() {
                continue;
            }
            groups.push(Group {
                title: format!("{whose} {plural} · {}", tiles.len()),
                tiles,
            });
        }
    }
    // A selection the library no longer has is dropped.
    let settings = browse.selected.clone().filter(|selected| {
        library.items.contains(selected)
            && has_settings(&library, selected)
    });
    let next = Shown {
        groups,
        settings,
        plural: KINDS
            .iter()
            .find(|(of, _)| *of == browse.kind)
            .map_or("assets", |(_, plural)| plural),
        collections: library
            .collections
            .paths
            .iter()
            .filter(|path| within(path, shelf) && *path != shelf)
            .map(|path| {
                let held = library
                    .items
                    .iter()
                    .filter(|item| {
                        item.kind == browse.kind
                            && library
                                .collections
                                .holds(path, item.kind, &item.name)
                    })
                    .count();
                (path.clone(), held)
            })
            .collect(),
        total: library
            .items
            .iter()
            .filter(|item| item.kind == browse.kind)
            .count(),
        collection,
        search: browse.search.clone(),
        renaming: browse.renaming.clone(),
    };
    if *shown != next {
        *shown = next;
    }
}

fn view() -> impl View {
    column((
        bar(),
        with_cx(|cx| {
            frame()
                .width(percent(100.0))
                .height(px(1.0))
                .fill(cx.theme().palette.border)
        }),
        // As tall as what is left, not as its tiles, so the grid
        // and the pane scroll.
        row((
            collections(),
            sides::line::<Panel>(Side::Left),
            grid(),
            row((sides::line::<Panel>(Side::Right), settings()))
                .fills_height()
                .grow(0.0)
                .gap(0.0)
                .display(resource::<Shown, _>(|shown| {
                    if shown.settings.is_some() {
                        Display::Flex
                    } else {
                        Display::None
                    }
                })),
        ))
        .width(percent(100.0))
        .grow(1.0)
        .min_height(px(0.0))
        .gap(0.0),
    ))
    .fills_width()
    .fills_height()
    .gap(0.0)
    .with((Panel, Hovered::default()))
}

/// The one row over the grid: the kinds to show, the last thing done
/// for a moment, the search, and Import.
fn bar() -> AnyView {
    with_cx(|cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        row((
            kinds(),
            description(resource::<Library, _>(|library| {
                library.note.clone()
            })),
            frame().grow(1.0),
            search_assets(),
            import(),
        ))
        .width(percent(100.0))
        .padding(UiRect::all(px(gap)))
        .gap(gap)
        .align(AlignItems::Center)
        .fill(theme.palette.card)
    })
    .boxed()
}

/// Clears the last thing done from the bar a while after it was said.
fn fade(
    time: Res<Time>,
    mut library: ResMut<Library>,
    mut said: Local<(String, f32)>,
) {
    let now = time.elapsed_secs();
    if library.note != said.0 {
        *said = (library.note.clone(), now);
    } else if !library.note.is_empty() && now - said.1 > NOTED {
        library.note.clear();
        said.0.clear();
    }
}

/// The kind filter, with the kind shown picked.
fn kinds() -> AnyView {
    keyed(
        resource::<Browse, _>(|browse| browse.kind),
        |kind: &Kind| {
            let picked = KINDS
                .iter()
                .position(|(of, _)| of == kind)
                .unwrap_or_default();
            segmented_words(FILTERS, picked, |world, at| {
                let Some(&(kind, _)) = KINDS.get(at) else {
                    return;
                };
                let mut browse = world.resource_mut::<Browse>();
                if browse.kind != kind {
                    browse.kind = kind;
                    browse.collection = None;
                    browse.selected = None;
                    browse.renaming = None;
                }
            })
            .boxed()
        },
    )
    .within(row(()).gap(0.0))
    .boxed()
}

/// The list down the left: every asset of the kind shown, each of its
/// collections under its parent, and the button that makes a new one.
/// A row shows its collection when pressed, and puts a tile dropped
/// on it in its collection.
fn collections() -> AnyView {
    let rows = keyed(
        resource::<Shown, _>(|shown| {
            (
                shown.plural,
                shown.total,
                shown.collections.clone(),
                shown.collection.clone(),
                match &shown.renaming {
                    Some(Renaming::Collection(path)) => {
                        Some(path.clone())
                    }
                    _ => None,
                },
            )
        }),
        |(plural, total, paths, open, renaming): &(
            &'static str,
            usize,
            Vec<(String, usize)>,
            Option<String>,
            Option<String>,
        )| {
            let (plural, total, paths, open, renaming) = (
                *plural,
                *total,
                paths.clone(),
                open.clone(),
                renaming.clone(),
            );
            with_cx(move |cx| {
                let gap = cx.theme().space.gap;
                let mut rows = vec![collection_row(
                    format!("All {plural}"),
                    None,
                    0,
                    total,
                    open.is_none(),
                )];
                rows.extend(paths.iter().map(|(path, held)| {
                    // Inside the kind's own, which is not listed.
                    let depth = path.matches('/').count();
                    if renaming.as_deref() == Some(path.as_str()) {
                        return collection_naming(
                            path.clone(),
                            depth,
                        );
                    }
                    let name = path
                        .rsplit('/')
                        .next()
                        .unwrap_or(path)
                        .replace('_', " ");
                    collection_row(
                        name,
                        Some(path.clone()),
                        depth,
                        *held,
                        open.as_deref() == Some(path.as_str()),
                    )
                }));
                let parent = open.clone();
                rows.push(
                    button(
                        row((
                            glyph(Glyph::Plus, ICON),
                            label("New collection"),
                        ))
                        .gap(4.0),
                    )
                    .padding(UiRect::axes(px(8.0), px(2.0)))
                    .tooltip(|| {
                        note("Make a collection in the one shown")
                    })
                    .rules(styles::button)
                    .tagged(Variant::Default)
                    .on_activate(move |world| {
                        let parent =
                            parent.clone().unwrap_or_else(|| {
                                root(world.resource::<Browse>().kind)
                                    .to_owned()
                            });
                        new_collection(world, &parent);
                    })
                    .boxed(),
                );
                column(rows).gap(gap / 4.0).width(percent(100.0))
            })
            .boxed()
        },
    )
    .within(column(()).gap(0.0).width(percent(100.0)));
    with_cx(move |cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        scroll((column((section("COLLECTIONS"), carry_hint(), rows))
            .gap(gap)
            .padding(UiRect::all(px(gap)))
            .width(percent(100.0)),))
        .width(sides::width::<Panel>(Side::Left))
        // Only as wide as dragged, not a share of the room left: not
        // `fills_height`, which grows it across the row too.
        .grow(0.0)
        .shrink(0.0)
        .height(percent(100.0))
        .min_height(px(0.0))
        .fill(theme.palette.card)
    })
    .boxed()
}

/// What the list says while a tile is carried: where to drop it.
fn carry_hint() -> AnyView {
    row((description(resource::<Carried, _>(|carried| {
        carried.item.as_ref().map_or_else(String::new, |item| {
            format!(
                "Drop {} on a collection to put it there",
                item.name.replace('_', " ")
            )
        })
    })),))
    .display(resource::<Carried, _>(|carried| {
        if carried.item.is_some() {
            Display::Flex
        } else {
            Display::None
        }
    }))
    .boxed()
}

/// A row of the list of collections: `name`, `depth` steps in, for
/// the collection `path`, or for every asset of the kind, and how
/// many assets it holds, `count`. Lit while `open`.
fn collection_row(
    name: String,
    path: Option<String>,
    depth: usize,
    count: usize,
    open: bool,
) -> AnyView {
    let tip = match &path {
        Some(_) => {
            "Show what is in it. Drag tiles here to put them in it; \
             right-click to rename or remove it."
        }
        None => {
            "Show every asset of this kind. Drag a tile here to take \
             it out of its collection."
        }
    };
    let shows = path.clone();
    let plain = button(
        row((
            glyph(
                if path.is_some() {
                    Glyph::Folder
                } else {
                    Glyph::Book
                },
                ICON,
            )
            .tone(if open {
                Tone::Accent
            } else {
                Tone::Dim
            }),
            label(name).wrap(false).grown(1.0),
            label(count.to_string()).wrap(false).tone(Tone::Dim),
        ))
        .gap(6.0)
        .grow(1.0)
        .align(AlignItems::Center),
    )
    .width(percent(100.0))
    .justify(JustifyContent::FlexStart)
    .padding(UiRect::new(
        px(6.0 + depth as f32 * INDENT),
        px(6.0),
        px(4.0),
        px(4.0),
    ))
    .rules(row_rules);
    let marked = if open {
        plain.tagged(Picked).boxed()
    } else {
        plain.boxed()
    };
    let shown = marked
        .tooltip(move || note(tip))
        .tagged(CollectionRow(path.clone()))
        .on_activate(move |world| {
            let mut browse = world.resource_mut::<Browse>();
            browse.collection = shows.clone();
            browse.selected = None;
        });
    let Some(path) = path else {
        return shown.boxed();
    };
    shown
        .context_menu(move || {
            let (renamed, removed) = (path.clone(), path.clone());
            (
                menu_item(label("Rename")).on_activate(
                    move |world| {
                        world.resource_mut::<Browse>().renaming =
                            Some(Renaming::Collection(
                                renamed.clone(),
                            ));
                    },
                ),
                menu_item(label("Remove")).on_activate(
                    move |world| {
                        remove_collection(world, &removed);
                    },
                ),
            )
        })
        .boxed()
}

/// The row of the collection `path` while its name is typed.
fn collection_naming(path: String, depth: usize) -> AnyView {
    let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
    let from = path.clone();
    row((text_field(name, move |world, to| {
        world.resource_mut::<Browse>().renaming = None;
        let to = to.trim().replace('/', "_");
        if to.is_empty() {
            return;
        }
        let to = match from.rsplit_once('/') {
            Some((parent, _)) => format!("{parent}/{to}"),
            None => to,
        };
        rename_collection(world, &from, &to);
    })
    .width(percent(100.0))
    .rules(text_input)
    .tagged(TakesFocus),))
    .width(percent(100.0))
    .padding(UiRect::left(px(depth as f32 * INDENT)))
    .boxed()
}

/// The rules of a row of the list: bare at rest, filled under the
/// pointer, lit while it is [`Picked`], the collection shown, and
/// outlined while a tile is dragged over it.
fn row_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(Color::NONE)
                .radius(t.space.round)
                .border(1.0)
                .border_color(Color::NONE)
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.gold_hover)))
        .when::<Picked>(|s| {
            s.frame(|f, t| f.fill(t.palette.primary_low))
        })
        .when::<DropTarget>(|s| {
            s.frame(|f, t| f.border_color(t.palette.gold_6))
        })
        .when::<DropOver>(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.gold_hover)
                    .border_color(t.palette.glow)
            })
        })
        .apply(cx);
}

/// A row of the list of collections, for its collection or for every
/// asset.
#[derive(Component, Clone)]
struct CollectionRow(Option<String>);

/// Puts the tile dropped on a row of the list in the row's
/// collection, or takes it out of its own on the row for every asset.
fn dropped_on_row(drop: On<PointerDragDrop>, mut commands: Commands) {
    if drop.event_target() != drop.original_event_target() {
        return;
    }
    let (on, dropped) = (drop.event_target(), drop.dropped);
    commands.queue(move |world: &mut World| {
        let Some(item) = tile_of(world, dropped) else {
            return;
        };
        let Some((_, row)) = row_of(world, on) else {
            return;
        };
        put_in(world, &item, row.as_deref());
    });
}

/// Notes the row of the list a tile is dragged over, to light it.
fn dragged_over_row(
    over: On<PointerDragOver>,
    mut commands: Commands,
) {
    if over.event_target() != over.original_event_target() {
        return;
    }
    let (on, dragged) = (over.event_target(), over.dragged);
    commands.queue(move |world: &mut World| {
        if tile_of(world, dragged).is_none() {
            return;
        }
        if let Some((row, _)) = row_of(world, on) {
            world.resource_mut::<Over>().0 = Some(row);
        }
    });
}

/// Moves [`DropOver`] to the row a tile was dragged over this frame.
fn light(
    mut commands: Commands,
    mut over: ResMut<Over>,
    rows: Query<(Entity, Has<DropOver>), With<CollectionRow>>,
) {
    let wanted = over.0.take();
    for (entity, lit) in &rows {
        let want = wanted == Some(entity);
        if want && !lit {
            commands.entity(entity).insert(DropOver);
        } else if lit && !want {
            commands.entity(entity).remove::<DropOver>();
        }
    }
}

/// Outlines every row of the list while a tile is carried, as the
/// places to drop it.
fn carrying(
    mut commands: Commands,
    carried: Res<Carried>,
    rows: Query<(Entity, Has<DropTarget>), With<CollectionRow>>,
) {
    let carrying = carried.item.is_some();
    for (entity, marked) in &rows {
        if carrying && !marked {
            commands.entity(entity).insert(DropTarget);
        } else if marked && !carrying {
            commands.entity(entity).remove::<DropTarget>();
        }
    }
}

/// The row of the list that is `entity` or holds it, and its
/// collection.
fn row_of(
    world: &World,
    mut entity: Entity,
) -> Option<(Entity, Option<String>)> {
    loop {
        if let Some(CollectionRow(path)) =
            world.get::<CollectionRow>(entity)
        {
            return Some((entity, path.clone()));
        }
        entity = world.get::<ChildOf>(entity)?.parent();
    }
}

/// Puts `item` in the collection `path`, or in none, to be undone.
fn put_in(world: &mut World, item: &Item, path: Option<&str>) {
    let from = world
        .resource::<Library>()
        .collections
        .of(item.kind, &item.name)
        .map(str::to_owned);
    if from.as_deref() == path {
        return;
    }
    if let Some(path) = path
        && !within(path, root(item.kind))
    {
        world.resource_mut::<Library>().note = format!(
            "{} cannot go in a collection of another kind",
            item.name.replace('_', " ")
        );
        return;
    }
    match moved(world, item.kind, &item.name, path) {
        Ok(note) => {
            world.resource_mut::<Library>().note = note;
            did(
                world,
                Done::Moved {
                    kind: item.kind,
                    name: item.name.clone(),
                    from,
                    to: path.map(str::to_owned),
                },
            );
        }
        Err(error) => {
            world.resource_mut::<Library>().note =
                format!("{error:#}");
        }
    }
}

/// Puts the asset `name` of `kind` in the collection `path`, or in
/// none, and says so.
fn moved(
    world: &mut World,
    kind: Kind,
    name: &str,
    path: Option<&str>,
) -> anyhow::Result<String> {
    world.resource_mut::<Library>().change_collections(
        |collections| collections.put(kind, name, path),
    )?;
    let spoken = name.replace('_', " ");
    Ok(match path {
        Some(path) => {
            format!("Put {spoken} in {}", self::spoken(path))
        }
        None => format!("Took {spoken} out of its collection"),
    })
}

/// The search over every asset's name.
fn search_assets() -> AnyView {
    search("Search assets", SEARCH, |world, text| {
        let mut browse = world.resource_mut::<Browse>();
        if browse.search != text {
            browse.search = text;
        }
    })
}

/// The button that adds `.glb` models as meshes and `.png` pictures
/// as textures, as dropping them on the window does.
fn import() -> AnyView {
    action(Glyph::Lower, "Import", crate::graphs::assets::import)
        .tooltip(|| {
            note(
                "Add .glb models as meshes and .png pictures as \
                 textures, into the collection shown; dropping them \
                 on the window does too",
            )
        })
        .boxed()
}

/// Makes a new collection in `parent`, shows it, and has its name
/// typed.
fn new_collection(world: &mut World, parent: &str) {
    let made = world.resource_mut::<Library>().change_collections(
        |collections| {
            let path = collections.free(Some(parent), "collection");
            collections.add(&path)
        },
    );
    match made {
        Ok(path) => {
            let mut browse = world.resource_mut::<Browse>();
            browse.collection = Some(path.clone());
            browse.renaming = Some(Renaming::Collection(path));
        }
        Err(error) => {
            world.resource_mut::<Library>().note =
                format!("{error:#}");
        }
    }
}

/// Gives the collection `from` the path `to`, and keeps showing it.
fn rename_collection(world: &mut World, from: &str, to: &str) {
    if from == to {
        return;
    }
    let renamed = world.resource_mut::<Library>().change_collections(
        |collections| collections.rename(from, to),
    );
    let note = match renamed {
        Ok(path) => {
            let note = format!(
                "Renamed the collection to {}",
                spoken(&path)
            );
            world.resource_mut::<Browse>().collection = Some(path);
            note
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Library>().note = note;
}

/// Takes the collection `path` away and shows the one above it.
fn remove_collection(world: &mut World, path: &str) {
    let removed = world
        .resource_mut::<Library>()
        .change_collections(|collections| collections.remove(path));
    let note = match removed {
        Ok(()) => {
            // The kind's own is shown as every asset of it.
            world.resource_mut::<Browse>().collection = path
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_owned())
                .filter(|parent| parent.contains('/'));
            format!("Took the collection {} away", spoken(path))
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Library>().note = note;
}

/// The file `item` is kept in, shown in its folder, for one of the
/// person's own; none for a built-in one.
fn show_file(world: &mut World, item: &Item) {
    if let Source::File(path) = &item.source {
        open::reveal(path);
    } else {
        world.resource_mut::<Library>().note = format!(
            "{} is built in, with no file of its own",
            item.name
        );
    }
}

/// The tiles shown under their headings, each group wrapped in rows.
fn grid() -> AnyView {
    // Built again for what is shown, never for what is selected.
    scroll((keyed(
        resource::<Shown, _>(|shown| {
            (
                shown.groups.clone(),
                shown.search.clone(),
                shown.collection.is_some(),
                match &shown.renaming {
                    Some(Renaming::Item(item)) => Some(item.clone()),
                    _ => None,
                },
            )
        }),
        |(groups, searched, in_collection, renaming): &(
            Vec<Group>,
            String,
            bool,
            Option<Item>,
        )| {
            let groups = groups
                .iter()
                .map(|group| {
                    let tiles = group
                        .tiles
                        .iter()
                        .map(|(item, stand)| {
                            let naming =
                                renaming.as_ref() == Some(item);
                            tile(item.clone(), *stand, naming)
                        })
                        .collect::<Vec<_>>();
                    (group.title.clone(), tiles)
                })
                .collect::<Vec<_>>();
            let (searched, in_collection) =
                (searched.clone(), *in_collection);
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                if groups.is_empty() {
                    let words = if !searched.is_empty() {
                        format!(
                            "No asset is named like “{searched}”."
                        )
                    } else if in_collection {
                        "Nothing in this collection yet. Import into \
                         it, or open the top row of the list and \
                         drag tiles onto it."
                            .to_owned()
                    } else {
                        "Nothing here yet. Import a .glb model or a \
                         .png picture to start."
                            .to_owned()
                    };
                    return cx.build(
                        column((description(words),))
                            .padding(UiRect::all(px(gap * 2.0))),
                    );
                }
                let mut parts = Vec::new();
                for (title, tiles) in groups {
                    parts.push(
                        column((section(title.to_uppercase()),))
                            .padding(UiRect::new(
                                px(0.0),
                                px(0.0),
                                px(gap),
                                px(0.0),
                            ))
                            .boxed(),
                    );
                    parts.push(wrapped(tiles, gap));
                }
                cx.build(
                    column(parts)
                        .gap(gap / 2.0)
                        .padding(UiRect::all(px(gap)))
                        .width(percent(100.0)),
                )
            })
        },
    ),))
    .grow(1.0)
    .min_width(px(0.0))
    .fills_height()
    .boxed()
}

/// Where `item` comes from, in words: its kind, or that it is built
/// in.
fn origin(item: &Item) -> String {
    match &item.source {
        Source::File(_) => {
            format!("Your {}", item.kind.word().to_lowercase())
        }
        Source::BuiltIn | Source::Example(_) => {
            format!("Built-in {}", item.kind.word().to_lowercase())
        }
    }
}

/// What a tile's mark says of `stand`, none where there is nothing
/// to say.
fn stand_words(stand: Stand) -> Option<&'static str> {
    match stand {
        Stand::Alone => None,
        Stand::Replaces => Some("Replaces built-in"),
        Stand::Replaced => Some("Replaced"),
    }
}

/// The rules of a tile: no border at rest, a gold one under the
/// pointer, and lit while it is [`Picked`].
fn tile_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(Color::NONE)
                .border(1.0)
                .border_color(Color::NONE)
                .radius(t.space.round)
        })
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.card).border_color(t.palette.gold_5)
            })
        })
        .when::<Picked>(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.text_selection)
                    .border_color(t.palette.glow)
            })
        })
        .apply(cx);
}

/// Marks the tile of the selected asset. Selecting another moves the
/// mark, so the grid is not built again, nor its names cut again.
#[derive(Component)]
struct Picked;

/// Moves [`Picked`] to the tile of the asset selected.
fn pick(
    mut commands: Commands,
    browse: Res<Browse>,
    tiles: Query<(Entity, &Tile, Has<Picked>)>,
) {
    for (entity, Tile(item), picked) in &tiles {
        let wanted = browse.selected.as_ref() == Some(item);
        if wanted && !picked {
            commands.entity(entity).insert(Picked);
        } else if picked && !wanted {
            commands.entity(entity).remove::<Picked>();
        }
    }
}

/// Whether `item` opens in an editor on a double click.
fn opens(item: &Item) -> bool {
    matches!(item.kind, Kind::Blueprint | Kind::Shader | Kind::Prefab)
}

/// One asset: its picture, or its kind's glyph, its name, or a field
/// for it while `naming`, and a mark where it replaces a built-in one
/// or is replaced.
fn tile(item: Item, stand: Stand, naming: bool) -> AnyView {
    let for_menu = item.clone();
    with_cx(move |cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        let palette = &theme.palette;
        let drawn = match item.kind {
            Kind::Mesh | Kind::Blueprint | Kind::Shader => {
                picture(item.kind, &item.name, PICTURE)
            }
            kind => {
                column((glyph(kind.glyph(), 32.0).tone(Tone::Faint),))
                    .width(px(PICTURE))
                    .height(px(PICTURE))
                    .align(AlignItems::Center)
                    .justify(JustifyContent::Center)
                    .fill(palette.input)
                    .radius(theme.space.round)
                    .boxed()
            }
        };
        let mark: AnyView = match stand_words(stand) {
            Some(words) => row((label(words)
                .size(theme.text.small)
                .bold(true)
                .tone(Tone::Accent)
                .wrap(false),))
            .position(PositionType::Absolute)
            .inset(UiRect::new(
                px(2.0),
                Val::Auto,
                px(2.0),
                Val::Auto,
            ))
            .padding(UiRect::axes(px(4.0), px(1.0)))
            .fill(palette.primary_low)
            .radius(theme.space.round)
            .boxed(),
            None => column(()).boxed(),
        };
        let spoken = item.name.replace('_', " ");
        let mut tip = match stand_words(stand) {
            Some(words) => {
                format!("{spoken}\n{} · {words}", origin(&item))
            }
            None => format!("{spoken}\n{}", origin(&item)),
        };
        if opens(&item) {
            tip.push_str("\nDouble-click to open it");
        }
        let name: AnyView = if naming {
            let renamed = item.clone();
            text_field(item.name.clone(), move |world, name| {
                world.resource_mut::<Browse>().renaming = None;
                rename(world, &renamed, &name);
            })
            .width(percent(100.0))
            .rules(text_input)
            .tagged(TakesFocus)
            .boxed()
        } else {
            centred(spoken)
        };
        column((
            column((drawn, mark)).gap(0.0),
            column((name,))
                .width(percent(100.0))
                .height(px(NAME))
                .overflow(Overflow::clip())
                .align(AlignItems::Center),
        ))
        .width(px(TILE))
        .padding(UiRect::all(px(gap / 2.0)))
        .gap(4.0)
        .align(AlignItems::Center)
        .rules(tile_rules)
        .tooltip(move || note(tip.clone()))
        .tagged(Tile(item.clone()))
        .tagged(EntityCursor(SystemCursorIcon::Grab))
        .context_menu(move || menu(&for_menu))
    })
    .boxed()
}

/// A tile's right-click menu: open it where it opens, and for one of
/// the person's own, rename it, show its file or delete it.
fn menu(item: &Item) -> Vec<AnyView> {
    let mut entries = Vec::new();
    if opens(item) {
        let opened = item.clone();
        entries.push(
            menu_item(label("Open"))
                .on_activate(move |world| open(world, &opened))
                .boxed(),
        );
    }
    if item.is_own() {
        let (renamed, shown, removed) =
            (item.clone(), item.clone(), item.clone());
        entries.push(
            menu_item(label("Rename      F2"))
                .on_activate(move |world| {
                    world.resource_mut::<Browse>().renaming =
                        Some(Renaming::Item(renamed.clone()));
                })
                .boxed(),
        );
        entries.push(
            menu_item(label("Show file"))
                .on_activate(move |world| show_file(world, &shown))
                .boxed(),
        );
        entries.push(
            menu_item(label("Delete      Del"))
                .on_activate(move |world| remove(world, &removed))
                .boxed(),
        );
    }
    entries
}

/// A press on a tile: it is selected, and opened on a second press.
fn pressed(
    press: On<PointerPress>,
    tiles: Query<&Tile>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(Tile(item)) = tiles.get(press.event_target()) else {
        return;
    };
    let item = item.clone();
    let twice = press.count == 2;
    commands.queue(move |world: &mut World| {
        let mut browse = world.resource_mut::<Browse>();
        if browse.renaming.as_ref().is_some_and(|naming| {
            *naming != Renaming::Item(item.clone())
        }) {
            browse.renaming = None;
        }
        browse.selected = Some(item.clone());
        if twice && opens(&item) {
            open(world, &item);
        }
    });
}

/// The keys of the panel, while the pointer is on it and nothing is
/// typed in: Delete moves the selected asset to the trash, F2 renames
/// it, Ctrl+Z undoes and Ctrl+Y or Ctrl+Shift+Z redoes. Escape stops
/// renaming, wherever the pointer is.
fn keys(world: &mut World) {
    let keys = world.resource::<ButtonInput<KeyCode>>();
    if keys.just_pressed(KeyCode::Escape)
        && world.resource::<Browse>().renaming.is_some()
    {
        world.resource_mut::<InputFocus>().clear();
        world.resource_mut::<Browse>().renaming = None;
        return;
    }
    if world.resource::<InputFocus>().get().is_some()
        || !world
            .query_filtered::<&Hovered, With<Panel>>()
            .iter(world)
            .any(Hovered::get)
    {
        return;
    }
    let keys = world.resource::<ButtonInput<KeyCode>>();
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    let shift =
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let delete = keys.just_pressed(KeyCode::Delete);
    let rename = keys.just_pressed(KeyCode::F2);
    let undo = command && keys.just_pressed(KeyCode::KeyZ);
    let redo = command && keys.just_pressed(KeyCode::KeyY);
    let selected = world.resource::<Browse>().selected.clone();
    if undo || redo {
        step(world, redo || shift);
    } else if let Some(item) = selected {
        if delete {
            remove(world, &item);
        } else if rename && item.is_own() {
            world.resource_mut::<Browse>().renaming =
                Some(Renaming::Item(item));
        }
    }
}

/// Keeps `done` to undo, and forgets what was undone.
fn did(world: &mut World, done: Done) {
    let mut undo = world.resource_mut::<Undo>();
    undo.done.push(done);
    undo.undone.clear();
}

/// Undoes the last thing done, or redoes the last undone with `redo`.
fn step(world: &mut World, redo: bool) {
    let popped = {
        let mut undo = world.resource_mut::<Undo>();
        if redo {
            undo.undone.pop()
        } else {
            undo.done.pop()
        }
    };
    let Some(done) = popped else {
        world.resource_mut::<Library>().note = if redo {
            "Nothing to redo"
        } else {
            "Nothing to undo"
        }
        .to_owned();
        return;
    };
    match reverse(world, done) {
        Ok((back, note)) => {
            let mut undo = world.resource_mut::<Undo>();
            if redo {
                undo.done.push(back);
            } else {
                undo.undone.push(back);
            }
            world.resource_mut::<Library>().note = note;
        }
        Err(error) => {
            world.resource_mut::<Library>().note =
                format!("{error:#}");
        }
    }
}

/// Does the opposite of `done`, and gives what was done then, whose
/// opposite is `done` again, and what to say of it.
fn reverse(
    world: &mut World,
    done: Done,
) -> anyhow::Result<(Done, String)> {
    match done {
        Done::Removed(removed) => {
            let item =
                world.resource_mut::<Library>().restore(&removed)?;
            let note = format!("Brought {} back", item.name);
            world.resource_mut::<Browse>().selected =
                Some(item.clone());
            Ok((
                Done::Restored {
                    kind: item.kind,
                    name: item.name,
                },
                note,
            ))
        }
        Done::Restored { kind, name } => {
            let item = world
                .resource::<Library>()
                .find(kind, &name)
                .filter(Item::is_own)
                .ok_or_else(|| anyhow::anyhow!("{name} is gone"))?;
            let removed =
                world.resource_mut::<Library>().remove(&item)?;
            Ok((
                Done::Removed(removed),
                format!("Moved {name} to the trash"),
            ))
        }
        Done::Moved {
            kind,
            name,
            from,
            to,
        } => {
            let note = moved(world, kind, &name, from.as_deref())?;
            Ok((
                Done::Moved {
                    kind,
                    name,
                    from: to,
                    to: from,
                },
                note,
            ))
        }
        Done::Renamed { kind, from, to } => {
            let item = world
                .resource::<Library>()
                .find(kind, &to)
                .filter(Item::is_own)
                .ok_or_else(|| anyhow::anyhow!("{to} is gone"))?;
            let name = world
                .resource_mut::<Library>()
                .rename(&item, &from)?;
            crate::graphs::prefab::renamed(world, kind, &to, &name);
            let note = format!("Renamed {to} back to {name}");
            Ok((
                Done::Renamed {
                    kind,
                    from: to,
                    to: name,
                },
                note,
            ))
        }
    }
}

/// The selected asset's settings beside the grid: a material's
/// shader and values, or a mesh's slot colours.
fn settings() -> AnyView {
    let pane = keyed(
        resource::<Shown, _>(|shown| shown.settings.clone()),
        |selected: &Option<Item>| {
            let selected = selected.clone();
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                let Some(item) = selected.clone() else {
                    return cx.build(column(()));
                };
                let (body, preview): (AnyView, AnyView) =
                    match item.kind {
                        Kind::Material => (
                            material::details(item.name.clone()),
                            material::preview(),
                        ),
                        _ => (
                            crate::graphs::assets::details(
                                item.name.clone(),
                            ),
                            column(()).boxed(),
                        ),
                    };
                cx.build(
                    column((
                        row((
                            section(item.name.replace('_', " ")),
                            frame().grow(1.0),
                            button(glyph(Glyph::Cross, ICON))
                                .padding(UiRect::all(px(4.0)))
                                .tooltip(|| note("Close"))
                                .rules(styles::button)
                                .tagged(Variant::Default)
                                .on_activate(|world| {
                                    world
                                        .resource_mut::<Browse>()
                                        .selected = None;
                                }),
                        ))
                        .width(percent(100.0))
                        .align(AlignItems::Center),
                        preview,
                        body,
                    ))
                    .gap(gap),
                )
            })
        },
    )
    .within(column(()).gap(0.0));
    with_cx(move |cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        scroll((column((pane,))
            .gap(gap)
            .padding(UiRect::all(px(gap))),))
        .width(sides::width::<Panel>(Side::Right))
        .grow(0.0)
        .shrink(0.0)
        .height(percent(100.0))
        .min_height(px(0.0))
        .fill(theme.palette.card)
        .border(1.0)
        .border_color(theme.palette.border)
    })
    .boxed()
}

fn open(world: &mut World, item: &Item) {
    let note = match crate::graphs::open_item(world, item) {
        Ok(()) => String::new(),
        Err(refusal) => refusal.to_string(),
    };
    world.resource_mut::<Library>().note = note;
}

/// Gives `item` the name `name`, to be undone.
fn rename(world: &mut World, item: &Item, name: &str) {
    let name = name.trim();
    if name.is_empty() || name == item.name {
        return;
    }
    let renamed = world.resource_mut::<Library>().rename(item, name);
    match renamed {
        Ok(name) => {
            crate::graphs::prefab::renamed(
                world, item.kind, &item.name, &name,
            );
            world.resource_mut::<Library>().note =
                format!("Renamed to {}", name.replace('_', " "));
            world.resource_mut::<Browse>().selected =
                world.resource::<Library>().find(item.kind, &name);
            did(
                world,
                Done::Renamed {
                    kind: item.kind,
                    from: item.name.clone(),
                    to: name,
                },
            );
        }
        Err(error) => {
            world.resource_mut::<Library>().note =
                format!("{error:#}");
        }
    }
}

/// Moves `item` to the library's trash, to be undone.
fn remove(world: &mut World, item: &Item) {
    if !item.is_own() {
        world.resource_mut::<Library>().note = format!(
            "{} is built in and cannot be deleted",
            item.name.replace('_', " ")
        );
        return;
    }
    let removed = world.resource_mut::<Library>().remove(item);
    match removed {
        Ok(removed) => {
            world.resource_mut::<Library>().note = format!(
                "Deleted {}. Ctrl+Z brings it back",
                item.name.replace('_', " ")
            );
            world.resource_mut::<Browse>().selected = None;
            did(world, Done::Removed(removed));
        }
        Err(error) => {
            world.resource_mut::<Library>().note =
                format!("{error:#}");
        }
    }
}
