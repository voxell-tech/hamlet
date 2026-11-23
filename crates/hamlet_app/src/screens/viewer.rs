//! An image shown over the window on a veil.

use bevy::app::{App, PostUpdate, Update};
use bevy::color::Alpha;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, Single};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::{UVec2, Vec2};
use bevy::picking::events::PointerClick;
use bevy::picking::pointer::PointerButton;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BorderRadius, ComputedNode, JustifyContent, Node,
    PositionType, UiRect, UiSystems, Val, percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, frame, label, overlay, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_core::overlays::OverlayAppExt;
use hamlet_protocol::client::ImageId;
use hamlet_ui::styles::{ICON, bare, icon_button};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::pictures::{Drawn, Pictures, Source, State, want};

/// How much of the window the veil under the image hides.
const VEIL: f32 = 0.85;

pub fn plugin(app: &mut App) {
    app.init_resource::<Viewing>()
        .add_overlay(50, || view().boxed())
        .add_cover(|world| {
            world
                .get_resource::<Viewing>()
                .is_some_and(Viewing::is_open)
        })
        .add_observer(click_veil)
        .add_systems(Update, close_on_escape.run_if(is_viewing))
        .add_systems(
            PostUpdate,
            fit.run_if(is_viewing).before(UiSystems::Layout),
        );
}

/// The image shown, and what had the keyboard before.
#[derive(Resource, Default)]
pub struct Viewing {
    image: Option<ImageId>,
    focus: Option<Entity>,
}

impl Viewing {
    pub(crate) fn is_open(&self) -> bool {
        self.image.is_some()
    }
}

/// Shows `image` over the window, holding the keyboard.
pub(crate) fn open(world: &mut World, image: ImageId) {
    want(world, &Source::Image(image.clone()));
    want(world, &Source::Original(image.clone()));
    let focus = world.get_resource_mut::<InputFocus>().and_then(
        |mut focus| {
            let had = focus.get();
            focus.clear();
            had
        },
    );
    let mut viewing = world.resource_mut::<Viewing>();
    if viewing.image.is_none() {
        viewing.focus = focus;
    }
    viewing.image = Some(image);
}

/// Closes the viewer, and gives the keyboard back to what had it.
fn close(world: &mut World) {
    let mut viewing = world.resource_mut::<Viewing>();
    viewing.image = None;
    let focus = viewing.focus.take();
    if let Some(focus) = focus
        && let Some(mut input) =
            world.get_resource_mut::<InputFocus>()
        && input.get().is_none()
    {
        input.set(focus, FocusCause::Navigated);
    }
}

fn is_viewing(viewing: Res<Viewing>) -> bool {
    viewing.is_open()
}

fn close_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        commands.queue(close);
    }
}

#[derive(Component)]
struct Veil;

/// The picture drawn of the image, sized in the room the veil leaves.
#[derive(Component)]
struct Shown {
    /// Its pixels, across and down.
    size: UVec2,
    /// Whether it is the image at its full size.
    full: bool,
}

/// Closes the viewer on a click on the veil itself.
fn click_veil(
    click: On<PointerClick>,
    veils: Query<(), With<Veil>>,
    mut commands: Commands,
) {
    if click.button == PointerButton::Primary
        && click.original_event_target() == click.entity
        && veils.contains(click.entity)
    {
        commands.queue(close);
    }
}

/// `size` fitted in `room`, at most `largest` times larger.
fn fitted(size: Vec2, room: Vec2, largest: f32) -> Vec2 {
    if size.min_element() <= 0.0 || room.min_element() <= 0.0 {
        return Vec2::ZERO;
    }
    size * (room / size).min_element().min(largest)
}

/// Sizes the picture in the room the veil leaves.
fn fit(
    veil: Single<&ComputedNode, With<Veil>>,
    mut shown: Query<(&Shown, &mut Node)>,
) {
    let scale = veil.inverse_scale_factor();
    let room = veil.content_box().size() * scale;
    for (shown, mut node) in &mut shown {
        let largest = match shown.full {
            true => scale,
            false => 1.0,
        };
        let size = fitted(shown.size.as_vec2(), room, largest);
        let (width, height) = (px(size.x), px(size.y));
        if node.width != width || node.height != height {
            node.width = width;
            node.height = height;
        }
    }
}

/// The best of an image that has come.
#[derive(Clone, PartialEq)]
enum Best {
    /// Nothing yet.
    Coming,
    /// Itself, or its thumbnail until it comes.
    Drawn { drawn: Drawn, full: bool },
    /// Neither could be had.
    Missing,
}

impl Best {
    fn of(pictures: &Pictures, image: &ImageId) -> Self {
        let original =
            pictures.state(&Source::Original(image.clone()));
        let thumbnail = pictures.state(&Source::Image(image.clone()));
        match (original, thumbnail) {
            (State::Ready(drawn), _) => {
                Best::Drawn { drawn, full: true }
            }
            (_, State::Ready(drawn)) => {
                Best::Drawn { drawn, full: false }
            }
            (State::Failed, State::Failed) => Best::Missing,
            _ => Best::Coming,
        }
    }
}

/// The picture of `image`, or what stands for it until it comes.
fn picture(image: ImageId) -> impl View {
    let of = image.clone();
    let state =
        resource::<Pictures, _>(move |all| Best::of(all, &of));
    keyed(state, move |state| match state {
        Best::Drawn { drawn, full } => {
            let (drawn, full, image) =
                (drawn.clone(), *full, image.clone());
            AnyView::new(move |cx: &mut Cx| {
                let theme = cx.theme();
                let (round, shadow) =
                    (theme.space.round, theme.shadow());
                let source = match full {
                    true => Source::Original(image.clone()),
                    false => Source::Image(image.clone()),
                };
                let node = cx.spawn();
                cx.world.entity_mut(node).insert((
                    Node {
                        border_radius: BorderRadius::all(px(round)),
                        ..Node::default()
                    },
                    ImageNode::new(drawn.image.clone())
                        .with_mode(NodeImageMode::Stretch),
                    shadow,
                    Shown {
                        size: drawn.size,
                        full,
                    },
                    source,
                ));
                node
            })
        }
        Best::Coming => {
            glyph(Glyph::Progress, ICON).tone(Tone::Dim).boxed()
        }
        Best::Missing => label("The image could not be had.")
            .tone(Tone::Dim)
            .boxed(),
    })
    .within(bare())
}

/// The image on its veil with the button that closes it.
fn shown(image: ImageId) -> AnyView {
    with_cx(move |cx| {
        let theme = cx.theme();
        let (edge, veil) = (
            theme.space.panel,
            theme.palette.shadow.with_alpha(VEIL),
        );
        let close = row((icon_button(Glyph::Cross, "Close", close),))
            .position(PositionType::Absolute)
            .inset(UiRect::new(
                Val::Auto,
                px(edge),
                px(edge),
                Val::Auto,
            ));
        row((picture(image.clone()), close))
            .width(percent(100.0))
            .height(percent(100.0))
            .padding(UiRect::all(px(edge * 3.0)))
            .align(AlignItems::Center)
            .justify(JustifyContent::Center)
            .fill(veil)
            .tagged(Veil)
    })
    .boxed()
}

/// The viewer, over the window while an image is shown.
pub fn view() -> impl View {
    keyed(
        resource::<Viewing, _>(|viewing| viewing.image.clone()),
        |viewing| match viewing {
            Some(image) => shown(image.clone()),
            None => frame().boxed(),
        },
    )
    .within(overlay(()))
}
