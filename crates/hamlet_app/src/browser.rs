//! The ASSETS panel: the library as a grid of tiles under a heading
//! for each kind, the person's own before the built-in ones, filtered
//! by kind and by the collection picked in the list beside it and
//! searched by name, with the selected asset's details on the other
//! side, or the library's folder while none is. A tile of the
//! person's that stands in for a built-in one of the same name says
//! so, and so does the built-in one.
//! A click selects a tile and a double click opens it; a tile dragged
//! onto a canvas adds what it is there, and onto a collection puts it
//! in it. The details rename, open, show the file of or remove the
//! selected asset, give a mesh's material slots their
//! colours, and give a material its shader and the values of that
//! shader's inputs. Blueprints, shaders and materials are made new
//! from the New menu, meshes are added from `.glb` files and textures
//! from `.png` files.

mod material;

use std::collections::BTreeSet;
use std::path::Path;

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::picking::events::{PointerDragDrop, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::ui::{
    AlignItems, Display, JustifyContent, Overflow, PositionType,
    UiRect, Val, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, TooltipExt, button,
    column, frame, label, menu_button, menu_item, row, scroll,
    text_field,
};
use bevy_fynix::{
    EntityCursor, ScopedExt, ViewExt, keyed, resource, style,
};
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_desktop::open;
use hamlet_graph::Catalogue;
use hamlet_graph_editor::files::{self, Files};
use hamlet_graph_editor::library::carry::tile_of;
use hamlet_graph_editor::library::picker::{
    centred, picture, wrapped,
};
use hamlet_graph_editor::library::search::search;
use hamlet_graph_editor::library::{
    Item, Kind, Library, Source, Tile,
};
use hamlet_ui::styles::{
    self, Fills, ICON, action, description, labelled, note, section,
    segmented_words, text_input,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::assets_folder::{in_realm, kept_at};
use crate::dock::{PanelAppExt, flush_panel};
use crate::graphs::{Blueprint, ShaderGraph};

/// A tile's width.
const TILE: f32 = 100.0;
/// The picture's side within a tile.
const PICTURE: f32 = 80.0;
/// The width of the selected asset's details beside the grid.
const DETAILS: f32 = 300.0;
/// The width of the search field.
const SEARCH: f32 = 180.0;
/// The height of a tile's name: two lines of a note.
const NAME: f32 = 32.0;
/// The width of the list of collections.
const LIST: f32 = 200.0;
/// How far a collection's row is set in for each collection above.
const INDENT: f32 = 14.0;

/// The kinds, in the order the grid shows them, each with its name in
/// the plural.
const KINDS: [(Kind, &str); 5] = [
    (Kind::Blueprint, "blueprints"),
    (Kind::Shader, "shaders"),
    (Kind::Material, "materials"),
    (Kind::Mesh, "meshes"),
    (Kind::Texture, "textures"),
];

/// The words of the kind filter: every kind, then each of [`KINDS`].
const FILTERS: [&str; 6] = [
    "All",
    "Blueprints",
    "Shaders",
    "Materials",
    "Meshes",
    "Textures",
];

/// What the New menu makes, in order.
const NEW: [&str; 5] = [
    "Blueprint",
    "Shader",
    "Material",
    "Mesh from a .glb file…",
    "Texture from a .png file…",
];

pub(crate) fn plugin(app: &mut App) {
    app.add_panel(
        "assets",
        DockWindowKind::new("ASSETS", || flush_panel(view()))
            .icon(Glyph::Folder.image(ICON)),
    )
    .add_plugins(material::plugin)
    .init_resource::<Browse>()
    .init_resource::<Shown>()
    .init_resource::<Importer>()
    .add_observer(chosen)
    .add_observer(pressed)
    .add_observer(dropped_on_row)
    .add_systems(Update, (show, pick));
}

/// How the library is browsed: the kind shown, none for every kind,
/// the collection shown, none for every asset, what is searched for,
/// and the tile selected.
#[derive(Resource, Default)]
struct Browse {
    kind: Option<Kind>,
    collection: Option<String>,
    search: String,
    selected: Option<Item>,
}

/// The tiles shown under their headings, worked out from the library
/// and how it is browsed, so the grid is built again only when they
/// change, and the collections with the one shown.
#[derive(Resource, Default, Clone, PartialEq)]
struct Shown {
    groups: Vec<Group>,
    selected: Option<Item>,
    collections: Vec<String>,
    collection: Option<String>,
    search: String,
    /// How many of the person's own assets there are of each kind,
    /// in the order of [`KINDS`].
    counts: Vec<usize>,
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

/// What the dialog for adding a texture answers to. Meshes are added
/// by `graphs::assets::import`, several at a time.
#[derive(Resource, Default)]
struct Importer(Option<Entity>);

fn show(
    library: Res<Library>,
    browse: Res<Browse>,
    mut shown: ResMut<Shown>,
) {
    if !library.is_changed() && !browse.is_changed() {
        return;
    }
    let search = browse.search.to_lowercase();
    // A collection the library no longer has shows every asset.
    let collection = browse.collection.clone().filter(|collection| {
        library.collections.paths.contains(collection)
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
    let mut counts = Vec::new();
    for (kind, plural) in KINDS {
        let of = |mine: bool| {
            library.items.iter().filter(move |item| {
                item.kind == kind && item.is_own() == mine
            })
        };
        counts.push(of(true).count());
        if browse.kind.is_some_and(|wanted| wanted != kind) {
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
    let selected = browse
        .selected
        .clone()
        .filter(|selected| library.items.contains(selected));
    let next = Shown {
        groups,
        selected,
        collections: library
            .collections
            .paths
            .iter()
            .cloned()
            .collect(),
        collection,
        search: browse.search.clone(),
        counts,
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
        // and the details scroll.
        row((collections(), grid(), details()))
            .width(percent(100.0))
            .grow(1.0)
            .min_height(px(0.0))
            .gap(0.0),
    ))
    .fills_width()
    .fills_height()
    .gap(0.0)
}

/// The one row over the grid: the kinds to show, the search, and the
/// New menu.
fn bar() -> AnyView {
    with_cx(|cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        row((kinds(), frame().grow(1.0), search_assets(), new_menu()))
            .width(percent(100.0))
            .padding(UiRect::all(px(gap)))
            .gap(gap)
            .align(AlignItems::Center)
            .fill(theme.palette.card)
    })
    .boxed()
}

/// The kind filter, with the kind shown picked.
fn kinds() -> AnyView {
    keyed(
        resource::<Browse, _>(|browse| browse.kind),
        |kind: &Option<Kind>| {
            let picked = kind.map_or(0, |kind| {
                KINDS
                    .iter()
                    .position(|(of, _)| *of == kind)
                    .map_or(0, |at| at + 1)
            });
            segmented_words(FILTERS, picked, |world, at| {
                world.resource_mut::<Browse>().kind =
                    at.checked_sub(1).map(|at| KINDS[at].0);
            })
            .boxed()
        },
    )
    .within(row(()).gap(0.0))
    .boxed()
}

/// The list down the left: the button that shows the assets folder
/// on disk, every asset, each collection under its parent, and the
/// button that makes a new one. A row shows its collection when
/// pressed, and puts a tile dropped on it in its collection.
fn collections() -> AnyView {
    let rows = keyed(
        resource::<Shown, _>(|shown| {
            (shown.collections.clone(), shown.collection.clone())
        }),
        |(paths, open): &(Vec<String>, Option<String>)| {
            let (paths, open) = (paths.clone(), open.clone());
            with_cx(move |cx| {
                let gap = cx.theme().space.gap;
                let mut rows = vec![collection_row(
                    "All assets".to_owned(),
                    None,
                    0,
                    open.is_none(),
                )];
                rows.extend(paths.iter().map(|path| {
                    let depth = path.matches('/').count();
                    let name = path
                        .rsplit('/')
                        .next()
                        .unwrap_or(path)
                        .replace('_', " ");
                    collection_row(
                        name,
                        Some(path.clone()),
                        depth + 1,
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
                        new_collection(world, parent.as_deref());
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
        scroll((column((
            show_folder(),
            section("COLLECTIONS"),
            rows,
        ))
        .gap(gap)
        .padding(UiRect::all(px(gap)))
        .width(percent(100.0)),))
        .width(px(LIST))
        .min_width(px(LIST))
        .shrink(0.0)
        .fills_height()
        .fill(theme.palette.card)
    })
    .boxed()
}

/// A row of the list of collections: `name`, `depth` steps in, for
/// the collection `path`, or for every asset. Lit while `open`.
fn collection_row(
    name: String,
    path: Option<String>,
    depth: usize,
    open: bool,
) -> AnyView {
    let tip = match &path {
        Some(_) => "Show it; drop a tile here to put it in it",
        None => {
            "Show every asset; drop a tile here to take it out of \
             its collection"
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
            label(name),
        ))
        .gap(6.0)
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
    marked
        .tooltip(move || note(tip))
        .tagged(CollectionRow(path.clone()))
        .on_activate(move |world| {
            let mut browse = world.resource_mut::<Browse>();
            browse.collection = shows.clone();
            browse.selected = None;
        })
        .context_menu(move || {
            let removed = path.clone();
            (menu_item(label("Remove")).on_activate(move |world| {
                if let Some(path) = &removed {
                    remove_collection(world, path);
                }
            }),)
        })
        .boxed()
}

/// The rules of a row of the list: bare at rest, filled under the
/// pointer, and lit while it is [`Picked`], the collection shown.
fn row_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| f.fill(Color::NONE).radius(t.space.round))
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.gold_hover)))
        .when::<Picked>(|s| {
            s.frame(|f, t| f.fill(t.palette.primary_low))
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
        let Some(row) = row_of(world, on) else {
            return;
        };
        put_in(world, &item, row.as_deref());
    });
}

/// The collection of the row that is `entity` or holds it.
fn row_of(
    world: &World,
    mut entity: Entity,
) -> Option<Option<String>> {
    loop {
        if let Some(CollectionRow(path)) =
            world.get::<CollectionRow>(entity)
        {
            return Some(path.clone());
        }
        entity = world.get::<ChildOf>(entity)?.parent();
    }
}

/// Puts `item` in the collection `path`, or in none.
fn put_in(world: &mut World, item: &Item, path: Option<&str>) {
    let put = world.resource_mut::<Library>().change_collections(
        |collections| collections.put(item.kind, &item.name, path),
    );
    world.resource_mut::<Library>().note = match (put, path) {
        (Ok(()), Some(path)) => {
            format!(
                "Put {} in {}",
                item.name,
                path.replace('/', " / ")
            )
        }
        (Ok(()), None) => {
            format!("Took {} out of its collection", item.name)
        }
        (Err(error), _) => format!("{error:#}"),
    };
}

/// The button that shows the folder the library keeps its files in.
fn show_folder() -> AnyView {
    action(Glyph::Eye, "Show folder", |world| {
        if let Some(root) =
            world.resource::<Library>().root().map(Path::to_owned)
            && std::fs::create_dir_all(&root).is_ok()
        {
            open::show_file(&root);
        }
    })
    .tooltip(|| note("Open the folder your assets are kept in"))
    .boxed()
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

/// The menu that makes a new blueprint, shader or material, or adds a
/// mesh or a texture from a file.
fn new_menu() -> AnyView {
    with_cx(|cx| {
        let theme = cx.theme();
        menu_button("+ New", NEW, |world, at| {
            let made = match at {
                0 => {
                    files::create::<Blueprint>(world);
                    world.resource::<Files<Blueprint>>().note.clone()
                }
                1 => {
                    files::create::<ShaderGraph>(world);
                    world
                        .resource::<Files<ShaderGraph>>()
                        .note
                        .clone()
                }
                2 => new_material(world),
                3 => {
                    crate::graphs::assets::import(world);
                    return;
                }
                _ => {
                    import_texture(world);
                    return;
                }
            };
            world.resource_mut::<Library>().note = made;
        })
        .padding(UiRect::axes(px(10.0), px(4.0)))
        .fill(theme.palette.button)
        .border(theme.space.border)
        .border_color(theme.palette.gold_5)
        .radius(theme.space.round)
    })
    .boxed()
}

/// Makes a new collection in `parent`, or at the top, and shows it.
fn new_collection(world: &mut World, parent: Option<&str>) {
    let made = world.resource_mut::<Library>().change_collections(
        |collections| {
            let path = collections.free(parent, "collection");
            collections.add(&path)
        },
    );
    let note = match made {
        Ok(path) => {
            let note = format!("Made the collection {path}");
            world.resource_mut::<Browse>().collection = Some(path);
            note
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Library>().note = note;
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
            let note = format!("Renamed the collection to {path}");
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
            world.resource_mut::<Browse>().collection = path
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_owned());
            format!("Took the collection {path} away")
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Library>().note = note;
}

/// The collection `item` is in, with a button that takes it out, or
/// how to put it in one. Built again whenever the library changes.
fn collection_of(item: &Item) -> AnyView {
    let item = item.clone();
    keyed(
        resource::<Library, _>(|library| library.revision),
        move |_: &u64| {
            let item = item.clone();
            with_cx(move |cx| {
                let gap = cx.theme().space.gap;
                let library = cx.world.resource::<Library>();
                let Some(path) = library
                    .collections
                    .of(item.kind, &item.name)
                    .map(str::to_owned)
                else {
                    return column((description(
                        "In none. Drag its tile onto a collection \
                         to put it in one.",
                    ),))
                    .boxed();
                };
                let taken = item.clone();
                row((
                    note(path.replace('/', " / ")),
                    frame().grow(1.0),
                    button(label("Take out"))
                        .padding(UiRect::axes(px(8.0), px(2.0)))
                        .rules(styles::button)
                        .tagged(Variant::Default)
                        .on_activate(move |world| {
                            put_in(world, &taken, None);
                        }),
                ))
                .gap(gap)
                .align(AlignItems::Center)
                .width(percent(100.0))
                .boxed()
            })
            .boxed()
        },
    )
    .within(column(()).gap(0.0).width(percent(100.0)))
    .boxed()
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
            (shown.groups.clone(), shown.search.clone())
        }),
        |(groups, searched): &(Vec<Group>, String)| {
            let groups = groups
                .iter()
                .map(|group| {
                    let tiles = group
                        .tiles
                        .iter()
                        .map(|(item, stand)| {
                            tile(item.clone(), *stand)
                        })
                        .collect::<Vec<_>>();
                    (group.title.clone(), tiles)
                })
                .collect::<Vec<_>>();
            let searched = searched.clone();
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                if groups.is_empty() {
                    let words = if searched.is_empty() {
                        "Nothing here yet. Make a blueprint, shader \
                         or material from New, or add a mesh or a \
                         texture from a file."
                            .to_owned()
                    } else {
                        format!(
                            "No asset is named like “{searched}”."
                        )
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

/// One asset: its picture, or its kind's glyph, its name on one line,
/// and a mark where it replaces a built-in one or is replaced.
fn tile(item: Item, stand: Stand) -> AnyView {
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
        let name = item.name.replace('_', " ");
        let tip = match stand_words(stand) {
            Some(words) => {
                format!("{name}\n{} · {words}", origin(&item))
            }
            None => format!("{name}\n{}", origin(&item)),
        };
        column((
            column((drawn, mark)).gap(0.0),
            column((centred(name),))
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
        .context_menu(move || {
            let (open_it, show_it, remove_it) = (
                for_menu.clone(),
                for_menu.clone(),
                for_menu.clone(),
            );
            (
                menu_item(label("Open"))
                    .on_activate(move |world| open(world, &open_it)),
                menu_item(label("Show file")).on_activate(
                    move |world| show_file(world, &show_it),
                ),
                menu_item(label("Remove")).on_activate(
                    move |world| remove(world, &remove_it),
                ),
            )
        })
    })
    .boxed()
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
        world.resource_mut::<Browse>().selected = Some(item.clone());
        if twice {
            open(world, &item);
        }
    });
}

/// The library while no asset is selected: where it is kept, whether
/// the realm builds with it, how many assets of each kind it has, and
/// the collection shown to rename.
fn library() -> AnyView {
    keyed(
        resource::<Shown, _>(|shown| {
            (shown.counts.clone(), shown.collection.clone())
        }),
        |(counts, collection): &(Vec<usize>, Option<String>)| {
            let counts = counts.clone();
            let collection = collection.clone();
            with_cx(move |cx| {
                let gap = cx.theme().space.gap;
                let yours = KINDS
                    .iter()
                    .zip(&counts)
                    .filter(|(_, count)| **count > 0)
                    .map(|((_, plural), count)| {
                        format!("{count} {plural}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut parts: Vec<AnyView> = vec![
                    section("YOUR ASSETS").boxed(),
                    kept_at(),
                    description(if yours.is_empty() {
                        "None yet. Make one from New, or save a \
                         built-in one under the same name to change \
                         it."
                        .to_owned()
                    } else {
                        yours
                    })
                    .boxed(),
                    section("IN THE REALM").boxed(),
                    in_realm(),
                ];
                if let Some(path) = collection.clone() {
                    let from = path.clone();
                    parts.push(section("COLLECTION SHOWN").boxed());
                    parts.push(
                        text_field(path, move |world, to| {
                            rename_collection(world, &from, &to);
                        })
                        .width(percent(100.0))
                        .rules(text_input)
                        .boxed(),
                    );
                }
                column(parts).gap(gap)
            })
            .boxed()
        },
    )
    .within(column(()).gap(0.0))
    .boxed()
}

/// The selected asset beside the grid: its kind, its name to change,
/// where it comes from, what its kind shows and changes, and the
/// buttons that open or remove it; the library while none is. Under
/// them, the last thing done or its Error.
fn details() -> AnyView {
    let pane = keyed(
        resource::<Shown, _>(|shown| shown.selected.clone()),
        |selected: &Option<Item>| {
            let selected = selected.clone();
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                let Some(item) = selected.clone() else {
                    return cx.build(library());
                };
                let own = item.is_own();
                // A graph opens in its editor; a mesh or a texture is
                // dragged onto a canvas instead.
                let opens = matches!(
                    item.kind,
                    Kind::Blueprint | Kind::Shader
                );
                let (renamed, opened, removed, shown_file) = (
                    item.clone(),
                    item.clone(),
                    item.clone(),
                    item.clone(),
                );
                let name: AnyView = if own {
                    text_field(
                        item.name.clone(),
                        move |world, name| {
                            rename(world, &renamed, &name);
                        },
                    )
                    .width(percent(100.0))
                    .rules(text_input)
                    .boxed()
                } else {
                    note(item.name.replace('_', " ")).boxed()
                };
                let body: AnyView = match item.kind {
                    Kind::Mesh => crate::graphs::assets::details(
                        item.name.clone(),
                    ),
                    Kind::Material => {
                        material::details(item.name.clone())
                    }
                    Kind::Blueprint | Kind::Shader => description(
                        "Open it to change its graph, or drag it \
                         onto a canvas.",
                    )
                    .boxed(),
                    Kind::Texture => description(
                        "Drag it onto a canvas to use it.",
                    )
                    .boxed(),
                };
                let preview: AnyView = if item.kind == Kind::Material
                {
                    material::preview()
                } else {
                    column(()).boxed()
                };
                let shown = |yes: bool| {
                    if yes { Display::Flex } else { Display::None }
                };
                cx.build(
                    column((
                        row((
                            section(item.kind.word().to_uppercase()),
                            frame().grow(1.0),
                            button(glyph(Glyph::Cross, ICON))
                                .padding(UiRect::all(px(4.0)))
                                .tooltip(|| {
                                    note("Back to the library")
                                })
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
                        name,
                        description(origin(&item)),
                        labelled("collection", collection_of(&item)),
                        body,
                        row((
                            button(label("Open"))
                                .padding(UiRect::axes(
                                    px(8.0),
                                    px(2.0),
                                ))
                                .display(shown(opens))
                                .rules(styles::button)
                                .tagged(Variant::Default)
                                .on_activate(move |world| {
                                    open(world, &opened);
                                }),
                            button(label("Show file"))
                                .padding(UiRect::axes(
                                    px(8.0),
                                    px(2.0),
                                ))
                                .display(shown(own))
                                .tooltip(|| {
                                    note(
                                        "Show its file in its folder",
                                    )
                                })
                                .rules(styles::button)
                                .tagged(Variant::Default)
                                .on_activate(move |world| {
                                    show_file(world, &shown_file);
                                }),
                            button(label("Remove"))
                                .padding(UiRect::axes(
                                    px(8.0),
                                    px(2.0),
                                ))
                                .display(shown(own))
                                .tooltip(|| {
                                    note(
                                        "Move it to the library's \
                                         trash folder",
                                    )
                                })
                                .rules(styles::button)
                                .tagged(Variant::Default)
                                .on_activate(move |world| {
                                    remove(world, &removed);
                                }),
                        ))
                        .gap(gap),
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
        scroll((column((
            pane,
            description(resource::<Library, _>(|library| {
                library.note.clone()
            })),
        ))
        .gap(gap)
        .padding(UiRect::all(px(gap))),))
        .width(px(DETAILS))
        .min_width(px(DETAILS))
        .max_width(px(DETAILS))
        .shrink(0.0)
        .fills_height()
        .fill(theme.palette.card)
        .border(1.0)
        .border_color(theme.palette.border)
    })
    .boxed()
}

/// Makes a new material that uses no shader yet and selects it, and
/// gives what happened.
fn new_material(world: &mut World) -> String {
    let made = world
        .resource_mut::<Library>()
        .create(Kind::Material, "new_material");
    match made {
        Ok(item) => {
            let note = format!("Made {}", item.name);
            world.resource_mut::<Browse>().selected = Some(item);
            note
        }
        Err(error) => format!("{error:#}"),
    }
}

fn open(world: &mut World, item: &Item) {
    let note = match crate::graphs::open_item(world, item) {
        Ok(()) => format!("Opened {}", item.name),
        Err(refusal) => refusal.to_string(),
    };
    world.resource_mut::<Library>().note = note;
}

fn rename(world: &mut World, item: &Item, name: &str) {
    if name == item.name {
        return;
    }
    let mut library = world.resource_mut::<Library>();
    library.note = match library.rename(item, name) {
        Ok(name) => format!("Renamed to {name}"),
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Browse>().selected = None;
}

fn remove(world: &mut World, item: &Item) {
    let mut library = world.resource_mut::<Library>();
    library.note = match library.remove(item) {
        Ok(()) => format!("Moved {} to the trash folder", item.name),
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Browse>().selected = None;
}

/// Asks for the `.png` file to add as a texture.
fn import_texture(world: &mut World) {
    let asker = match world.resource::<Importer>().0 {
        Some(asker) => asker,
        None => {
            let asker = world.spawn_empty().id();
            world.resource_mut::<Importer>().0 = Some(asker);
            asker
        }
    };
    world.trigger(ChooseFile {
        asker,
        mode: FileMode::Open,
        title: "Add a texture".to_owned(),
        extension: Kind::Texture.extension().to_owned(),
        name: String::new(),
    });
}

fn chosen(
    chosen: On<FileChosen>,
    importer: Res<Importer>,
    mut commands: Commands,
) {
    if importer.0 != Some(chosen.asker) {
        return;
    }
    let path = chosen.path.clone();
    commands.queue(move |world: &mut World| {
        let mut library = world.resource_mut::<Library>();
        library.note = match library.add_texture(Path::new(&path)) {
            Ok(name) => format!("Added {name}"),
            Err(error) => format!("{error:#}"),
        };
    });
}
