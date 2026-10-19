//! The root of the window: the top bar, the dock with what plugins
//! put over it, the foot, and what floats over them.

use bevy::app::{App, First, Startup};
use bevy::camera::Camera2d;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Added;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::{
    any_component_removed, any_match_filter, on_message,
};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::ui::{AlignItems, IsDefaultUiCamera, UiRect, percent, px};
use bevy_fynix::ViewExt;
use bevy_fynix::dock::dock;
use bevy_fynix::shortcut::Scope;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, POPUP, column, frame, row,
};
use hamlet_core::core::{Settings, TestConnection};
use hamlet_realm_api::pick::Taken;
use hamlet_sdk::frame::{self, FramePart, Place};

use crate::looks::{Fills, hairline};
use crate::platform::Glow;
use crate::theme::{HamletTheme, ThemeChanged};
use crate::{AnyView, Cx, View, chrome, hover};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, open)
        // Before anything this frame queues work on the panels it
        // would despawn.
        .add_systems(
            First,
            (
                (
                    rebuild.run_if(on_message::<ThemeChanged>),
                    reframe.run_if(
                        any_match_filter::<Added<FramePart>>
                            .or_eager(
                                any_component_removed::<FramePart>,
                            ),
                    ),
                )
                    .chain(),
                hold_escape,
            ),
        );
}

/// The root the panels are mounted on, and the parts of the frame it
/// was built with.
#[derive(Resource)]
struct Panels(Entity, Vec<(FramePart, Entity)>);

/// Mounts the views, as one tab group so Tab moves through the
/// controls.
pub(crate) fn mount(world: &mut World) {
    let parts = frame::parts(world);
    let root = bevy_fynix::mount::<HamletTheme>(world, root());
    world.entity_mut(root).insert(TabGroup::default());
    world.insert_resource(Panels(root, parts));
}

/// Builds the panels again for a new theme, which each read as it
/// was built.
fn rebuild(world: &mut World) {
    let Some(Panels(root, _)) = world.remove_resource::<Panels>()
    else {
        return;
    };
    world.despawn(root);
    mount(world);
}

/// Builds the panels again when a part of the frame came or went.
fn reframe(world: &mut World) {
    let same = world
        .get_resource::<Panels>()
        .is_none_or(|panels| panels.1 == frame::parts(world));
    if !same {
        rebuild(world);
    }
}

/// Tells the world whether a popup is open as the frame begins,
/// so that the Escape that closes it does not also let go of what
/// is selected. The toolkit closes the popup itself.
pub(crate) fn hold_escape(
    open: Query<&Scope>,
    taken: Option<ResMut<Taken>>,
) {
    let Some(mut taken) = taken else {
        return;
    };
    let over = open.iter().any(|scope| scope.0 == POPUP);
    if taken.over != over {
        taken.over = over;
    }
}

fn root() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let mut shown = vec![full(cx).boxed()];
        shown.extend(frame::views(cx.world, Place::Over));
        shown.push(hover::view());
        cx.build(
            column(shown)
                .width(percent(100.0))
                .height(percent(100.0))
                .fill(cx.theme().background())
                .gap(0.0),
        )
    })
}

fn full(cx: &Cx) -> impl View {
    let mut middle = vec![dock::<HamletTheme>().boxed()];
    middle.extend(frame::views(cx.world, Place::Dock));
    let mut shown = vec![
        toolbar(cx).boxed(),
        column(middle).fills_height().gap(0.0).boxed(),
    ];
    shown.extend(frame::views(cx.world, Place::Foot));
    column(shown).fills_height().gap(0.0)
}

/// The bar across the top: the parts plugins put there, on a surface
/// that warms toward its foot.
pub fn toolbar(cx: &Cx) -> impl View {
    let palette = &cx.theme().palette;
    let mut bar = frame::views(cx.world, Place::Bar);
    bar.extend([
        // The rest of the bar is where the window is held.
        frame()
            .grow(1.0)
            .height(percent(100.0))
            .tagged(chrome::Grip)
            .boxed(),
        chrome::buttons().boxed(),
    ]);
    column((
        row(bar)
            .gap(0.0)
            .align(AlignItems::Center)
            .padding(UiRect::axes(px(8.0), px(0.0)))
            .fill(palette.black)
            .tagged(Glow)
            .tagged(chrome::Grip),
        hairline(palette.seam),
    ))
    .gap(0.0)
}

/// The window's own camera, and a first test of the connection.
fn open(world: &mut World) {
    // The panels belong to the window's camera and the world to its
    // own.
    world.spawn((Camera2d, IsDefaultUiCamera));
    let config = world.resource::<Settings>().config.clone();
    world.trigger(TestConnection(config));
}
