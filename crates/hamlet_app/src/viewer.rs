//! The realm's viewer in a panel: its picture, turned by a drag and
//! brought nearer by the wheel, with RESET and BIGGER or SMALLER
//! under it. What it shows is the realm's [`Viewer`]'s to say.

use bevy::app::{App, Update};
use bevy::camera::visibility::InheritedVisibility;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::input::mouse::MouseScrollPixelsPerLine;
use bevy::math::Vec2;
use bevy::picking::events::{PointerDrag, PointerScroll};
use bevy::picking::pointer::PointerButton;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{AlignItems, ComputedNode, Node, Val, percent, px};
use bevy_fynix::views::{FrameProps, column, row};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_realm::viewer::{self, Viewer};
use hamlet_ui::styles::{bare, note, quiet};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, Cx};

/// The tallest the viewer is drawn bigger, in shares of the window's
/// height.
const BIGGEST: f32 = 70.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<ViewerSize>()
        .add_systems(Update, sight.run_if(resource_exists::<Viewer>));
}

/// How big the viewer is drawn.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ViewerSize {
    /// As wide and tall as the view asks.
    #[default]
    Small,
    /// As wide as the panel.
    Big,
}

/// On the picture of the viewer.
#[derive(Component)]
struct Seen;

/// The viewer's picture, `small` pixels while it is small, and the
/// ways to put it back and make it bigger.
pub fn view(small: Vec2) -> AnyView {
    let picture =
        keyed(resource::<ViewerSize, _>(|size| *size), move |size| {
            picture(*size, small)
        })
        .within(bare());
    let controls =
        keyed(resource::<ViewerSize, _>(|size| *size), |size| {
            let (which, words, other) = match size {
                ViewerSize::Small => {
                    (Glyph::Move, "BIGGER", ViewerSize::Big)
                }
                ViewerSize::Big => {
                    (Glyph::Move, "SMALLER", ViewerSize::Small)
                }
            };
            column((
                row((
                    quiet(Glyph::Aim, "RESET", |world| {
                        if let Some(mut viewer) =
                            world.get_resource_mut::<Viewer>()
                        {
                            viewer.reset();
                        }
                    }),
                    quiet(which, words, move |world| {
                        *world.resource_mut::<ViewerSize>() = other;
                    }),
                ))
                .gap(6.0)
                .align(AlignItems::Center),
                note("Drag to turn it, scroll to come nearer."),
            ))
            .gap(6.0)
            .boxed()
        })
        .within(bare());
    column((picture, controls)).gap(6.0).boxed()
}

fn picture(size: ViewerSize, small: Vec2) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let Some(image) = viewer::image(cx.world) else {
            return cx.build(note("It cannot be shown here."));
        };
        let node = match size {
            ViewerSize::Small => Node {
                width: px(small.x),
                height: px(small.y),
                ..Node::default()
            },
            ViewerSize::Big => Node {
                width: percent(100.0),
                aspect_ratio: Some(small.x / small.y),
                max_height: Val::Vh(BIGGEST),
                ..Node::default()
            },
        };
        let within = match size {
            ViewerSize::Small => {
                cx.build(bare().align(AlignItems::Center))
            }
            ViewerSize::Big => cx.build(bare()),
        };
        cx.world
            .spawn((
                node,
                ImageNode::new(image)
                    .with_mode(NodeImageMode::Stretch),
                Seen,
                ChildOf(within),
            ))
            .observe(drag)
            .observe(wheel);
        within
    })
}

fn drag(drag: On<PointerDrag>, mut viewer: ResMut<Viewer>) {
    if drag.button == PointerButton::Primary {
        viewer.drag(drag.delta);
    }
}

/// Brings the camera nearer, the wheel kept from the panel's scroll.
fn wheel(
    mut scroll: On<PointerScroll>,
    per_line: Res<MouseScrollPixelsPerLine>,
    mut viewer: ResMut<Viewer>,
) {
    viewer.wheel(bevy_fynix::scroll::wheel(&scroll, *per_line).0.y);
    scroll.propagate(false);
}

/// Tells the realm the pixels of the viewer's picture in sight, and
/// none while none is.
fn sight(
    pictures: Query<
        (&ComputedNode, &InheritedVisibility),
        With<Seen>,
    >,
    mut viewer: ResMut<Viewer>,
) {
    // A tab that is not in front is laid out with no size.
    let wanted = pictures
        .iter()
        .find(|(node, shown)| shown.get() && node.size().x > 0.0)
        .map(|(node, _)| node.size().round().as_uvec2());
    if viewer.sight != wanted {
        viewer.sight = wanted;
    }
}
