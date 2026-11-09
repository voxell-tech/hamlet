//! The picker of the library's assets of one kind: a search over a
//! grid of their pictures, the person's own under one heading and the
//! built-in ones under another. A click on one is handed to the
//! view's owner, which decides what picking one does. Every choice of
//! an asset is made in one.

use bevy::app::{App, Update};
use bevy::asset::Handle;
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::text::{Justify, TextLayout};
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignContent, AlignItems, FlexWrap, JustifyContent, Node,
    Overflow, percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_ui::styles::{
    card_choice, description, note_rules, section,
};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, with_cx};

use super::search::search;
use super::thumbnails::Thumbnails;
use super::{Item, Kind, Library};

/// A picture's side in a cell of the grid.
const PICTURE: f32 = 64.0;

/// A cell's width.
const CELL: f32 = 88.0;

/// The height of a cell's name: two lines of a note.
const NAME: f32 = 32.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Sought>()
        .init_resource::<Listed>()
        .add_systems(Update, list);
}

/// What the open picker searches for. A picker built anew searches
/// for nothing.
#[derive(Resource, Default, Clone, PartialEq)]
struct Sought(String);

/// The assets a picker lists, of each kind: the person's own and the
/// built-in ones, those whose names hold what is sought.
#[derive(Resource, Default, Clone, PartialEq)]
struct Listed(Vec<(Kind, Vec<Item>, Vec<Item>)>);

impl Listed {
    fn of(&self, kind: Kind) -> (Vec<Item>, Vec<Item>) {
        self.0
            .iter()
            .find(|(of, ..)| *of == kind)
            .map(|(_, own, built_in)| (own.clone(), built_in.clone()))
            .unwrap_or_default()
    }
}

fn list(
    library: Res<Library>,
    sought: Res<Sought>,
    mut listed: ResMut<Listed>,
) {
    if !library.is_changed() && !sought.is_changed() {
        return;
    }
    let sought = sought.0.to_lowercase();
    let kinds = [
        Kind::Blueprint,
        Kind::Shader,
        Kind::Material,
        Kind::Prefab,
        Kind::Mesh,
        Kind::Texture,
    ];
    let next = Listed(
        kinds
            .into_iter()
            .map(|kind| {
                let of = |own: bool| {
                    library
                        .items
                        .iter()
                        .filter(|item| {
                            item.kind == kind
                                && item.is_own() == own
                                && (sought.is_empty()
                                    || item
                                        .name
                                        .to_lowercase()
                                        .contains(&sought))
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                };
                (kind, of(true), of(false))
            })
            .collect(),
    );
    if *listed != next {
        *listed = next;
    }
}

/// What a click on an asset does.
pub trait Pick:
    Fn(&mut World, &Item) + Clone + Send + Sync + 'static
{
}

impl<F> Pick for F where
    F: Fn(&mut World, &Item) + Clone + Send + Sync + 'static
{
}

/// The picker of the `kind` assets: a search over a scrolling grid of
/// them, `chosen` lit. `pick` runs with the asset a click lands on.
pub fn picker(
    kind: Kind,
    chosen: Option<String>,
    pick: impl Pick,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        cx.world.resource_mut::<Sought>().0.clear();
        let gap = cx.theme().space.gap;
        let hint = match kind {
            Kind::Blueprint => "Search blueprints",
            Kind::Shader => "Search shaders",
            Kind::Material => "Search materials",
            Kind::Prefab => "Search prefabs",
            Kind::Mesh => "Search meshes",
            Kind::Texture => "Search textures",
        };
        cx.build(
            column((
                search(hint, 180.0, |world, text| {
                    world.resource_mut::<Sought>().0 = text;
                }),
                scroll((grid(kind, chosen.clone(), pick.clone()),))
                    .grow(1.0)
                    .min_height(px(0.0))
                    .width(percent(100.0)),
            ))
            .gap(gap)
            .width(percent(100.0))
            .min_height(px(0.0)),
        )
    })
}

/// The `kind` assets listed, the person's own and then the built-in
/// ones, each under its heading and wrapped in rows of cells.
fn grid(
    kind: Kind,
    chosen: Option<String>,
    pick: impl Pick,
) -> AnyView {
    keyed(
        resource::<Listed, _>(move |listed| listed.of(kind)),
        move |(own, built_in): &(Vec<Item>, Vec<Item>)| {
            let groups = [
                ("YOURS", own.clone()),
                ("BUILT IN", built_in.clone()),
            ];
            let (chosen, pick) = (chosen.clone(), pick.clone());
            with_cx(move |cx| {
                let gap = cx.theme().space.gap;
                let mut parts: Vec<AnyView> = Vec::new();
                for (title, items) in groups.clone() {
                    if items.is_empty() {
                        continue;
                    }
                    parts.push(
                        section(format!("{title} · {}", items.len()))
                            .boxed(),
                    );
                    let cells = items
                        .into_iter()
                        .map(|item| {
                            let lit =
                                chosen.as_deref() == Some(&item.name);
                            cell(item, lit, pick.clone())
                        })
                        .collect::<Vec<_>>();
                    parts.push(wrapped(cells, gap));
                }
                if parts.is_empty() {
                    parts.push(
                        description("Nothing matches.").boxed(),
                    );
                }
                column(parts).gap(gap / 2.0).width(percent(100.0))
            })
            .boxed()
        },
    )
    .within(column(()).gap(0.0).width(percent(100.0)))
    .boxed()
}

/// `cells` in rows that fill the width and go on in the next, packed
/// to the top.
pub fn wrapped(cells: Vec<AnyView>, gap: f32) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(
            row(cells)
                .gap(gap / 2.0)
                .width(percent(100.0))
                .align(AlignItems::FlexStart),
        );
        if let Some(mut layout) = cx.world.get_mut::<Node>(node) {
            layout.flex_wrap = FlexWrap::Wrap;
            layout.row_gap = px(gap / 2.0);
            layout.align_content = AlignContent::FlexStart;
        }
        node
    })
}

/// One asset: its picture over its name, lit while it is `lit`.
fn cell(item: Item, lit: bool, pick: impl Pick) -> AnyView {
    let name = item.name.clone();
    button(
        column((
            picture(item.kind, &name, PICTURE),
            column((centred(name.replace('_', " ")),))
                .width(percent(100.0))
                .height(px(NAME))
                .overflow(Overflow::clip())
                .align(AlignItems::Center),
        ))
        .gap(2.0)
        .align(AlignItems::Center),
    )
    .width(px(CELL))
    .rules(card_choice(lit))
    .on_activate(move |world| pick(world, &item))
    .boxed()
}

/// `text` as a note, its lines centred, wrapping where it is wider
/// than its place.
pub fn centred(text: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let built = cx.build(label(text.clone()).rules(note_rules));
        if let Some(mut layout) =
            cx.world.get_mut::<TextLayout>(built)
        {
            layout.justify = Justify::Center;
        }
        built
    })
}

/// Marks the plate of a shader's picture, which the window fills
/// with the shader on a sphere.
#[derive(Component, Clone, Debug)]
pub struct ShaderPicture(pub String);

/// The picture of the `kind` asset `name` in a square of `side`
/// pixels: a mesh's or a blueprint's rendering once drawn, a shader
/// on a sphere, else its kind's glyph. Showing it asks for it.
pub fn picture(kind: Kind, name: &str, side: f32) -> AnyView {
    let name = name.to_owned();
    if kind == Kind::Shader {
        return shader_plate(name, side);
    }
    AnyView::new(move |cx: &mut Cx| {
        Thumbnails::ask(cx.world, kind, &name);
        let read = name.clone();
        cx.build(
            keyed(
                resource::<Thumbnails, _>(move |thumbnails| {
                    thumbnails.picture(kind, &read)
                }),
                move |image: &Option<Handle<Image>>| {
                    square(image.clone(), kind.glyph(), side)
                },
            )
            .within(column(()).gap(0.0)),
        )
    })
}

/// A plate for the shader `name`, marked for the window to fill.
fn shader_plate(name: String, side: f32) -> AnyView {
    with_cx(move |cx| {
        let theme = cx.theme();
        column(())
            .width(px(side))
            .height(px(side))
            .fill(theme.palette.input)
            .radius(theme.space.round)
            .tagged(ShaderPicture(name.clone()))
    })
    .boxed()
}

/// A square plate with `image` filling it, or with `glyph_of` where
/// there is none.
fn square(
    image: Option<Handle<Image>>,
    glyph_of: Glyph,
    side: f32,
) -> AnyView {
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
                glyph(glyph_of, side * 0.4).tone(Tone::Dim).boxed(),
            ])),
        }
    })
}
