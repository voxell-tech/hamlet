//! The layer a panel's controls draw their outer glow on, so that
//! the glow is cut at the panel's border and not at its padding.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::With;
use bevy::ecs::system::Query;
use bevy::picking::Pickable;
use bevy::ui::{
    ComputedNode, Overflow, OverrideClip, PositionType,
    UiGlobalTransform, UiRect, px,
};
use bevy_fynix::views::{BehaviorExt, frame};

use crate::View;

/// A panel whose controls draw their outer glow on its
/// [`GlowLayer`].
#[derive(Component)]
pub(crate) struct GlowPanel;

/// The node over a panel's whole area that holds the outer glow of
/// its controls.
#[derive(Component)]
pub(super) struct GlowLayer;

/// The layer of a panel, under the panel's contents. It ignores the
/// clip of the panel, which cuts at the content box, and cuts at its
/// own edge, which is the panel's.
pub(crate) fn glow_layer() -> impl View {
    frame()
        .position(PositionType::Absolute)
        .inset(UiRect::all(px(0.0)))
        .overflow(Overflow::clip())
        .tagged(GlowLayer)
        .tagged(OverrideClip)
        .tagged(Pickable::IGNORE)
}

/// The layer of the nearest panel above `control` that has one and
/// has been laid out.
pub(super) fn layer_of(
    control: Entity,
    ancestors: &Query<&ChildOf>,
    panels: &Query<&Children, With<GlowPanel>>,
    layers: &Query<
        (&ComputedNode, &UiGlobalTransform),
        With<GlowLayer>,
    >,
) -> Option<Entity> {
    let mut at = control;
    while let Ok(above) = ancestors.get(at) {
        at = above.parent();
        let Ok(within) = panels.get(at) else {
            continue;
        };
        return within.iter().copied().find(|child| {
            layers
                .get(*child)
                .is_ok_and(|(node, _)| node.size.min_element() > 0.0)
        });
    }
    None
}
