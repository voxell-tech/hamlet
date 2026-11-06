//! The ASSETS panel: the library as a grid of tiles, filtered by kind
//! and by folder and searched by name, with the selected asset's
//! details beside it. Folders are made, renamed and taken away above
//! the grid, and the details put the asset in one.
//! A click selects a tile and a double click opens it; a tile dragged
//! onto a canvas adds what it is there. The details rename, open or
//! remove the selected asset, give a mesh's material slots their
//! colours, and give a material its shader and the values of that
//! shader's inputs. Blueprints, shaders and materials are made new
//! here, meshes are added from `.glb` files and textures from `.png`
//! files.

mod material;

use std::path::Path;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui::{
    AlignItems, Display, FlexWrap, JustifyContent, Node, Overflow,
    UiRect, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, TooltipExt, button,
    column, dropdown, frame, label, menu_item, row, scroll, text_field,
};
use bevy_fynix::{EntityCursor, ScopedExt, ViewExt, keyed, resource};
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_graph_editor::files::{self, Files};
use hamlet_graph_editor::library::picker::{
    Section, picture, sections,
};
use hamlet_graph_editor::library::{
    Item, Kind, Library, Source, Tile,
};
use hamlet_ui::styles::{
    self, Fills, ICON, description, labelled, note, section, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::dock::{PanelAppExt, flush_panel};
use crate::graphs::{Blueprint, ShaderGraph};

/// A tile's width; its picture is as tall.
const TILE: f32 = 96.0;
/// The picture's side within a tile.
const PICTURE: f32 = 64.0;
/// The width of the selected asset's details beside the grid.
const DETAILS: f32 = 300.0;

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
    .add_systems(Update, (show, listen));
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

/// The tiles shown, worked out from the library and how it is
/// browsed, so the grid is built again only when they change, and the
/// folders with the one shown.
#[derive(Resource, Default, Clone, PartialEq)]
struct Shown {
    items: Vec<Item>,
    selected: Option<Item>,
    folders: Vec<String>,
    folder: Option<String>,
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
    section: Res<Section>,
    mut shown: ResMut<Shown>,
) {
    if !library.is_changed()
        && !browse.is_changed()
        && !section.is_changed()
    {
        return;
    }
    let search = browse.search.to_lowercase();
    // A folder the library no longer has shows every folder.
    let folder = browse
        .folder
        .clone()
        .filter(|folder| library.folders.paths.contains(folder));
    let items = library
        .items
        .iter()
        .filter(|item| section.holds(item))
        .filter(|item| {
            browse.kind.is_none_or(|kind| item.kind == kind)
        })
        .filter(|item| {
            folder.as_ref().is_none_or(|folder| {
                library.folders.holds(folder, item.kind, &item.name)
            })
        })
        .filter(|item| {
            search.is_empty()
                || item.name.to_lowercase().contains(&search)
        })
        .cloned()
        .collect::<Vec<_>>();
    // A selection the library no longer has is dropped.
    let selected = browse
        .selected
        .clone()
        .filter(|selected| library.items.contains(selected));
    let next = Shown {
        items,
        selected,
        folders: library.folders.paths.iter().cloned().collect(),
        folder,
    };
    if *shown != next {
        *shown = next;
    }
}

fn view() -> impl View {
    column((
        bar(),
        folders(),
        sections(),
        row((grid(), details()))
            .width(percent(100.0))
            .grow(1.0)
            .gap(0.0),
    ))
    .fills_width()
    .fills_height()
    .gap(0.0)
}

/// The kinds to show, the search and the import buttons.
fn bar() -> AnyView {
    with_cx(|cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        let chip = |word: &'static str, kind: Option<Kind>| {
            button(label(resource::<Browse, _>(move |browse| {
                if browse.kind == kind {
                    format!("• {word}")
                } else {
                    word.to_owned()
                }
            })))
            .padding(UiRect::axes(px(8.0), px(2.0)))
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(move |world| {
                world.resource_mut::<Browse>().kind = kind;
            })
            .boxed()
        };
        let mut chips = vec![chip("All", None)];
        chips.extend(
            [
                ("Blueprints", Kind::Blueprint),
                ("Shaders", Kind::Shader),
                ("Materials", Kind::Material),
                ("Meshes", Kind::Mesh),
                ("Textures", Kind::Texture),
            ]
            .map(|(word, kind)| chip(word, Some(kind))),
        );
        let import = |word: &'static str,
                      tip: &'static str,
                      ask: fn(&mut World)| {
            button(
                row((glyph(Glyph::Plus, ICON), label(word))).gap(4.0),
            )
            .padding(UiRect::axes(px(8.0), px(2.0)))
            .tooltip(move || note(tip))
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(move |world| ask(world))
        };
        let new = |word: &'static str,
                   tip: &'static str,
                   make: fn(&mut World) -> String| {
            button(
                row((glyph(Glyph::Plus, ICON), label(word))).gap(4.0),
            )
            .padding(UiRect::axes(px(8.0), px(2.0)))
            .tooltip(move || note(tip))
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(move |world| {
                let made = make(world);
                world.resource_mut::<Library>().note = made;
            })
        };
        row((
            row(chips).gap(2.0).align(AlignItems::Center),
            frame().grow(1.0),
            row((
                glyph(Glyph::Lens, ICON).tone(Tone::Dim),
                text_field(String::new(), |_, _| {})
                    .width(px(160.0))
                    .tagged(SearchField)
                    .rules(text_input),
            ))
            .gap(gap / 2.0)
            .align(AlignItems::Center),
            new("Blueprint", "New blueprint", |world| {
                files::create::<Blueprint>(world);
                world.resource::<Files<Blueprint>>().note.clone()
            }),
            new("Shader", "New shader", |world| {
                files::create::<ShaderGraph>(world);
                world.resource::<Files<ShaderGraph>>().note.clone()
            }),
            new(
                "Material",
                "New material, to give a shader's inputs values",
                new_material,
            ),
            import(
                "Mesh",
                "Add meshes from .glb files",
                crate::graphs::assets::import,
            ),
            import("Texture", "Add a texture from a .png file", import_texture),
        ))
        .width(percent(100.0))
        .padding(UiRect::all(px(gap)))
        .gap(gap)
        .align(AlignItems::Center)
        .fill(theme.palette.card)
    })
    .boxed()
}

/// The folders to show, each a chip, the button that makes a new one
/// in the folder shown, and while one is shown, a field to rename it
/// and a button that takes it away.
fn folders() -> AnyView {
    keyed(
        resource::<Shown, _>(|shown| {
            (shown.folders.clone(), shown.folder.clone())
        }),
        |(paths, open): &(Vec<String>, Option<String>)| {
            let (paths, open) = (paths.clone(), open.clone());
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                let fill = cx.theme().palette.card;
                let chip = |word: String, folder: Option<String>| {
                    let word = if folder == open {
                        format!("• {word}")
                    } else {
                        word
                    };
                    button(label(word))
                        .padding(UiRect::axes(px(8.0), px(2.0)))
                        .rules(styles::button)
                        .tagged(Variant::Default)
                        .on_activate(move |world| {
                            world.resource_mut::<Browse>().folder =
                                folder.clone();
                        })
                        .boxed()
                };
                let mut row_of = vec![chip("All folders".to_owned(), None)];
                row_of.extend(paths.iter().map(|path| {
                    chip(path.replace('/', " / "), Some(path.clone()))
                }));
                let parent = open.clone();
                row_of.push(
                    button(
                        row((glyph(Glyph::Plus, ICON), label("Folder")))
                            .gap(4.0),
                    )
                    .padding(UiRect::axes(px(8.0), px(2.0)))
                    .tooltip(|| note("New folder in the folder shown"))
                    .rules(styles::button)
                    .tagged(Variant::Default)
                    .on_activate(move |world| {
                        new_folder(world, parent.as_deref());
                    })
                    .boxed(),
                );
                if let Some(path) = open.clone() {
                    let removed = path.clone();
                    row_of.push(frame().grow(1.0).boxed());
                    row_of.push(
                        text_field(path.clone(), move |world, to| {
                            rename_folder(world, &path, &to);
                        })
                        .width(px(180.0))
                        .rules(text_input)
                        .boxed(),
                    );
                    row_of.push(
                        button(label("Remove folder"))
                            .padding(UiRect::axes(px(8.0), px(2.0)))
                            .tooltip(|| {
                                note(
                                    "Take the folder away; what is in \
                                     it goes to the folder above",
                                )
                            })
                            .rules(styles::button)
                            .tagged(Variant::Default)
                            .on_activate(move |world| {
                                remove_folder(world, &removed);
                            })
                            .boxed(),
                    );
                }
                cx.build(
                    row(row_of)
                        .width(percent(100.0))
                        .padding(UiRect::axes(px(gap), px(0.0)))
                        .gap(2.0)
                        .align(AlignItems::Center)
                        .fill(fill),
                )
            })
        },
    )
    .within(column(()).gap(0.0))
    .boxed()
}

/// Makes a new folder in `parent`, or at the top, and shows it.
fn new_folder(world: &mut World, parent: Option<&str>) {
    let made = world.resource_mut::<Library>().change_folders(|folders| {
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
            .and_then(|folder| paths.iter().position(|path| path == folder))
            .map_or(0, |at| at + 1);
        let words = std::iter::once("none".to_owned())
            .chain(paths.iter().map(|path| path.replace('/', " / ")))
            .collect::<Vec<_>>();
        dropdown(words, at, Glyph::Chevron.image(ICON), move |world, at| {
            let folder = at.checked_sub(1).and_then(|at| paths.get(at));
            let put = world.resource_mut::<Library>().change_folders(
                |folders| {
                    folders.put(item.kind, &item.name, folder.map(String::as_str))
                },
            );
            if let Err(error) = put {
                world.resource_mut::<Library>().note = format!("{error:#}");
            }
        })
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

/// The tiles shown, wrapped in rows.
fn grid() -> AnyView {
    scroll((keyed(
        resource::<Shown, _>(Clone::clone),
        |shown: &Shown| {
            let tiles = shown
                .items
                .iter()
                .map(|item| {
                    tile(
                        item.clone(),
                        shown.selected.as_ref() == Some(item),
                    )
                })
                .collect::<Vec<_>>();
            let empty = tiles.is_empty();
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                if empty {
                    return cx.build(
                        column((description(
                            "Nothing here yet. Add a mesh or a \
                             texture, or save a blueprint or shader.",
                        ),))
                        .padding(UiRect::all(px(gap * 2.0))),
                    );
                }
                let grid = cx.build(
                    row(tiles)
                        .gap(gap)
                        .padding(UiRect::all(px(gap)))
                        .width(percent(100.0)),
                );
                // Tiles fill a row and go on in the next.
                if let Some(mut node) = cx.world.get_mut::<Node>(grid)
                {
                    node.flex_wrap = FlexWrap::Wrap;
                    node.row_gap = px(gap);
                }
                grid
            })
        },
    ),))
    .grow(1.0)
    .min_width(px(0.0))
    .fills_height()
    .boxed()
}

/// Where `item` comes from, in words: its kind, or that it is built
/// in or an example.
fn origin(item: &Item) -> String {
    match &item.source {
        Source::File(_) => item.kind.word().to_owned(),
        Source::BuiltIn => "Built-in mesh".to_owned(),
        Source::Example(_) => {
            format!("Example {}", item.kind.word().to_lowercase())
        }
    }
}

/// One asset: its kind's picture, its name, and where it comes from.
fn tile(item: Item, selected: bool) -> AnyView {
    let for_menu = item.clone();
    with_cx(move |cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        let palette = &theme.palette;
        let tint = match item.kind {
            Kind::Blueprint => palette.gold_3,
            Kind::Shader => palette.red,
            Kind::Material => palette.glow,
            Kind::Mesh => palette.agent,
            Kind::Texture => palette.green,
        };
        let from = origin(&item);
        let drawn = match item.kind {
            Kind::Mesh => picture(&item.name, PICTURE),
            _ => column((
                glyph(item.kind.glyph(), 28.0).tint(Some(tint)),
            ))
            .width(px(PICTURE))
            .height(px(PICTURE))
            .align(AlignItems::Center)
            .justify(JustifyContent::Center)
            .fill(palette.input)
            .radius(theme.space.round)
            .boxed(),
        };
        column((
            drawn,
            column((note(item.name.replace('_', " ")),))
                .width(percent(100.0))
                .overflow(Overflow::clip())
                .align(AlignItems::Center),
            description(from),
        ))
        .width(px(TILE))
        .padding(UiRect::all(px(gap / 2.0)))
        .gap(2.0)
        .align(AlignItems::Center)
        .fill(if selected {
            palette.text_selection
        } else {
            palette.card
        })
        .border(1.0)
        .border_color(if selected {
            palette.glow
        } else {
            palette.border
        })
        .radius(theme.space.round)
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

/// The selected asset beside the grid: its kind, its name to change,
/// where it comes from, what its kind shows and changes, and the
/// buttons that open or remove it. Under them, the last thing done or
/// its Error.
fn details() -> AnyView {
    let pane = keyed(
        resource::<Shown, _>(|shown| shown.selected.clone()),
        |selected: &Option<Item>| {
            let selected = selected.clone();
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                let Some(item) = selected.clone() else {
                    return cx.build(column((description(
                        "Select an asset to see and change it here.",
                    ),)));
                };
                let own = item.is_own();
                // A graph opens in its editor; a mesh or a texture is
                // dragged onto a canvas instead.
                let opens =
                    matches!(item.kind, Kind::Blueprint | Kind::Shader);
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
                    Kind::Mesh => {
                        crate::graphs::assets::details(item.name.clone())
                    }
                    Kind::Material => {
                        material::details(item.name.clone())
                    }
                    Kind::Blueprint | Kind::Shader => description(
                        "Open it to change its graph, or drag it onto \
                         a canvas.",
                    )
                    .boxed(),
                    Kind::Texture => {
                        description("Drag it onto a canvas to use it.")
                            .boxed()
                    }
                };
                let shown = |yes: bool| {
                    if yes { Display::Flex } else { Display::None }
                };
                cx.build(
                    column((
                        section(item.kind.word().to_uppercase()),
                        name,
                        description(origin(&item)),
                        labelled("folder", folder_of(&item)),
                        body,
                        row((
                            button(label("Open"))
                                .padding(UiRect::axes(px(8.0), px(2.0)))
                                .display(shown(opens))
                                .rules(styles::button)
                                .tagged(Variant::Default)
                                .on_activate(move |world| {
                                    open(world, &opened);
                                }),
                            button(label("Remove"))
                                .padding(UiRect::axes(px(8.0), px(2.0)))
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
