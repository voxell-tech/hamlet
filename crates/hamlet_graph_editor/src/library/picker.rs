//! The library's meshes as a grid of pictures, in two sections: the
//! built-in ones and the ones added. A click on a picture is handed
//! to the view's owner, which decides what picking one does.

use bevy::app::{App, Update};
use bevy::asset::Handle;
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, Display, FlexWrap, JustifyContent, Node, Overflow,
    percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, row, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_ui::styles::{card_button, description, note};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::tabs;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::thumbnails::Thumbnails;
use super::{Item, Kind, Library};

/// A picture's side in a cell of the grid.
const PICTURE: f32 = 64.0;

/// A cell's width.
const CELL: f32 = 96.0;

/// The tabs of the sections, in order.
const TABS: [(Section, Glyph, &str); 2] = [
    (Section::BuiltIn, Glyph::Book, "BUILT-IN"),
    (Section::Added, Glyph::Plus, "ADDED"),
];

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Section>()
        .init_resource::<Listed>()
        .add_systems(Update, (list, tabs::mark::<Section>));
}

/// The part of the library a list shows.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Section {
    /// What comes with the window.
    #[default]
    BuiltIn,
    /// What the person added to the library.
    Added,
}

impl Section {
    /// Whether `item` is in this section.
    pub fn holds(self, item: &Item) -> bool {
        item.is_own() == (self == Section::Added)
    }
}

/// What a click on a mesh does.
pub trait Pick:
    Fn(&mut World, &Item) + Clone + Send + Sync + 'static
{
}

impl<F> Pick for F where
    F: Fn(&mut World, &Item) + Clone + Send + Sync + 'static
{
}

/// The library's meshes, by section.
#[derive(Resource, Default, Clone, PartialEq)]
struct Listed {
    built_in: Vec<Item>,
    added: Vec<Item>,
}

impl Listed {
    fn of(&self, section: Section) -> &[Item] {
        match section {
            Section::BuiltIn => &self.built_in,
            Section::Added => &self.added,
        }
    }
}

fn list(library: Res<Library>, mut listed: ResMut<Listed>) {
    if !library.is_changed() {
        return;
    }
    let meshes = |section: Section| {
        library
            .items
            .iter()
            .filter(|item| {
                item.kind == Kind::Mesh && section.holds(item)
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    let next = Listed {
        built_in: meshes(Section::BuiltIn),
        added: meshes(Section::Added),
    };
    if *listed != next {
        *listed = next;
    }
}

/// The tabs that choose the [`Section`] shown.
pub fn sections() -> impl View {
    tabs::strip(&TABS, |world, section| {
        *world.resource_mut::<Section>() = section;
    })
}

/// `body`, displayed only while `section` is the one chosen.
pub fn shown(section: Section, body: impl View) -> impl View {
    column((body,)).gap(0.0).display(resource::<Section, _>(
        move |chosen| {
            if *chosen == section {
                Display::Flex
            } else {
                Display::None
            }
        },
    ))
}

/// The picker: the tabs of the sections over a scrolling grid of
/// the meshes of the one chosen. `pick` runs with the mesh a click
/// lands on.
pub fn picker(pick: impl Pick) -> AnyView {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        column((
            sections(),
            scroll((
                shown(
                    Section::BuiltIn,
                    grid(Section::BuiltIn, pick.clone()),
                ),
                shown(Section::Added, grid(Section::Added, pick)),
            ))
            .grow(1.0)
            .width(percent(100.0)),
        ))
        .gap(gap)
        .width(percent(100.0))
        .min_height(px(0.0))
    })
    .boxed()
}

/// The meshes of `section`, wrapped in rows of cells.
pub fn grid(section: Section, pick: impl Pick) -> AnyView {
    keyed(
        resource::<Listed, _>(move |listed| {
            listed.of(section).to_vec()
        }),
        move |items: &Vec<Item>| {
            let (items, pick) = (items.clone(), pick.clone());
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                if items.is_empty() {
                    return cx.build(description("Nothing here yet"));
                }
                let cells = items
                    .into_iter()
                    .map(|item| cell(item, pick.clone()))
                    .collect::<Vec<_>>();
                let node = cx
                    .build(row(cells).gap(gap).width(percent(100.0)));
                if let Some(mut layout) =
                    cx.world.get_mut::<Node>(node)
                {
                    layout.flex_wrap = FlexWrap::Wrap;
                    layout.row_gap = px(gap);
                }
                node
            })
        },
    )
    .within(column(()).gap(0.0).width(percent(100.0)))
    .boxed()
}

/// One mesh: its picture over its name.
fn cell(item: Item, pick: impl Pick) -> AnyView {
    let name = item.name.clone();
    button(
        column((
            picture(&name, PICTURE),
            column((note(name.replace('_', " ")),))
                .width(percent(100.0))
                .overflow(Overflow::clip())
                .align(AlignItems::Center),
        ))
        .gap(2.0)
        .align(AlignItems::Center),
    )
    .width(px(CELL))
    .rules(card_button)
    .on_activate(move |world| pick(world, &item))
    .boxed()
}

/// The picture of the mesh `name` in a square of `side` pixels: its
/// rendering once drawn, a cube until then. Showing it asks for it.
pub fn picture(name: &str, side: f32) -> AnyView {
    let name = name.to_owned();
    AnyView::new(move |cx: &mut Cx| {
        Thumbnails::ask(cx.world, &name);
        let read = name.clone();
        cx.build(
            keyed(
                resource::<Thumbnails, _>(move |thumbnails| {
                    thumbnails.picture(&read)
                }),
                move |image: &Option<Handle<Image>>| {
                    square(image.clone(), side)
                },
            )
            .within(column(()).gap(0.0)),
        )
    })
}

/// A square plate with `image` filling it, or with the cube where
/// there is none.
fn square(image: Option<Handle<Image>>, side: f32) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (fill, round) = (theme.palette.input, theme.space.round);
        let plate = |inside: Vec<AnyView>| {
            column(inside)
                .width(px(side))
                .height(px(side))
                .align(AlignItems::Center)
                .justify(JustifyContent::Center)
                .fill(fill)
                .radius(round)
        };
        match image {
            Some(image) => {
                let node = cx.build(plate(Vec::new()));
                // A node of its own, which no paint of a view tints.
                cx.world.spawn((
                    Node {
                        width: percent(100.0),
                        height: percent(100.0),
                        ..Node::default()
                    },
                    ImageNode::new(image)
                        .with_mode(NodeImageMode::Stretch),
                    ChildOf(node),
                ));
                node
            }
            None => cx.build(plate(vec![
                glyph(Glyph::Cube, side * 0.4)
                    .tone(Tone::Dim)
                    .boxed(),
            ])),
        }
    })
}
