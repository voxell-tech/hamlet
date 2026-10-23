//! What is shown over the entities of a scene, in each view of it:
//! words and balloons, each a node of the interface kept centred over
//! its entity as the camera sees it, and the cards hung over an
//! entity. Over a viewport they are children of its node; behind the
//! dock, of a node of their own covering the window under the
//! interface, so the dock's panels cover them. The words an owner
//! shows beside the pointer follow it over the owner's views.

use std::collections::HashMap;

use bevy::app::{App, PostUpdate, Update};
use bevy::camera::Camera;
use bevy::camera::visibility::InheritedVisibility;
use bevy::color::Color;
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::{With, Without};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, SystemParam};
use bevy::ecs::world::EntityWorldMut;
use bevy::math::{Rect, Vec2};
use bevy::picking::Pickable;
use bevy::text::{FontSize, TextColor, TextFont};
use bevy::transform::TransformSystems;
use bevy::transform::components::GlobalTransform;
use bevy::ui::widget::Text;
use bevy::ui::{
    BackgroundColor, BorderColor, ComputedNode, GlobalZIndex, Node,
    PositionType, UiGlobalTransform, UiRect, percent, px,
};
use bevy::window::{PrimaryWindow, Window};
use hamlet_engine_shell::{CardPlace, EngineTheme, Hung};

use crate::balloon::{self, Balloon, Popping};
use crate::scene::{SceneCamera, SceneOf, ScenePart};
use crate::viewport::{ViewPointer, Viewport};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, balloon::pop).add_systems(
        PostUpdate,
        (place, place_behind, hang, follow_pointer, balloon::dots)
            .after(TransformSystems::Propagate),
    );
}

/// The layer of the words over a scene behind the dock, under every
/// root of the interface.
const BEHIND_Z: i32 = -1;

/// The layer of the words beside the pointer: over the panels and
/// the cards, under the menus.
const POINTER_Z: i32 = 960;

/// The space across and down inside the words beside the pointer.
const POINTER_PADDING: (f32, f32) = (8.0, 3.0);

/// On an entity of a scene: the words shown over it.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct WorldLabel {
    pub text: String,
    pub look: LabelLook,
    /// Pixels between the entity and the label's foot.
    pub gap: f32,
    /// The farthest the camera is from the entity for it to be
    /// shown, or any distance at 0.
    pub far: f32,
}

/// How a [`WorldLabel`] looks.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LabelLook {
    Plain,
    Sign,
    Tag,
}

/// On the root of a scene: the words shown beside the pointer while
/// it is over one of the owner's views.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct PointerLabel {
    pub text: String,
    pub color: Color,
    pub back: Color,
    pub size: f32,
    /// Where its top left is from the pointer.
    pub offset: Vec2,
}

/// On a card floating over the window: the entity of a scene it
/// hangs `gap` pixels over.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct HangsOn {
    pub entity: Entity,
    pub gap: f32,
}

/// What a node over a view shows of an entity.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Mark {
    Label(LabelLook),
    Balloon,
}

/// On a node over a view: the entity it shows, and what of it.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct MarkOf {
    entity: Entity,
    mark: Mark,
}

/// On a node covering the window: it holds what is over the scene of
/// `camera`, a camera behind the dock.
#[derive(Component, Clone, Copy, Debug)]
struct Behind {
    camera: Entity,
}

/// On the words beside the pointer: the root of the scene whose they
/// are, and their text.
#[derive(Component, Clone, Copy, Debug)]
struct BesidePointer {
    root: Entity,
    text: Entity,
}

/// What is shown over entities, and the nodes that show it.
#[derive(SystemParam)]
struct Marks<'w, 's> {
    labelled: Query<
        'w,
        's,
        (
            Entity,
            &'static WorldLabel,
            &'static GlobalTransform,
            &'static ScenePart,
            &'static InheritedVisibility,
        ),
    >,
    ballooned: Query<
        'w,
        's,
        (
            Entity,
            &'static Balloon,
            &'static Popping,
            &'static GlobalTransform,
            &'static ScenePart,
            &'static InheritedVisibility,
        ),
    >,
    shown: Query<
        'w,
        's,
        (&'static MarkOf, &'static mut Node, &'static ComputedNode),
    >,
    children: Query<'w, 's, &'static Children>,
    parts: balloon::Parts<'w, 's>,
    theme: Option<Res<'w, EngineTheme>>,
    commands: Commands<'w, 's>,
}

impl Marks<'_, '_> {
    /// Keeps a node under `holder` over each labelled or ballooned
    /// entity of the scene of `scene_camera` that `camera` sees,
    /// `scale` pixels of the interface to one of the camera's.
    fn keep(
        &mut self,
        holder: Entity,
        children: Option<&Children>,
        (camera, place, scene_camera): (
            &Camera,
            &GlobalTransform,
            &SceneCamera,
        ),
        scale: f32,
    ) {
        let mut nodes = children
            .into_iter()
            .flatten()
            .filter_map(|&child| {
                self.shown
                    .get(child)
                    .ok()
                    .map(|(of, ..)| ((of.entity, of.mark), child))
            })
            .collect::<HashMap<_, _>>();
        let eye = place.translation();
        let owner = scene_camera.owner;
        let seen = |at: &GlobalTransform, far: f32| {
            let point = at.translation();
            if far > 0.0 && eye.distance(point) > far {
                return None;
            }
            camera
                .world_to_viewport(place, point)
                .ok()
                .map(|seen| seen * scale)
        };
        let mut wanted = Vec::new();
        for (entity, label, at, part, visible) in &self.labelled {
            if part.owner != owner || !visible.get() {
                continue;
            }
            if let Some(point) = seen(at, label.far) {
                wanted.push((
                    entity,
                    Mark::Label(label.look),
                    point,
                    label.gap,
                ));
            }
        }
        for (entity, shown, _, at, part, visible) in &self.ballooned {
            if part.owner != owner || !visible.get() {
                continue;
            }
            if let Some(point) = seen(at, shown.far) {
                wanted.push((
                    entity,
                    Mark::Balloon,
                    point,
                    shown.gap,
                ));
            }
        }
        for (entity, mark, point, gap) in wanted {
            match nodes.remove(&(entity, mark)) {
                Some(node) => {
                    self.update(node, entity, mark, point, gap)
                }
                None => self.spawn(holder, entity, mark, point),
            }
        }
        for (_, gone) in nodes {
            self.commands.entity(gone).despawn();
        }
    }

    /// Moves `node` over `point` and gives it what `entity` shows
    /// now.
    fn update(
        &mut self,
        node: Entity,
        entity: Entity,
        mark: Mark,
        point: Vec2,
        gap: f32,
    ) {
        if let Ok((_, mut placed, computed)) =
            self.shown.get_mut(node)
        {
            let size =
                computed.size() * computed.inverse_scale_factor();
            let corner = (point
                - Vec2::new(size.x * 0.5, size.y + gap))
            .round();
            put(&mut placed, corner);
        }
        match mark {
            Mark::Label(_) => {
                let Ok((_, label, ..)) = self.labelled.get(entity)
                else {
                    return;
                };
                let text =
                    self.children.get(node).ok().and_then(
                        |children| children.first().copied(),
                    );
                if let Some(text) = text {
                    self.parts.write(text, &label.text);
                }
            }
            Mark::Balloon => {
                let (Ok((_, shown, popping, ..)), Some(theme)) =
                    (self.ballooned.get(entity), self.theme.as_ref())
                else {
                    return;
                };
                self.parts.dress(node, shown, popping, theme);
            }
        }
    }

    /// Makes a node under `holder` showing what `entity` shows, at
    /// `point` until it is laid out.
    fn spawn(
        &mut self,
        holder: Entity,
        entity: Entity,
        mark: Mark,
        point: Vec2,
    ) {
        let Some(theme) =
            self.theme.as_ref().map(|theme| (**theme).clone())
        else {
            return;
        };
        let of = MarkOf { entity, mark };
        let made = match mark {
            Mark::Label(look) => {
                let Ok((_, label, ..)) = self.labelled.get(entity)
                else {
                    return;
                };
                let text = label.text.clone();
                Made::Label(text, look)
            }
            Mark::Balloon => Made::Balloon,
        };
        // Made only while its holder is there: a holder gone in the
        // same frame would leave it at the window's root.
        self.commands.entity(holder).queue_silenced(
            move |mut holder: EntityWorldMut| {
                let id = holder.id();
                holder.world_scope(|world| {
                    let node = match made {
                        Made::Label(text, look) => {
                            label_node(world, &theme, text, look)
                        }
                        Made::Balloon => {
                            balloon::spawn(world, &theme)
                        }
                    };
                    let mut node = world.entity_mut(node);
                    if let Some(mut placed) = node.get_mut::<Node>() {
                        placed.position_type = PositionType::Absolute;
                        placed.left = px(point.x);
                        placed.top = px(point.y);
                    }
                    node.insert((of, ChildOf(id)));
                });
            },
        );
    }
}

/// What a node over a view is made as.
enum Made {
    Label(String, LabelLook),
    Balloon,
}

/// A node of `text` as `look` draws it.
fn label_node(
    world: &mut bevy::ecs::world::World,
    theme: &EngineTheme,
    text: String,
    look: LabelLook,
) -> Entity {
    let (looks, colors) = (&theme.world, &theme.colors);
    let (fill, edge, ink, size, pad) = match look {
        LabelLook::Plain => (
            Color::NONE,
            Color::NONE,
            *colors.text,
            theme.text.small,
            (0.0, 0.0),
        ),
        LabelLook::Sign => (
            *looks.sign,
            *looks.sign_edge,
            *looks.sign_text,
            looks.sign_size,
            (looks.sign_padding_x, looks.sign_padding_y),
        ),
        LabelLook::Tag => (
            *looks.name_tag,
            *looks.name_tag_edge,
            *looks.name_tag_text,
            looks.name_tag_size,
            (looks.name_tag_padding_x, looks.name_tag_padding_y),
        ),
    };
    let line = if look == LabelLook::Plain { 0.0 } else { 1.0 };
    world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(px(pad.0), px(pad.1)),
                border: UiRect::all(px(line)),
                ..Node::default()
            },
            BackgroundColor(fill),
            BorderColor::all(edge),
            Pickable::IGNORE,
        ))
        .with_children(|node| {
            node.spawn((
                Text::new(text),
                TextFont::from_font_size(size),
                TextColor(ink),
                Pickable::IGNORE,
            ));
        })
        .id()
}

/// Moves a node placed by hand to `corner`.
fn put(node: &mut Node, corner: Vec2) {
    let (left, top) = (px(corner.x), px(corner.y));
    if node.left != left || node.top != top {
        node.left = left;
        node.top = top;
    }
}

/// Keeps what is shown over each entity a viewport's camera sees,
/// where the camera sees it.
fn place(
    viewports: Query<(
        Entity,
        &Viewport,
        &ComputedNode,
        Option<&Children>,
    )>,
    cameras: Query<(&Camera, &GlobalTransform, &SceneCamera)>,
    mut marks: Marks,
) {
    for (viewport, view, computed, children) in &viewports {
        let Ok(camera) = cameras.get(view.camera) else {
            // Its camera is gone, and what was over it with it.
            for &child in children.into_iter().flatten() {
                if marks.shown.contains(child) {
                    marks.commands.entity(child).despawn();
                }
            }
            continue;
        };
        marks.keep(
            viewport,
            children,
            camera,
            computed.inverse_scale_factor(),
        );
    }
}

/// Keeps what is shown over the scene of each camera behind the dock
/// on a layer of its own, made with the camera and gone with it.
fn place_behind(
    cameras: Query<(Entity, &Camera, &GlobalTransform, &SceneCamera)>,
    layers: Query<(Entity, &Behind, Option<&Children>)>,
    mut marks: Marks,
) {
    for (layer, behind, children) in &layers {
        match cameras.get(behind.camera) {
            Ok((_, camera, place, scene_camera))
                if camera.is_active =>
            {
                // The window's view is in the interface's pixels.
                marks.keep(
                    layer,
                    children,
                    (camera, place, scene_camera),
                    1.0,
                );
            }
            _ => marks.commands.entity(layer).despawn(),
        }
    }
    for (entity, camera, _, scene_camera) in &cameras {
        let behind = scene_camera.image.is_none() && camera.is_active;
        if behind
            && !layers.iter().any(|(_, of, ..)| of.camera == entity)
        {
            marks.commands.spawn((
                Behind { camera: entity },
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100.0),
                    height: percent(100.0),
                    ..Node::default()
                },
                GlobalZIndex(BEHIND_Z),
                Pickable::IGNORE,
            ));
        }
    }
}

/// Places each card hung over an entity over it, in the first view
/// of its scene shown: a viewport with room, or the window behind the
/// dock.
fn hang(
    mut cards: Query<(&HangsOn, &mut CardPlace)>,
    entities: Query<(
        &GlobalTransform,
        &ScenePart,
        &InheritedVisibility,
    )>,
    cameras: Query<(Entity, &Camera, &GlobalTransform, &SceneCamera)>,
    viewports: Query<(&Viewport, &ComputedNode, &UiGlobalTransform)>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let window =
        windows.iter().next().map_or(Rect::default(), |window| {
            Rect::new(0.0, 0.0, window.width(), window.height())
        });
    for (hangs, mut place) in &mut cards {
        let hung = entities.get(hangs.entity).ok().and_then(
            |(at, part, visible)| {
                cameras.iter().find_map(
                    |(entity, camera, eye, of)| {
                        if of.owner != part.owner || !camera.is_active
                        {
                            return None;
                        }
                        let (clip, scale) = match &of.image {
                            None => (window, 1.0),
                            Some(_) => {
                                let (_, computed, placed) = viewports
                                    .iter()
                                    .find(|(view, computed, _)| {
                                        view.camera == entity
                                            && !computed.is_empty()
                                    })?;
                                let scale =
                                    computed.inverse_scale_factor();
                                let (_, _, centre) = placed
                                    .to_scale_angle_translation();
                                let rect = Rect::from_center_size(
                                    centre * scale,
                                    computed.size() * scale,
                                );
                                (rect, scale)
                            }
                        };
                        let point = visible
                            .get()
                            .then(|| {
                                camera.world_to_viewport(
                                    eye,
                                    at.translation(),
                                )
                            })
                            .and_then(Result::ok)
                            .map(|seen| clip.min + seen * scale);
                        Some(Hung {
                            clip,
                            at: point,
                            gap: hangs.gap,
                        })
                    },
                )
            },
        );
        let now = CardPlace::Hung(hung.unwrap_or(Hung {
            clip: window,
            at: None,
            gap: hangs.gap,
        }));
        place.set_if_neq(now);
    }
}

/// Keeps the words an owner shows beside the pointer by it while the
/// pointer is over one of the owner's views.
fn follow_pointer(
    scenes: Query<(Entity, &SceneOf, Option<&PointerLabel>)>,
    cameras: Query<(&SceneCamera, &ViewPointer)>,
    mut shown: Query<
        (Entity, &BesidePointer, &mut Node, &mut BackgroundColor),
        Without<Text>,
    >,
    mut texts: Query<(&mut Text, &mut TextColor, &mut TextFont)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut commands: Commands,
) {
    let cursor =
        windows.iter().next().and_then(Window::cursor_position);
    let wanted = |root: Entity| -> Option<(&PointerLabel, Vec2)> {
        let (_, of, label) = scenes.get(root).ok()?;
        let over = cameras.iter().any(|(camera, pointer)| {
            camera.owner == of.0 && pointer.at.is_some()
        });
        Some((label?, cursor.filter(|_| over)?))
    };
    for (node, beside, mut placed, mut back) in &mut shown {
        let Some((label, at)) = wanted(beside.root) else {
            commands.entity(node).despawn();
            continue;
        };
        put(&mut placed, at + label.offset);
        back.set_if_neq(BackgroundColor(label.back));
        if let Ok((mut text, mut ink, mut font)) =
            texts.get_mut(beside.text)
        {
            if text.0 != label.text {
                text.0.clone_from(&label.text);
            }
            ink.set_if_neq(TextColor(label.color));
            let size = FontSize::Px(label.size);
            if font.font_size != size {
                font.font_size = size;
            }
        }
    }
    for (root, ..) in &scenes {
        let Some((label, at)) = wanted(root) else {
            continue;
        };
        if shown.iter().any(|(_, beside, ..)| beside.root == root) {
            continue;
        }
        let corner = at + label.offset;
        let text = commands
            .spawn((
                Text::new(label.text.clone()),
                TextFont::from_font_size(label.size),
                TextColor(label.color),
                Pickable::IGNORE,
            ))
            .id();
        commands
            .spawn((
                BesidePointer { root, text },
                Node {
                    position_type: PositionType::Absolute,
                    left: px(corner.x),
                    top: px(corner.y),
                    padding: UiRect::axes(
                        px(POINTER_PADDING.0),
                        px(POINTER_PADDING.1),
                    ),
                    ..Node::default()
                },
                BackgroundColor(label.back),
                GlobalZIndex(POINTER_Z),
                Pickable::IGNORE,
            ))
            .add_child(text);
    }
}
