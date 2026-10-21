//! Words over the entities of a scene, in each viewport of it: a text
//! of the interface kept over the entity as the camera sees it.

use std::collections::HashMap;

use bevy::app::{App, PostUpdate};
use bevy::camera::Camera;
use bevy::camera::visibility::InheritedVisibility;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::text::{TextColor, TextFont};
use bevy::transform::TransformSystems;
use bevy::transform::components::GlobalTransform;
use bevy::ui::widget::Text;
use bevy::ui::{ComputedNode, Node, PositionType, px};
use hamlet_engine_shell::EngineTheme;

use crate::scene::{SceneCamera, ScenePart};
use crate::viewport::Viewport;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        place.after(TransformSystems::Propagate),
    );
}

/// On an entity of a scene: the words shown over it.
#[derive(Component, Clone, PartialEq, Debug)]
pub(crate) struct WorldLabel(pub(crate) String);

/// On a text over a viewport: the entity whose words it shows.
#[derive(Component, Clone, Copy, Debug)]
struct LabelOf(Entity);

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
    labelled: Query<(
        Entity,
        &WorldLabel,
        &GlobalTransform,
        &ScenePart,
        &InheritedVisibility,
    )>,
    mut shown: Query<(&LabelOf, &mut Node, &mut Text)>,
    theme: Option<Res<EngineTheme>>,
    mut commands: Commands,
) {
    for (viewport, view, computed, children) in &viewports {
        let Ok((camera, place, scene_camera)) =
            cameras.get(view.camera)
        else {
            continue;
        };
        let mut texts = children
            .into_iter()
            .flatten()
            .filter_map(|&child| {
                shown.get(child).ok().map(|(of, ..)| (of.0, child))
            })
            .collect::<HashMap<_, _>>();
        let scale = computed.inverse_scale_factor();
        for (entity, label, at, part, visible) in &labelled {
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
                        shown.get_mut(text)
                    {
                        node.left = left;
                        node.top = top;
                        if words.0 != label.0 {
                            words.0.clone_from(&label.0);
                        }
                    }
                }
                None => {
                    let mut made = commands.spawn((
                        Text::new(label.0.clone()),
                        Node {
                            position_type: PositionType::Absolute,
                            left,
                            top,
                            ..Node::default()
                        },
                        LabelOf(entity),
                        ChildOf(viewport),
                    ));
                    if let Some(theme) = &theme {
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
            commands.entity(gone).despawn();
        }
    }
}
