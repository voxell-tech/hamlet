//! The chips in a line of text, put where the line holds their
//! places in the frame it is drawn: fynix moves a chip's view to its
//! place only at the next layout, so a line that reflows would draw
//! its chips where the line was before.

use std::collections::HashMap;

use bevy::app::{App, Last};
use bevy::camera::visibility::Visibility;
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Affine2, Vec2};
use bevy::text::{InlineBox, InlineBoxKind, TextLayoutInfo};
use bevy::ui::{
    ComputedNode, Node, PositionType, UiGlobalTransform, Val,
};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ChipSizes>()
        .add_systems(Last, (place, (remember, show)).chain());
}

/// The frames a text that is laid out waits for its chips' sizes
/// before it shows without them.
const WAIT: u8 = 4;

/// On the node a text with chips is built in.
#[derive(Component)]
struct Chips {
    /// The words of each chip, in the order of the line.
    words: Vec<String>,
    /// The frames it has been laid out while hidden.
    waited: u8,
}

/// The size each chip was last given in a line, by its words, so that
/// a line built again lays out its chips from its first frame.
#[derive(Resource, Default)]
struct ChipSizes(HashMap<String, Vec2>);

/// Gives the chips of the text `body` holds the sizes they were last
/// given, and hides it until each of them has its size in the line,
/// so its words show once, where they stay.
pub(super) fn seed(
    world: &mut World,
    body: Entity,
    words: Vec<String>,
) {
    let boxes = boxes(world, body);
    let known = world.get_resource::<ChipSizes>().map(|sizes| {
        words
            .iter()
            .map(|words| sizes.0.get(words).copied())
            .collect::<Vec<_>>()
    });
    let mut whole = true;
    for (at, inline) in boxes.into_iter().enumerate() {
        match known
            .as_ref()
            .and_then(|known| known.get(at).copied()?)
        {
            Some(size) => {
                world.entity_mut(inline).insert(InlineBox {
                    kind: InlineBoxKind::InFlow,
                    size,
                });
            }
            None => whole = false,
        }
    }
    if !whole {
        world.entity_mut(body).insert(Visibility::Hidden);
    }
    world.entity_mut(body).insert(Chips { words, waited: 0 });
}

/// The inline boxes of the text `body` holds, in the order of its
/// line.
fn boxes(world: &World, body: Entity) -> Vec<Entity> {
    text_of(world, body)
        .and_then(|text| world.get::<Children>(text))
        .map(|kids| {
            kids.iter()
                .copied()
                .filter(|&kid| world.get::<InlineBox>(kid).is_some())
                .collect()
        })
        .unwrap_or_default()
}

fn text_of(world: &World, body: Entity) -> Option<Entity> {
    world.get::<Children>(body)?.first().copied()
}

/// Keeps the size each chip has been given.
fn remember(
    bodies: Query<(&Chips, &Children)>,
    texts: Query<&Children, With<TextLayoutInfo>>,
    boxes: Query<&InlineBox>,
    mut sizes: ResMut<ChipSizes>,
) {
    for (chips, kids) in &bodies {
        let Some(text) =
            kids.first().and_then(|&t| texts.get(t).ok())
        else {
            continue;
        };
        let given =
            text.iter().filter_map(|&kid| boxes.get(kid).ok());
        for (words, inline) in chips.words.iter().zip(given) {
            if inline.kind == InlineBoxKind::InFlow
                && sizes.0.get(words) != Some(&inline.size)
            {
                sizes.0.insert(words.clone(), inline.size);
            }
        }
    }
}

/// Shows a hidden text once its line was laid out with every chip at
/// its size, or after [`WAIT`] frames laid out.
fn show(
    mut bodies: Query<(&Children, &mut Visibility, &mut Chips)>,
    texts: Query<(&TextLayoutInfo, &ComputedNode, &Children)>,
    boxes: Query<&InlineBox>,
) {
    for (kids, mut seen, mut chips) in &mut bodies {
        if *seen != Visibility::Hidden {
            continue;
        }
        let Some((laid, node, inside)) =
            kids.first().and_then(|&text| texts.get(text).ok())
        else {
            continue;
        };
        if node.size().y > 0.0 {
            chips.waited = chips.waited.saturating_add(1);
        }
        let scale = node.inverse_scale_factor();
        let mut all = inside
            .iter()
            .filter_map(|&kid| Some((kid, boxes.get(kid).ok()?)));
        let ready = all.all(|(kid, inline)| {
            inline.kind == InlineBoxKind::InFlow
                && laid.inline_boxes.iter().any(
                    |(placed, _, rect)| {
                        *placed == kid
                            && (rect.width() * scale - inline.size.x)
                                .abs()
                                < 0.5
                    },
                )
        });
        if ready || chips.waited >= WAIT {
            seen.set_if_neq(Visibility::Inherited);
        }
    }
}

/// Moves each chip's view, and all inside it, to the place fynix set
/// for it from this frame's line.
fn place(
    bodies: Query<(Entity, &Children), With<Chips>>,
    nodes: Query<(&Node, &ComputedNode)>,
    mut transforms: Query<&mut UiGlobalTransform>,
    inside: Query<&Children>,
) {
    for (body, kids) in &bodies {
        let (Ok((_, frame)), Ok(at)) =
            (nodes.get(body), transforms.get(body))
        else {
            continue;
        };
        // The text is at the corner of the node, which has no
        // padding.
        let corner = at.translation - frame.size() / 2.0;
        let scale = frame.inverse_scale_factor();
        for &slot in kids.iter().skip(1) {
            let Ok((node, laid)) = nodes.get(slot) else {
                continue;
            };
            let (PositionType::Absolute, Some(left), Some(top)) = (
                node.position_type,
                logical(node.left),
                logical(node.top),
            ) else {
                continue;
            };
            let to = corner
                + Vec2::new(left, top) / scale
                + laid.size() / 2.0;
            let Ok(now) = transforms.get(slot) else {
                continue;
            };
            let by = to - now.translation;
            if by.length_squared() > 1e-4 {
                shift(slot, by, &mut transforms, &inside);
            }
        }
    }
}

/// Moves the node `at` and all inside it by `by`, in physical
/// pixels.
fn shift(
    at: Entity,
    by: Vec2,
    transforms: &mut Query<&mut UiGlobalTransform>,
    inside: &Query<&Children>,
) {
    if let Ok(mut transform) = transforms.get_mut(at) {
        let mut moved = Affine2::from(*transform);
        moved.translation += by;
        *transform = moved.into();
    }
    if let Ok(kids) = inside.get(at) {
        for &kid in kids {
            shift(kid, by, transforms, inside);
        }
    }
}

/// The logical pixels of `value`, none for anything else.
fn logical(value: Val) -> Option<f32> {
    match value {
        Val::Px(px) => Some(px),
        _ => None,
    }
}
