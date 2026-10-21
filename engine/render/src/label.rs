//! Words over the entities of a scene, in each view of it: a text of
//! the interface kept over the entity as the camera sees it. Over a
//! viewport they are children of its node; behind the dock, of a node
//! of their own covering the window under the interface, so the
//! dock's panels cover them.

use std::collections::HashMap;

use bevy::app::{App, PostUpdate};
use bevy::camera::Camera;
use bevy::camera::visibility::InheritedVisibility;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, SystemParam};
use bevy::picking::Pickable;
use bevy::text::{TextColor, TextFont};
use bevy::transform::TransformSystems;
use bevy::transform::components::GlobalTransform;
use bevy::ui::widget::Text;
use bevy::ui::{
    ComputedNode, GlobalZIndex, Node, PositionType, percent, px,
};
use hamlet_engine_shell::EngineTheme;

use crate::scene::{SceneCamera, ScenePart};
use crate::viewport::Viewport;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        (place, place_behind).after(TransformSystems::Propagate),
    );
}

/// The layer of the words over a scene behind the dock, under every
/// root of the interface.
const BEHIND_Z: i32 = -1;

/// On an entity of a scene: the words shown over it.
#[derive(Component, Clone, PartialEq, Debug)]
pub(crate) struct WorldLabel(pub(crate) String);

/// On a text over a view: the entity whose words it shows.
#[derive(Component, Clone, Copy, Debug)]
struct LabelOf(Entity);

/// On a node covering the window: it holds the words over the scene
/// of `camera`, a camera behind the dock.
#[derive(Component, Clone, Copy, Debug)]
struct Behind {
    camera: Entity,
}

/// The labelled entities and the texts that show them.
#[derive(SystemParam)]
struct Labels<'w, 's> {
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
    shown: Query<
        'w,
        's,
        (&'static LabelOf, &'static mut Node, &'static mut Text),
    >,
    theme: Option<Res<'w, EngineTheme>>,
    commands: Commands<'w, 's>,
}

impl Labels<'_, '_> {
    /// Keeps a text under `holder` over each labelled entity of the
    /// scene of `scene_camera` that `camera` sees, `scale` pixels of
    /// the interface to one of the camera's.
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
        let mut texts = children
            .into_iter()
            .flatten()
            .filter_map(|&child| {
                self.shown
                    .get(child)
                    .ok()
                    .map(|(of, ..)| (of.0, child))
            })
            .collect::<HashMap<_, _>>();
        for (entity, label, at, part, visible) in &self.labelled {
            if part.owner != scene_camera.owner || !visible.get() {
                continue;
            }
            let Ok(seen) =
                camera.world_to_viewport(place, at.translation())
            else {
                continue;
            };
            let (left, top) =
                (px(seen.x * scale), px(seen.y * scale));
            match texts.remove(&entity) {
                Some(text) => {
                    if let Ok((_, mut node, mut words)) =
                        self.shown.get_mut(text)
                    {
                        node.left = left;
                        node.top = top;
                        if words.0 != label.0 {
                            words.0.clone_from(&label.0);
                        }
                    }
                }
                None => {
                    let mut made = self.commands.spawn((
                        Text::new(label.0.clone()),
                        Node {
                            position_type: PositionType::Absolute,
                            left,
                            top,
                            ..Node::default()
                        },
                        LabelOf(entity),
                        Pickable::IGNORE,
                        ChildOf(holder),
                    ));
                    if let Some(theme) = &self.theme {
                        made.insert((
                            TextFont::from_font_size(
                                theme.text.small,
                            ),
                            TextColor(*theme.colors.text),
                        ));
                    }
                }
            }
        }
        for (_, gone) in texts {
            self.commands.entity(gone).despawn();
        }
    }
}

/// Keeps a text over each labelled entity a viewport's camera sees,
/// where the camera sees it.
fn place(
    viewports: Query<(
        Entity,
        &Viewport,
        &ComputedNode,
        Option<&Children>,
    )>,
    cameras: Query<(&Camera, &GlobalTransform, &SceneCamera)>,
    mut labels: Labels,
) {
    for (viewport, view, computed, children) in &viewports {
        let Ok(camera) = cameras.get(view.camera) else {
            continue;
        };
        labels.keep(
            viewport,
            children,
            camera,
            computed.inverse_scale_factor(),
        );
    }
}

/// Keeps the words over the scene of each camera behind the dock on
/// a layer of their own, made with the camera and gone with it.
fn place_behind(
    cameras: Query<(Entity, &Camera, &GlobalTransform, &SceneCamera)>,
    layers: Query<(Entity, &Behind, Option<&Children>)>,
    mut labels: Labels,
) {
    for (layer, behind, children) in &layers {
        match cameras.get(behind.camera) {
            Ok((_, camera, place, scene_camera))
                if camera.is_active =>
            {
                // The window's view is in the interface's pixels.
                labels.keep(
                    layer,
                    children,
                    (camera, place, scene_camera),
                    1.0,
                );
            }
            _ => labels.commands.entity(layer).despawn(),
        }
    }
    for (entity, camera, _, scene_camera) in &cameras {
        let behind = scene_camera.image.is_none() && camera.is_active;
        if behind
            && !layers.iter().any(|(_, of, ..)| of.camera == entity)
        {
            labels.commands.spawn((
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
