//! A hamlet template's town on its page, turning on its plinth as the
//! realm draws it apart from the world.

use bevy::camera::visibility::InheritedVisibility;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::With;
use bevy::ecs::system::{Query, ResMut};
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{AlignItems, ComputedNode, Node, px};
use bevy_fynix::views::FrameProps;
use hamlet_realm::diorama::{self, Dioramas, Model, STILL};
use hamlet_ui::styles::{bare, note};
use hamlet_ui::{AnyView, Cx};

use crate::bazaar::BazaarSettings;

/// On the picture of a template's town, which turns while it is seen.
#[derive(Component)]
pub(super) struct Turning(Model);

/// The most agents a town is drawn with, its mayor aside.
const MOST_AGENTS: u32 = 48;

/// The town of the seed `seed`, a mayor and `agents` more, turning on
/// its plinth.
pub(super) fn view(seed: u64, agents: u32) -> AnyView {
    let model = Model {
        seed,
        agents: agents.min(MOST_AGENTS) + 1,
    };
    AnyView::new(move |cx: &mut Cx| {
        let Some(image) = diorama::turning(cx.world) else {
            return cx.build(note("The town cannot be shown here."));
        };
        let tall = cx.world.resource::<BazaarSettings>().diorama;
        let wide = tall * STILL.x as f32 / STILL.y as f32;
        let within = cx.build(bare().align(AlignItems::Center));
        cx.world.spawn((
            Node {
                width: px(wide),
                height: px(tall),
                ..Node::default()
            },
            ImageNode::new(image).with_mode(NodeImageMode::Stretch),
            Turning(model),
            ChildOf(within),
        ));
        within
    })
}

/// Turns the town whose picture is in sight, and none while none is.
pub(super) fn turn(
    pictures: Query<
        (&Turning, &ComputedNode, &InheritedVisibility),
        With<ImageNode>,
    >,
    mut dioramas: ResMut<Dioramas>,
) {
    // A tab that is not in front is laid out with no size.
    let wanted = pictures
        .iter()
        .find(|(_, node, shown)| shown.get() && node.size().x > 0.0)
        .map(|(turning, ..)| turning.0);
    if dioramas.turning != wanted {
        dioramas.turning = wanted;
    }
}
