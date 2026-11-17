//! An image of a chief's line shown over the window on a veil: its
//! thumbnail at first, then the image itself once it is had, fitted
//! to the window and never drawn past its own pixels.

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
use hamlet_protocol::client::ImageId;
use hamlet_ui::styles::{ICON, bare, icon_button};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::pictures::{Drawn, Pictures, Source, State, want};

/// How much of the window the veil under the image hides.
const VEIL: f32 = 0.85;

pub fn plugin(app: &mut App) {
    app.init_resource::<Viewing>()
        .add_observer(click_veil)
        .add_systems(Update, close_on_escape.run_if(is_viewing))
        .add_systems(
            PostUpdate,
            fit.run_if(is_viewing).before(UiSystems::Layout),
        );
}

/// The image shown over the window, if one is, and what had the
/// keyboard before it opened.
#[derive(Resource, Default)]
pub struct Viewing {
    image: Option<ImageId>,
    focus: Option<Entity>,
}

impl Viewing {
    /// Whether an image is shown, which takes the keyboard: its
    /// Escape closes the viewer and nothing under it.
    pub(crate) fn is_open(&self) -> bool {
        self.image.is_some()
    }
}

/// Shows `image` over the window, and asks for it at its full size.
/// The keyboard leaves what had it, a field included, until the
/// viewer closes.
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

/// The veil the image is shown on, which a click on closes it.
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

/// Closes the viewer on a click on the veil itself, not on what it
/// holds.
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

/// The size, in logical pixels, of a picture of `size` drawn in
/// `room`, its proportions kept, made at most `largest` times larger.
fn fitted(size: Vec2, room: Vec2, largest: f32) -> Vec2 {
    if size.min_element() <= 0.0 || room.min_element() <= 0.0 {
        return Vec2::ZERO;
    }
    size * (room / size).min_element().min(largest)
}

/// Sizes the picture shown in the room the veil leaves. The image at
/// its full size is drawn at most a pixel of the screen to each of
/// its own; its thumbnail, until it comes, at most a logical pixel to
/// each of its own, so that it is never stretched past being read.
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

#[cfg(test)]
mod tests {
    use hamlet_ui::theme::HamletTheme;

    use super::*;
    use crate::chat::ChatSettings;
    use crate::testing::panel_app;

    #[test]
    fn the_viewer_shows_the_image_on_its_veil_and_closes_on_escape() {
        use std::time::{Duration, Instant};

        use bevy::asset::Assets;
        use bevy::ecs::hierarchy::ChildOf;
        use bevy::image::Image;
        use hamlet_client::images::ImageArrived;
        use hamlet_protocol::client::ImageSize;

        let mut app = panel_app();
        app.init_resource::<ChatSettings>()
            .init_resource::<Assets<Image>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins((crate::pictures::plugin, plugin));
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), view());
        let image = ImageId::try_from("c".repeat(64)).unwrap();
        let veils = |app: &mut App| {
            app.world_mut()
                .query_filtered::<Entity, With<Veil>>()
                .iter(app.world())
                .collect::<Vec<_>>()
        };

        open(app.world_mut(), image.clone());
        app.update();
        assert_eq!(veils(&mut app).len(), 1);
        // The image itself is asked for beside its thumbnail.
        let pictures = app.world().resource::<Pictures>();
        assert_eq!(
            pictures.state(&Source::Original(image.clone())),
            State::Loading
        );

        app.world_mut().trigger(ImageArrived {
            image: image.clone(),
            size: ImageSize::Original,
            mime: "image/png".into(),
            bytes: crate::testing::png(UVec2::new(40, 30)),
        });
        let start = Instant::now();
        let picture = loop {
            app.update();
            let shown = app
                .world_mut()
                .query_filtered::<Entity, With<Shown>>()
                .iter(app.world())
                .next();
            if let Some(shown) = shown {
                break shown;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        };
        // The picture hangs on the veil, not on a root of its own.
        let veil = veils(&mut app)[0];
        let mut parents = app.world_mut().query::<&ChildOf>();
        assert!(
            parents
                .query(app.world())
                .iter_ancestors(picture)
                .any(|up| up == veil)
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(veils(&mut app).is_empty());
    }

    #[test]
    fn the_image_itself_is_never_drawn_past_its_pixels() {
        let room = Vec2::new(800.0, 600.0);
        let small = Vec2::new(200.0, 100.0);
        // On a screen of two pixels to a point.
        assert_eq!(fitted(small, room, 0.5), Vec2::new(100.0, 50.0));
        assert_eq!(
            fitted(small, room, f32::INFINITY),
            Vec2::new(800.0, 400.0)
        );
        assert_eq!(
            fitted(Vec2::new(4000.0, 6000.0), room, 0.5),
            Vec2::new(400.0, 600.0)
        );
    }
}
