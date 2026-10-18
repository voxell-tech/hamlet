//! The menu of a thing in the world, asked for with the right button,
//! and the question before anything that ends an agent or closes a
//! hamlet.

mod deed;
mod founding;
mod items;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::picking::events::{Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::ui::px;
use bevy::window::Window;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, frame, label, menu_item, overlay, popup,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_core::agents::{self};
use hamlet_core::oversight::Veto;
use hamlet_realm::orders;
use hamlet_realm::pick::{Asked, AskedGround, Thing};
use hamlet_sdk::confirm::Confirm;

pub(crate) use self::deed::{Deed, question};
pub(crate) use self::founding::Founding;
use crate::{AnyView, Cx, View};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Open>()
        .init_resource::<Founding>()
        .add_observer(ask_of_row)
        .add_observer(ask_confirm)
        .add_systems(Update, answer_ask);
}

/// Asks the question a plugin gave, at the pointer.
fn ask_confirm(
    asked: On<Confirm>,
    window: Query<&Window>,
    mut open: ResMut<Open>,
) {
    let at = window
        .iter()
        .find_map(Window::cursor_position)
        .unwrap_or_default();
    *open = Open::Question(Deed::Command(asked.event().clone()), at);
}

/// The popup open over the world, and its place in the window.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) enum Open {
    #[default]
    Nothing,
    Menu(Thing, Vec2),
    Question(Deed, Vec2),
    /// The menu of the bare ground.
    Ground(Vec2),
    /// The form a hamlet is founded with.
    Founding(Vec2),
    /// The skills an agent could be given.
    Skills(Entity, Vec2),
}

/// On a row of a panel: the thing it stands for, whose menu the
/// right button opens there as it does in the world.
#[derive(Component)]
pub struct Asks(pub Thing);

pub(crate) fn ask_of_row(
    press: On<Pointer<Press>>,
    rows: Query<&Asks>,
    window: Query<&Window>,
    mut asked: ResMut<Asked>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    let at = window
        .iter()
        .find_map(Window::cursor_position)
        .unwrap_or_default();
    asked.0 = Some((row.0, at));
}

/// Opens the menu of a thing that was asked about, or of the
/// ground.
pub(crate) fn answer_ask(
    mut asked: ResMut<Asked>,
    mut ground: ResMut<AskedGround>,
    mut open: ResMut<Open>,
) {
    let ground = ground.0.take();
    if let Some((thing, at)) = asked.0.take() {
        *open = Open::Menu(thing, at);
        return;
    }
    let Some(at) = ground else {
        return;
    };
    // The click that asked about a thing reaches the world's own
    // surface too, a moment later and at the same place. It is not
    // a second question about the ground under the thing.
    let same = match &*open {
        Open::Menu(_, was) | Open::Question(_, was) => {
            was.distance(at) < 2.0
        }
        _ => false,
    };
    if !same {
        *open = Open::Ground(at);
    }
}

fn close(world: &mut World) {
    *world.resource_mut::<Open>() = Open::Nothing;
}

/// Stops what `agent` is doing now.
pub(super) fn stop(world: &mut World, agent: Entity) {
    let agent = Some(agents::name_of(world, agent));
    let chief = world.resource::<crate::me::Me>().user.clone();
    world.trigger(Veto { agent, chief });
    world.flush();
}

/// The place of the pointer in the window.
fn pointer(world: &mut World) -> Vec2 {
    world
        .query::<&bevy::window::Window>()
        .iter(world)
        .find_map(bevy::window::Window::cursor_position)
        .unwrap_or_default()
}

/// Opens the menu of `thing` at the pointer, as a right click on
/// it does.
pub(super) fn open_for(world: &mut World, thing: Thing) {
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Menu(thing, at);
}

/// Opens the form a hamlet is founded with, at the pointer.
pub(super) fn ask_found(world: &mut World) {
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Founding(at);
}

/// Asks before `deed` is done, at the pointer.
pub(crate) fn ask(world: &mut World, deed: Deed) {
    let at = pointer(world);
    *world.resource_mut::<Open>() = Open::Question(deed, at);
}

/// Brings back `agent`, which was removed or merged away.
pub(super) fn restore(world: &mut World, agent: Entity) {
    let text = format!("/restore {}", agents::name_of(world, agent));
    orders::decree(world, agent, text);
}

/// A menu of `items` at `at`.
fn menu(at: Vec2, items: Vec<AnyView>) -> impl View {
    popup(Rect::from_corners(at, at), items)
        .on_dismiss(|world, _| close(world))
        .min_width(px(200.0))
}

/// The menu or the question that is open, over everything.
pub fn view() -> AnyView {
    keyed(resource::<Open, _>(Clone::clone), |open| {
        match open.clone() {
            Open::Nothing => frame().boxed(),
            Open::Menu(thing, at) => {
                AnyView::new(move |cx: &mut Cx| {
                    let items = items::items(cx.world, thing, at);
                    if items.is_empty() {
                        return cx.build(frame());
                    }
                    cx.build(menu(at, items))
                })
            }
            Open::Question(deed, at) => question(deed, at),
            Open::Ground(at) => {
                let found =
                    menu_item(label("Found a Hamlet...").wrap(false))
                        .on_activate(move |world| {
                            *world.resource_mut::<Open>() =
                                Open::Founding(at);
                        })
                        .boxed();
                menu(at, vec![found]).boxed()
            }
            Open::Founding(at) => founding::founding(at),
            Open::Skills(agent, at) => {
                AnyView::new(move |cx: &mut Cx| {
                    let items = items::skills(cx.world, agent);
                    cx.build(menu(at, items))
                })
            }
        }
    })
    .within(overlay(()))
    .boxed()
}
