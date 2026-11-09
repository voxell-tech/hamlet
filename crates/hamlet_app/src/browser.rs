//! The ASSETS panel: the library as a grid of tiles under a heading
//! for each kind, the person's own before the built-in ones, filtered
//! by kind and by folder and searched by name, with the selected
//! asset's details beside it, or the library's folder while none is.
//! A tile of the person's that stands in for a built-in one of the
//! same name says so, and so does the built-in one.
//! A click selects a tile and a double click opens it; a tile dragged
//! onto a canvas adds what it is there. The details rename, open or
//! remove the selected asset, give a mesh's material slots their
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
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::picking::Pickable;
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::text::{EditableText, Justify, TextEditChange, TextLayout};
use bevy::ui::{
    AlignContent, AlignItems, Display, FlexWrap, JustifyContent,
    Node, Overflow, PositionType, UiRect, Val, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, TooltipExt, button,
    column, dropdown, frame, label, menu_button, menu_item, row,
    scroll, text_field,
};
use bevy_fynix::{
    EntityCursor, ScopedExt, ViewExt, keyed, resource, style,
};
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_graph::Catalogue;
use hamlet_graph_editor::files::{self, Files};
use hamlet_graph_editor::library::picker::picture;
use hamlet_graph_editor::library::{
    Item, Kind, Library, Source, Tile,
};
use hamlet_ui::styles::{
    self, Fills, ICON, description, labelled, note, note_rules,
    section, segmented_words, text_input,
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
    .add_systems(Update, (show, listen, pick));
}

/// How the library is browsed: the kind shown, none for every kind,
/// the folder shown, none for every folder, what is searched for, and
/// the tile selected.
#[derive(Resource, Default)]
struct Browse {
    kind: Option<Kind>,
    folder: Option<String>,
    search: String,
    selected: Option<Item>,
}

/// The tiles shown under their headings, worked out from the library
/// and how it is browsed, so the grid is built again only when they
/// change, and the folders with the one shown.
#[derive(Resource, Default, Clone, PartialEq)]
struct Shown {
    groups: Vec<Group>,
    selected: Option<Item>,
    folders: Vec<String>,
    folder: Option<String>,
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

/// The search field.
#[derive(Component)]
struct SearchField;

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
    // A folder the library no longer has shows every folder.
    let folder = browse
        .folder
        .clone()
        .filter(|folder| library.folders.paths.contains(folder));
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
                    folder.as_ref().is_none_or(|folder| {
                        library
                            .folders
                            .holds(folder, item.kind, &item.name)
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
        folders: library.folders.paths.iter().cloned().collect(),
        folder,
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
        row((grid(), details()))
            .width(percent(100.0))
            .grow(1.0)
            .min_height(px(0.0))
            .gap(0.0),
    ))
    .fills_width()
    .fills_height()
    .gap(0.0)
}

/// The one row over the grid: the kinds to show, the folder to show
/// and what is done with folders, the search, and the New menu.
fn bar() -> AnyView {
    with_cx(|cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        row((
            kinds(),
            folders(),
            frame().grow(1.0),
            search(),
            new_menu(),
        ))
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

/// The folder shown, every folder or one, and a menu that makes a
/// new one in it or takes it away.
fn folders() -> AnyView {
    keyed(
        resource::<Shown, _>(|shown| {
            (shown.folders.clone(), shown.folder.clone())
        }),
        |(paths, open): &(Vec<String>, Option<String>)| {
            let (paths, open) = (paths.clone(), open.clone());
            with_cx(move |cx| {
                let theme = cx.theme();
                let at = open
                    .as_ref()
                    .and_then(|open| {
                        paths.iter().position(|path| path == open)
                    })
                    .map_or(0, |at| at + 1);
                let words = std::iter::once("All folders".to_owned())
                    .chain(
                        paths
                            .iter()
                            .map(|path| path.replace('/', " / ")),
                    )
                    .collect::<Vec<_>>();
                let chosen = paths.clone();
                let pick = dropdown(
                    words,
                    at,
                    Glyph::Chevron.image(ICON),
                    move |world, at| {
                        world.resource_mut::<Browse>().folder = at
                            .checked_sub(1)
                            .and_then(|at| chosen.get(at))
                            .cloned();
                    },
                )
                .width(px(150.0));
                let mut deeds = vec!["New folder".to_owned()];
                if open.is_some() {
                    deeds.push("Remove this folder".to_owned());
                }
                let parent = open.clone();
                let menu =
                    menu_button("", deeds, move |world, at| {
                        match (at, parent.as_deref()) {
                            (0, parent) => new_folder(world, parent),
                            (_, Some(path)) => {
                                remove_folder(world, path);
                            }
                            _ => {}
                        }
                    })
                    .icon(Glyph::More.image(ICON))
                    .padding(UiRect::axes(px(8.0), px(4.0)))
                    .fill(theme.palette.button)
                    .border(theme.space.border)
                    .border_color(theme.palette.gold_5)
                    .radius(theme.space.round)
                    .tooltip(|| {
                        note(
                            "Make a folder in the one shown, or \
                             take the one shown away",
                        )
                    });
                row((pick, menu)).gap(2.0).align(AlignItems::Center)
            })
            .boxed()
        },
    )
    .within(row(()).gap(0.0))
    .boxed()
}

/// The search field, with what it searches written in it while it is
/// empty.
fn search() -> AnyView {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        let hint =
            row((note("Search assets").tagged(Pickable::IGNORE),))
                .position(PositionType::Absolute)
                .inset(UiRect::new(
                    px(10.0),
                    Val::Auto,
                    px(0.0),
                    px(0.0),
                ))
                .align(AlignItems::Center)
                .display(resource::<Browse, _>(|browse| {
                    if browse.search.is_empty() {
                        Display::Flex
                    } else {
                        Display::None
                    }
                }))
                .tagged(Pickable::IGNORE);
        row((
            glyph(Glyph::Lens, ICON).tone(Tone::Dim),
            column((
                text_field(String::new(), |_, _| {})
                    .width(px(SEARCH))
                    .tagged(SearchField)
                    .rules(text_input),
                hint,
            ))
            .gap(0.0),
        ))
        .gap(gap / 2.0)
        .align(AlignItems::Center)
    })
    .boxed()
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

/// Makes a new folder in `parent`, or at the top, and shows it.
fn new_folder(world: &mut World, parent: Option<&str>) {
    let made =
        world.resource_mut::<Library>().change_folders(|folders| {
            let path = folders.free(parent, "folder");
            folders.add(&path)
        });
    let note = match made {
        Ok(path) => {
            let note = format!("Made the folder {path}");
            world.resource_mut::<Browse>().folder = Some(path);
            note
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Library>().note = note;
}

/// Gives the folder `from` the path `to`, and keeps showing it.
fn rename_folder(world: &mut World, from: &str, to: &str) {
    if from == to {
        return;
    }
    let renamed = world
        .resource_mut::<Library>()
        .change_folders(|folders| folders.rename(from, to));
    let note = match renamed {
        Ok(path) => {
            let note = format!("Renamed the folder to {path}");
            world.resource_mut::<Browse>().folder = Some(path);
            note
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Library>().note = note;
}

/// Takes the folder `path` away and shows the folder above it.
fn remove_folder(world: &mut World, path: &str) {
    let removed = world
        .resource_mut::<Library>()
        .change_folders(|folders| folders.remove(path));
    let note = match removed {
        Ok(()) => {
            world.resource_mut::<Browse>().folder = path
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_owned());
            format!("Took the folder {path} away")
        }
        Err(error) => format!("{error:#}"),
    };
    world.resource_mut::<Library>().note = note;
}

/// The dropdown of the folder `item` is in, none or one of the
/// library's, built again whenever the library changes.
fn folder_of(item: &Item) -> AnyView {
    let item = item.clone();
    keyed(
        resource::<Library, _>(|library| library.revision),
        move |_: &u64| folder_picker(item.clone()),
    )
    .within(column(()).gap(0.0))
    .boxed()
}

fn folder_picker(item: Item) -> AnyView {
    with_cx(move |cx| {
        let library = cx.world.resource::<Library>();
        let paths: Vec<String> =
            library.folders.paths.iter().cloned().collect();
        let at = library
            .folders
            .of(item.kind, &item.name)
            .and_then(|folder| {
                paths.iter().position(|path| path == folder)
            })
            .map_or(0, |at| at + 1);
        let words = std::iter::once("none".to_owned())
            .chain(paths.iter().map(|path| path.replace('/', " / ")))
            .collect::<Vec<_>>();
        dropdown(
            words,
            at,
            Glyph::Chevron.image(ICON),
            move |world, at| {
                let folder =
                    at.checked_sub(1).and_then(|at| paths.get(at));
                let put = world
                    .resource_mut::<Library>()
                    .change_folders(|folders| {
                        folders.put(
                            item.kind,
                            &item.name,
                            folder.map(String::as_str),
                        )
                    });
                if let Err(error) = put {
                    world.resource_mut::<Library>().note =
                        format!("{error:#}");
                }
            },
        )
        .width(percent(100.0))
    })
    .boxed()
}

/// Observes the search field's input once the field is built.
fn listen(
    mut commands: Commands,
    fields: Query<&Children, With<SearchField>>,
    mut heard: Local<Option<Entity>>,
) {
    let Some(input) =
        fields.iter().next().and_then(|kids| kids.first().copied())
    else {
        return;
    };
    if *heard != Some(input) {
        commands.entity(input).observe(searched);
        *heard = Some(input);
    }
}

fn searched(
    change: On<TextEditChange>,
    texts: Query<&EditableText>,
    mut browse: ResMut<Browse>,
) {
    let Ok(text) = texts.get(change.event_target()) else {
        return;
    };
    let typed = text.value().to_string();
    if browse.search != typed {
        browse.search = typed;
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
                    // Built where it goes, under its heading. Tiles
                    // fill a row and go on in the next, packed to the
                    // top, each as tall as itself.
                    parts.push(AnyView::new(move |cx: &mut Cx| {
                        let wrap = cx.build(
                            row(tiles)
                                .gap(gap / 2.0)
                                .width(percent(100.0))
                                .align(AlignItems::FlexStart),
                        );
                        if let Some(mut node) =
                            cx.world.get_mut::<Node>(wrap)
                        {
                            node.flex_wrap = FlexWrap::Wrap;
                            node.row_gap = px(gap / 2.0);
                            node.align_content =
                                AlignContent::FlexStart;
                        }
                        wrap
                    }));
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

/// `name` as a note, its lines centred, wrapping where it is wider
/// than its tile.
fn centred(name: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let built = cx.build(label(name.clone()).rules(note_rules));
        if let Some(mut layout) =
            cx.world.get_mut::<TextLayout>(built)
        {
            layout.justify = Justify::Center;
        }
        built
    })
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
            Kind::Mesh | Kind::Blueprint => {
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
            let (open_it, remove_it) =
                (for_menu.clone(), for_menu.clone());
            (
                menu_item(label("Open"))
                    .on_activate(move |world| open(world, &open_it)),
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
/// the folder shown to rename.
fn library() -> AnyView {
    keyed(
        resource::<Shown, _>(|shown| {
            (shown.counts.clone(), shown.folder.clone())
        }),
        |(counts, folder): &(Vec<usize>, Option<String>)| {
            let counts = counts.clone();
            let folder = folder.clone();
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
                if let Some(path) = folder.clone() {
                    let from = path.clone();
                    parts.push(section("FOLDER SHOWN").boxed());
                    parts.push(
                        text_field(path, move |world, to| {
                            rename_folder(world, &from, &to);
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
                let (renamed, opened, removed) =
                    (item.clone(), item.clone(), item.clone());
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
                        name,
                        description(origin(&item)),
                        labelled("folder", folder_of(&item)),
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
                        preview,
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
