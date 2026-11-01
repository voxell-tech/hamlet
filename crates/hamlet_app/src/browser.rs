//! The ASSETS panel: the library as a grid of tiles, filtered by kind
//! and searched by name. A click selects a tile and a double click
//! opens it; a tile dragged onto a canvas adds what it is there. The
//! selected tile is renamed, opened or removed in the strip under the
//! grid. Meshes are added from `.glb` files and textures from `.png`
//! files.

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
    column, frame, label, menu_item, row, scroll, text_field,
};
use bevy_fynix::{EntityCursor, ScopedExt, ViewExt, keyed, resource};
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_graph_editor::library::{
    Item, Kind, Library, Source, Tile,
};
use hamlet_ui::styles::{
    self, Fills, ICON, description, note, section, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::dock::{PanelAppExt, flush_panel};

/// A tile's width; its picture is as tall.
const TILE: f32 = 96.0;
/// The picture's side within a tile.
const PICTURE: f32 = 64.0;

pub(crate) fn plugin(app: &mut App) {
    app.add_panel(
        "assets",
        DockWindowKind::new("ASSETS", || flush_panel(view()))
            .icon(Glyph::Cube.image(ICON)),
    )
    .init_resource::<Browse>()
    .init_resource::<Shown>()
    .init_resource::<Importer>()
    .add_observer(chosen)
    .add_observer(pressed)
    .add_systems(Update, (show, listen));
}

/// How the library is browsed: the kind shown, none for every kind,
/// what is searched for, and the tile selected.
#[derive(Resource, Default)]
struct Browse {
    kind: Option<Kind>,
    search: String,
    selected: Option<Item>,
}

/// The tiles shown, worked out from the library and how it is
/// browsed, so the grid is built again only when they change.
#[derive(Resource, Default, Clone, PartialEq)]
struct Shown {
    items: Vec<Item>,
    selected: Option<Item>,
}

/// The search field.
#[derive(Component)]
struct SearchField;

/// What the import dialogs answer to: one for meshes, one for
/// textures.
#[derive(Resource, Default)]
struct Importer(Option<(Entity, Entity)>);

fn show(
    library: Res<Library>,
    browse: Res<Browse>,
    mut shown: ResMut<Shown>,
) {
    if !library.is_changed() && !browse.is_changed() {
        return;
    }
    let search = browse.search.to_lowercase();
    let items = library
        .items
        .iter()
        .filter(|item| {
            browse.kind.is_none_or(|kind| item.kind == kind)
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
    let next = Shown { items, selected };
    if *shown != next {
        *shown = next;
    }
}

fn view() -> impl View {
    column((bar(), grid(), details()))
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
                ("Materials", Kind::Material),
                ("Meshes", Kind::Mesh),
                ("Textures", Kind::Texture),
            ]
            .map(|(word, kind)| chip(word, Some(kind))),
        );
        let import = |word: &'static str,
                      tip: &'static str,
                      kind: Kind| {
            button(
                row((glyph(Glyph::Plus, ICON), label(word))).gap(4.0),
            )
            .padding(UiRect::axes(px(8.0), px(2.0)))
            .tooltip(move || note(tip))
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(move |world| import(world, kind))
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
            import("Mesh", "Add a mesh from a .glb file", Kind::Mesh),
            import(
                "Texture",
                "Add a texture from a .png file",
                Kind::Texture,
            ),
        ))
        .width(percent(100.0))
        .padding(UiRect::all(px(gap)))
        .gap(gap)
        .align(AlignItems::Center)
        .fill(theme.palette.card)
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
                             texture, or save a blueprint or \
                             material.",
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
    .fills_width()
    .fills_height()
    .boxed()
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
            Kind::Material => palette.red,
            Kind::Mesh => palette.agent,
            Kind::Texture => palette.green,
        };
        let from = match &item.source {
            Source::File(_) => item.kind.word().to_owned(),
            Source::BuiltIn => "Built-in mesh".to_owned(),
            Source::Example(_) => {
                format!("Example {}", item.kind.word().to_lowercase())
            }
        };
        let picture = column((
            glyph(item.kind.glyph(), 28.0).tint(Some(tint)),
        ))
        .width(px(PICTURE))
        .height(px(PICTURE))
        .align(AlignItems::Center)
        .justify(JustifyContent::Center)
        .fill(palette.input)
        .radius(theme.space.round);
        column((
            picture,
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

/// The selected asset: its name to change, and what can be done to
/// it.
fn details() -> AnyView {
    keyed(
        resource::<Shown, _>(|shown| shown.selected.clone()),
        |selected: &Option<Item>| {
            let selected = selected.clone();
            AnyView::new(move |cx: &mut Cx| {
                let theme = cx.theme();
                let gap = theme.space.gap;
                let Some(item) = selected.clone() else {
                    return cx.build(
                        row((description(resource::<Library, _>(
                            |library| {
                                if library.note.is_empty() {
                                    "Select an asset to rename, open \
                                     or remove it"
                                        .to_owned()
                                } else {
                                    library.note.clone()
                                }
                            },
                        )),))
                        .padding(UiRect::all(px(gap)))
                        .fill(theme.palette.card),
                    );
                };
                let own = item.is_own();
                let (renamed, opened, removed) =
                    (item.clone(), item.clone(), item.clone());
                let name: AnyView = if own {
                    text_field(
                        item.name.clone(),
                        move |world, name| {
                            rename(world, &renamed, &name);
                        },
                    )
                    .width(px(200.0))
                    .rules(text_input)
                    .boxed()
                } else {
                    note(item.name.replace('_', " ")).boxed()
                };
                cx.build(
                    row((
                        section(item.kind.word().to_uppercase()),
                        name,
                        frame().grow(1.0),
                        description(resource::<Library, _>(
                            |library| library.note.clone(),
                        )),
                        button(label("Open"))
                            .padding(UiRect::axes(px(8.0), px(2.0)))
                            .rules(styles::button)
                            .tagged(Variant::Default)
                            .on_activate(move |world| {
                                open(world, &opened);
                            }),
                        button(label("Remove"))
                            .padding(UiRect::axes(px(8.0), px(2.0)))
                            .display(if own {
                                Display::Flex
                            } else {
                                Display::None
                            })
                            .tooltip(|| {
                                note(
                                    "Move it to the library's trash \
                                     folder",
                                )
                            })
                            .rules(styles::button)
                            .tagged(Variant::Default)
                            .on_activate(move |world| {
                                remove(world, &removed);
                            }),
                    ))
                    .width(percent(100.0))
                    .padding(UiRect::all(px(gap)))
                    .gap(gap)
                    .align(AlignItems::Center)
                    .fill(theme.palette.card),
                )
            })
        },
    )
    .within(column(()).gap(0.0))
    .boxed()
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

fn import(world: &mut World, kind: Kind) {
    let (meshes, textures) = match world.resource::<Importer>().0 {
        Some(askers) => askers,
        None => {
            let askers =
                (world.spawn_empty().id(), world.spawn_empty().id());
            world.resource_mut::<Importer>().0 = Some(askers);
            askers
        }
    };
    let asker = if kind == Kind::Mesh { meshes } else { textures };
    world.trigger(ChooseFile {
        asker,
        mode: FileMode::Open,
        title: format!("Add a {}", kind.word().to_lowercase()),
        extension: kind.extension().to_owned(),
        name: String::new(),
    });
}

fn chosen(
    chosen: On<FileChosen>,
    importer: Res<Importer>,
    mut commands: Commands,
) {
    let Some((meshes, textures)) = importer.0 else {
        return;
    };
    let path = chosen.path.clone();
    if chosen.asker == meshes {
        commands.queue(move |world: &mut World| {
            let note = match crate::graphs::add_mesh(world, &path) {
                Ok(name) => format!("Added {name}"),
                Err(error) => format!("{error:#}"),
            };
            world.resource_mut::<Library>().note = note;
        });
    } else if chosen.asker == textures {
        commands.queue(move |world: &mut World| {
            let mut library = world.resource_mut::<Library>();
            library.note = match library.add_texture(Path::new(&path))
            {
                Ok(name) => format!("Added {name}"),
                Err(error) => format!("{error:#}"),
            };
        });
    }
}
