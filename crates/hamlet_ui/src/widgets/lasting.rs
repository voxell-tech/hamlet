//! Nodes whose scroll and focus pass to their rebuild with the same
//! key, as when a panel is built again for a new theme.

use bevy::app::{App, Last};
use bevy::ecs::component::Component;
use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::ui::ScrollPosition;

pub fn plugin(app: &mut App) {
    app.init_resource::<Lasted>()
        .add_observer(outlast)
        .add_observer(last_on)
        .add_systems(Last, forget);
}

/// A node whose scroll and focus pass to its rebuild with the same
/// key.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
pub struct Lasting(pub String);

/// The scrolls and focus of [`Lasting`] nodes despawned this frame.
#[derive(Resource, Default)]
struct Lasted {
    scrolls: Vec<(String, ScrollPosition)>,
    focus: Option<String>,
}

fn outlast(
    gone: On<Remove<Lasting>>,
    nodes: Query<(&Lasting, Option<&ScrollPosition>)>,
    focus: Res<InputFocus>,
    mut lasted: ResMut<Lasted>,
) {
    let Ok((lasting, scroll)) = nodes.get(gone.entity) else {
        return;
    };
    if let Some(scroll) = scroll {
        lasted.scrolls.push((lasting.0.clone(), scroll.clone()));
    }
    if focus.get() == Some(gone.entity) {
        lasted.focus = Some(lasting.0.clone());
    }
}

fn last_on(
    built: On<Add<Lasting>>,
    nodes: Query<&Lasting>,
    mut lasted: ResMut<Lasted>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let Ok(Lasting(key)) = nodes.get(built.entity) else {
        return;
    };
    if let Some(at) =
        lasted.scrolls.iter().position(|(kept, _)| kept == key)
    {
        let (_, scroll) = lasted.scrolls.swap_remove(at);
        commands.entity(built.entity).insert(scroll);
    }
    if lasted.focus.as_ref() == Some(key) {
        lasted.focus = None;
        focus.set(built.entity, FocusCause::Auto);
    }
}

/// A node not built again by the end of its frame is gone for good.
fn forget(mut lasted: ResMut<Lasted>) {
    if !lasted.scrolls.is_empty() || lasted.focus.is_some() {
        *lasted = Lasted::default();
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;

    #[test]
    fn a_lasting_node_built_again_keeps_its_scroll_and_focus() {
        let mut app = App::new();
        app.init_resource::<InputFocus>().add_plugins(plugin);
        let lasting = || Lasting("page".into());
        let world = app.world_mut();
        let old = world
            .spawn((lasting(), ScrollPosition(Vec2::new(0.0, 240.0))))
            .id();
        world
            .resource_mut::<InputFocus>()
            .set(old, FocusCause::Navigated);
        world.despawn(old);
        let new =
            world.spawn((lasting(), ScrollPosition::default())).id();
        world.flush();

        assert_eq!(
            world.get::<ScrollPosition>(new).unwrap().y,
            240.0
        );
        assert_eq!(world.resource::<InputFocus>().get(), Some(new));
    }
}
