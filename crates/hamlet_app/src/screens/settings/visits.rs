//! The Settings pages gone through, to go back and forward among
//! them: with the primary key and `[` or `]`, or the mouse's back and
//! forward buttons, while the pointer is over Settings.

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use bevy_fynix::shortcut::{
    CommandId, CommandSpec, Layer, ScopeId, ScopeSpec, ShortcutAppExt,
};
use hamlet_ui::widgets::keys::Keys;

use super::Pane;
use crate::trail::{
    Trail, Way, mouse_pressed, mouse_way, pointer_over,
};

/// The keys of Settings, in force while the pointer is over it.
pub(super) const KEYS: ScopeId = ScopeId("settings");

pub(super) fn plugin(app: &mut App) {
    let command = |id, label, run| CommandSpec {
        id: CommandId(id),
        label,
        scope: KEYS,
        run,
        enabled: |_| true,
        repeat: false,
    };
    let primary = |key| Keys::new(key).primary().chord();
    app.init_resource::<Visits>()
        .add_scope(ScopeSpec {
            id: KEYS,
            label: "Settings",
            layer: Layer::Panel,
            active: None,
        })
        .add_command(
            command("settings.back", "Back", |world, _| {
                travel(world, Way::Back);
            }),
            &[primary(KeyCode::BracketLeft)],
        )
        .add_command(
            command("settings.forward", "Forward", |world, _| {
                travel(world, Way::Forward);
            }),
            &[primary(KeyCode::BracketRight)],
        )
        .add_systems(
            Update,
            (mouse_travel.run_if(mouse_pressed), record).chain(),
        );
}

/// The pages behind the one shown and those ahead of it.
#[derive(Resource, Default)]
struct Visits(Trail<Pane>);

/// Keeps the page left behind whenever Settings turns to another.
fn record(pane: Res<Pane>, mut visits: ResMut<Visits>) {
    visits.0.keep(*pane);
}

/// Turns Settings to the page behind, or the one ahead.
fn travel(world: &mut World, way: Way) {
    let now = *world.resource::<Pane>();
    let next =
        world.resource_mut::<Visits>().0.travel(way, now, |_| true);
    if let Some(next) = next {
        *world.resource_mut::<Pane>() = next;
    }
}

/// Turns Settings by the mouse's back or forward button, when the
/// pointer is over it.
fn mouse_travel(world: &mut World) {
    if let Some(way) = mouse_way(world)
        && pointer_over(world, KEYS)
    {
        travel(world, way);
    }
}
