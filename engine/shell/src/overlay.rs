//! Overlays: cards over the dock, each an entity as plugins add
//! them, shown while it is [`OverlayShown`], on a backdrop that takes
//! every press so nothing under it is reached.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, overlay, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_engine_api::PanelId;

use crate::style::scroll_area;
use crate::{AnyView, Cx};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Shown>().add_systems(Update, follow);
}

/// A card over the dock.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct Overlay {
    pub id: PanelId,
    /// Builds its content, given the overlay's entity.
    pub content: fn(Entity) -> AnyView,
}

/// On an overlay while it is shown.
#[derive(Component, Clone, Copy, Debug)]
pub struct OverlayShown;

/// The overlay shown: the first that is, by its entity.
#[derive(Resource, Clone, Copy, PartialEq, Default)]
struct Shown(Option<Entity>);

/// The overlay shown, over everything but the menus, or nothing.
pub fn view() -> AnyView {
    keyed(resource::<Shown, _>(|shown| shown.0), |shown| {
        let Some(shown) = *shown else {
            return frame().boxed();
        };
        AnyView::new(move |cx: &mut Cx| {
            let content = cx
                .world
                .get::<Overlay>(shown)
                .map(|overlay| overlay.content);
            let body = match content {
                Some(content) => content(shown),
                None => column(()).boxed(),
            };
            let theme = cx.theme();
            let space = &theme.space;
            let (padding, width, border, radius, inset) = (
                space.overlay_padding,
                space.overlay_width,
                space.border,
                space.menu_radius,
                space.panel,
            );
            let (fill, line, backdrop, shadow) = (
                *theme.colors.surface,
                *theme.colors.border_strong,
                *theme.colors.shadow,
                theme.drop_shadow(),
            );
            let card = column((scroll_area((column((body,))
                .gap(0.0)
                .padding(UiRect::all(px(padding))),)),))
            .width(percent(100.0))
            .max_width(px(width))
            .max_height(percent(100.0))
            .gap(0.0)
            .fill(fill)
            .border(border)
            .border_color(line)
            .radius(radius)
            .tagged(shadow);
            // Filled, so no press reaches what is under it. A row, so
            // the card is as tall as its words once they wrap to its
            // width.
            cx.build(
                row((card,))
                    .width(percent(100.0))
                    .height(percent(100.0))
                    .padding(UiRect::all(px(inset)))
                    .align(AlignItems::Center)
                    .justify(JustifyContent::Center)
                    .fill(backdrop),
            )
        })
    })
    .within(overlay(()))
    .boxed()
}

/// Finds the overlay to show again when one is shown, hidden or
/// gone.
fn follow(
    added: Query<(), Added<OverlayShown>>,
    mut hidden: RemovedComponents<OverlayShown>,
    mut gone: RemovedComponents<Overlay>,
    all: Query<Entity, (With<Overlay>, With<OverlayShown>)>,
    mut shown: ResMut<Shown>,
) {
    let went = hidden.read().count() + gone.read().count() > 0;
    if added.is_empty() && !went {
        return;
    }
    let now = Shown(all.iter().min());
    if *shown != now {
        *shown = now;
    }
}
